use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RequestType {
    Document,
    Subdocument,
    Script,
    Stylesheet,
    Image,
    Font,
    Media,
    Xhr,
    Websocket,
    Ping,
    Other,
}

const ALL_TYPES: u16 = (1 << 11) - 1;

impl RequestType {
    fn bit(self) -> u16 {
        1 << self as u16
    }

    fn from_option(name: &str) -> Option<Self> {
        Some(match name {
            "document" | "doc" => Self::Document,
            "subdocument" | "frame" => Self::Subdocument,
            "script" => Self::Script,
            "stylesheet" | "css" => Self::Stylesheet,
            "image" => Self::Image,
            "font" => Self::Font,
            "media" => Self::Media,
            "xmlhttprequest" | "xhr" => Self::Xhr,
            "websocket" => Self::Websocket,
            "ping" => Self::Ping,
            "other" => Self::Other,
            _ => return None,
        })
    }
}

struct Request<'a> {
    url: &'a str,
    url_lower: String,
    host: (usize, usize),
    source_host: String,
    third_party: bool,
    kind: RequestType,
}

impl<'a> Request<'a> {
    fn new(url: &'a str, source_url: &str, kind: RequestType) -> Self {
        let host = host_range(url);
        let (s, e) = host_range(source_url);
        let source_host = source_url[s..e].to_ascii_lowercase();
        let url_lower = url.to_ascii_lowercase();
        let third_party = base_domain(&url_lower[host.0..host.1]) != base_domain(&source_host);
        Self { url, url_lower, host, source_host, third_party, kind }
    }
}

// ponytail: ignores userinfo and IPv6 hosts, swap in a real URL parser if those show up
fn host_range(url: &str) -> (usize, usize) {
    let start = url.find("://").map_or(0, |i| i + 3);
    let rest = &url[start..];
    let end = rest.find(|c: char| matches!(c, '/' | '?' | '#' | ':')).unwrap_or(rest.len());
    (start, start + end)
}

// ponytail: last two labels, wrong for suffixes like co.uk; use the Public Suffix List when that matters
fn base_domain(host: &str) -> &str {
    host.rmatch_indices('.').nth(1).map_or(host, |(i, _)| &host[i + 1..])
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host.strip_suffix(domain).is_some_and(|r| r.is_empty() || r.ends_with('.'))
}

fn is_separator(c: u8) -> bool {
    !(c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b'%'))
}

// ponytail: backtracking on '*', exponential on pathological patterns; switch to an NFA if a list triggers it
fn glob(p: &[u8], s: &[u8], end_anchor: bool) -> bool {
    match p.split_first() {
        None => !end_anchor || s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|i| glob(rest, &s[i..], end_anchor)),
        Some((b'^', rest)) => match s.split_first() {
            None => glob(rest, s, end_anchor),
            Some((&c, tail)) => is_separator(c) && glob(rest, tail, end_anchor),
        },
        Some((&c, rest)) => s.first() == Some(&c) && glob(rest, &s[1..], end_anchor),
    }
}

#[derive(Default)]
struct Filter {
    pattern: String,
    host_anchor: bool,
    end_anchor: bool,
    exception: bool,
    important: bool,
    match_case: bool,
    third_party: Option<bool>,
    types: u16,
    domains: Vec<String>,
    not_domains: Vec<String>,
}

impl Filter {
    fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with('!')
            || line.starts_with('[')
            || ["##", "#@#", "#?#", "#$#", "#%#"].iter().any(|m| line.contains(m))
        {
            return None;
        }
        let mut f = Filter::default();
        let line = match line.strip_prefix("@@") {
            Some(rest) => {
                f.exception = true;
                rest
            }
            None => line,
        };
        let (mut pattern, options) = match line.rfind('$') {
            Some(i) => (&line[..i], Some(&line[i + 1..])),
            None => (line, None),
        };
        // ponytail: regex filters are skipped, add a matcher if lists lean on them
        if pattern.len() > 1 && pattern.starts_with('/') && pattern.ends_with('/') {
            return None;
        }
        if options.is_none() && pattern.trim_matches('*').is_empty() {
            return None;
        }

