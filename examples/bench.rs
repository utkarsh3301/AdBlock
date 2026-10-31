use adblock::{Engine, RequestType};
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("usage: bench <filter list>");
    let rules = std::fs::read_to_string(path).expect("read filter list");
    let network_lines = rules
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with(['!', '[']) && !l.contains('#'))
        .count();

    let t = Instant::now();
    let engine = Engine::new(&rules);
    println!("parsed {} of {} network lines in {:?}", engine.filter_count(), network_lines, t.elapsed());

    let ad_hosts = rules
        .lines()
        .filter_map(|l| l.strip_prefix("||"))
        .map(|l| &l[..l.find(['^', '/', '$', '*']).unwrap_or(l.len())])
        .take(5000);
    let mut urls: Vec<String> = ad_hosts.map(|h| format!("https://{h}/ad.js")).collect();
    urls.extend((0..5000).map(|i| format!("https://cdn{i}.example.org/static/app{i}.js?v={i}")));

    let t = Instant::now();
    let blocked = urls
        .iter()
        .filter(|u| engine.should_block(u, "https://www.news.com/", RequestType::Script))
        .count();
    let elapsed = t.elapsed();
    println!(
        "{} requests, {} blocked, {:?} total, {:?} per request",
        urls.len(),
        blocked,
        elapsed,
        elapsed / urls.len() as u32
    );
}