        let (mut include, mut exclude) = (0u16, 0u16);
        for opt in options.into_iter().flat_map(|o| o.split(',')) {
            let (neg, name) = match opt.strip_prefix('~') {
                Some(n) => (true, n),
                None => (false, opt),
            };
            match name {
                "third-party" | "3p" => f.third_party = Some(!neg),
                "first-party" | "1p" => f.third_party = Some(neg),
                "important" => f.important = true,
                "match-case" => f.match_case = true,
                _ => {
                    if let Some(list) = name.strip_prefix("domain=").or(name.strip_prefix("from=")) {
                        for d in list.split('|') {
                            match d.strip_prefix('~') {
                                Some(d) => f.not_domains.push(d.to_ascii_lowercase()),
                                None => f.domains.push(d.to_ascii_lowercase()),
                            }
                        }
                    } else if let Some(t) = RequestType::from_option(name) {
                        if neg {
                            exclude |= t.bit()
                        } else {
                            include |= t.bit()
                        }
                    } else {
                        // Unknown option: dropping the filter is safer than applying it too broadly.
                        return None;
                    }
                }
            }
        }
        f.types = (if include != 0 { include } else { ALL_TYPES }) & !exclude;

        if let Some(p) = pattern.strip_prefix("||") {
            f.host_anchor = true;
            pattern = p;
        } else if let Some(p) = pattern.strip_prefix('|') {
            pattern = p;
        } else {
            f.pattern.push('*');
        }
        if let Some(p) = pattern.strip_suffix('|') {
            f.end_anchor = true;
            pattern = p;
        }
        if f.match_case {
            f.pattern.push_str(pattern);
        } else {
            f.pattern.push_str(&pattern.to_ascii_lowercase());
        }
        Some(f)
    }

    // Tokens that must appear whole in any URL this filter matches: not touching a '*' and not cut off
    // by an unanchored pattern end.
    fn tokens(&self) -> impl Iterator<Item = u64> + '_ {
        let p = self.pattern.as_bytes();
        token_spans(p)
            .filter(move |&(s, e)| {
                (s == 0 || p[s - 1] != b'*') && if e == p.len() { self.end_anchor } else { p[e] != b'*' }
            })
            .map(move |(s, e)| hash(&p[s..e]))
    }

    fn matches(&self, req: &Request) -> bool {
        if self.types & req.kind.bit() == 0
            || self.third_party.is_some_and(|tp| tp != req.third_party)
            || (!self.domains.is_empty() && !self.domains.iter().any(|d| domain_matches(&req.source_host, d)))
            || self.not_domains.iter().any(|d| domain_matches(&req.source_host, d))
        {
            return false;
        }
        let url = if self.match_case { req.url } else { &req.url_lower }.as_bytes();
        let p = self.pattern.as_bytes();
        if !self.host_anchor {
            return glob(p, url, self.end_anchor);
        }
        let (start, end) = req.host;
        std::iter::once(start)
            .chain((start..end).filter(|&i| url[i] == b'.').map(|i| i + 1))
            .any(|i| glob(p, &url[i..], self.end_anchor))
    }
}

fn token_spans(s: &[u8]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < s.len() && !s[i].is_ascii_alphanumeric() {
            i += 1;
        }
        let start = i;
        while i < s.len() && s[i].is_ascii_alphanumeric() {
            i += 1;
        }
        (start < i).then_some((start, i))
    })
}

fn hash(token: &[u8]) -> u64 {
    token
        .iter()
        .fold(0xcbf29ce484222325, |h, &c| (h ^ c.to_ascii_lowercase() as u64).wrapping_mul(0x100000001b3))
}

// Filters are bucketed under their rarest token. A request only checks the buckets for tokens in its
// URL, plus bucket 0 for filters with no usable token.
struct Index {
    buckets: HashMap<u64, Vec<Filter>>,
}

impl Index {
    fn new(filters: Vec<Filter>) -> Self {
        let mut counts: HashMap<u64, usize> = HashMap::new();
        for t in filters.iter().flat_map(Filter::tokens) {
            *counts.entry(t).or_default() += 1;
        }
        let mut buckets: HashMap<u64, Vec<Filter>> = HashMap::new();
        for f in filters {
            let key = f.tokens().min_by_key(|t| counts[t]).unwrap_or(0);
            buckets.entry(key).or_default().push(f);
        }
        Self { buckets }
    }

    fn any(&self, tokens: &[u64], req: &Request) -> bool {
        tokens.iter().chain(&[0]).filter_map(|t| self.buckets.get(t)).flatten().any(|f| f.matches(req))
    }
}

pub struct Engine {
    important: Index,
    blocks: Index,
    exceptions: Index,
}

impl Engine {
    pub fn new(rules: &str) -> Self {
        let (mut important, mut blocks, mut exceptions) = (vec![], vec![], vec![]);
        for f in rules.lines().filter_map(Filter::parse) {
            match (f.exception, f.important) {
                (true, _) => exceptions.push(f),
                (false, true) => important.push(f),
                (false, false) => blocks.push(f),
            }
        }
        Self { important: Index::new(important), blocks: Index::new(blocks), exceptions: Index::new(exceptions) }
    }

    pub fn filter_count(&self) -> usize {
        [&self.important, &self.blocks, &self.exceptions].iter().flat_map(|i| i.buckets.values()).map(Vec::len).sum()
    }

    pub fn should_block(&self, url: &str, source_url: &str, kind: RequestType) -> bool {
        let req = Request::new(url, source_url, kind);
        let url = req.url_lower.as_bytes();
        let mut tokens: Vec<u64> = token_spans(url).map(|(s, e)| hash(&url[s..e])).collect();
        tokens.sort_unstable();
        tokens.dedup();
        self.important.any(&tokens, &req)
            || (self.blocks.any(&tokens, &req) && !self.exceptions.any(&tokens, &req))
    }
}

#[cfg(test)]
mod tests {
    use super::RequestType::*;
    use super::*;

    #[test]
    fn network_filters() {
        let e = Engine::new(
            "
! comment
||ads.example.com^
/banner/*.gif
||tracker.net^$third-party
@@||ads.example.com/allowed^
||cdn.site.org/ad.js$script,domain=news.com|~sports.news.com
||evil.com^$important
@@||evil.com^
||bad.com^$unknown-option
example.com##.ad
",
        );
        let b = |u: &str, s: &str, t| e.should_block(u, s, t);

        assert!(b("https://ads.example.com/x.js", "https://a.com/", Script));
        assert!(b("https://sub.ads.example.com/x", "https://a.com/", Image));
        assert!(b("https://ADS.Example.com/X", "https://a.com/", Image));
        assert!(!b("https://notads.example.com/x", "https://a.com/", Image));
        assert!(!b("https://ads.example.com.evil/x", "https://a.com/", Image));
        assert!(!b("https://ads.example.com/allowed/1", "https://a.com/", Image));

        assert!(b("http://x.com/banner/top.gif", "http://x.com/", Image));
        assert!(!b("http://x.com/banner/top.png", "http://x.com/", Image));

        assert!(b("https://tracker.net/p", "https://blog.com/", Ping));
        assert!(!b("https://tracker.net/p", "https://www.tracker.net/", Ping));

        assert!(b("https://cdn.site.org/ad.js", "https://www.news.com/", Script));
        assert!(!b("https://cdn.site.org/ad.js", "https://www.news.com/", Image));
        assert!(!b("https://cdn.site.org/ad.js", "https://sports.news.com/", Script));
        assert!(!b("https://cdn.site.org/ad.js", "https://other.com/", Script));

        assert!(b("https://evil.com/", "https://a.com/", Script));
        assert!(!b("https://bad.com/", "https://a.com/", Script));
        assert!(!b("https://example.com/", "https://a.com/", Document));
    }
}
