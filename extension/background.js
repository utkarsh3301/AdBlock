const LISTS = [
  "https://easylist.to/easylist/easylist.txt",
  "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters.txt",
  "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/quick-fixes.txt",
  "filters.txt",
];

// Index into RequestType in src/wasm.rs; anything missing maps to Other.
const TYPES = {
  main_frame: 0,
  sub_frame: 1,
  script: 2,
  stylesheet: 3,
  image: 4,
  imageset: 4,
  font: 5,
  media: 6,
  xmlhttprequest: 7,
  websocket: 8,
  ping: 9,
  beacon: 9,
};

const encoder = new TextEncoder();
const decoder = new TextDecoder();
let wasm, engine;

function pass(str) {
  const bytes = encoder.encode(str);
  const ptr = wasm.alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
  return [ptr, bytes.length];
}

function take(packed) {
  const ptr = Number(packed >> 32n);
  const len = Number(packed & 0xffffffffn);
  const str = decoder.decode(new Uint8Array(wasm.memory.buffer, ptr, len));
  wasm.dealloc(ptr, len);
  return str;
}

// ponytail: lists are fetched on every startup and requests before they load pass through; cache them in storage if that matters
async function init() {
  const [bytes, ...lists] = await Promise.all([
    fetch(browser.runtime.getURL("adblock.wasm")).then((r) => r.arrayBuffer()),
    ...LISTS.map((u) =>
      fetch(u.startsWith("https:") ? u : browser.runtime.getURL(u))
        .then((r) => r.text())
        .catch(() => ""),
    ),
  ]);
  wasm = (await WebAssembly.instantiate(bytes)).instance.exports;
  const [ptr, len] = pass(lists.join("\n"));
  engine = wasm.engine_new(ptr, len);
  wasm.dealloc(ptr, len);
}

// ponytail: assumes UTF-8 bodies, fine for the JSON and HTML these filters target
function rewrite(requestId, parts) {
  const filter = browser.webRequest.filterResponseData(requestId);
  const utf8 = new TextDecoder();
  let text = "";
  filter.ondata = (e) => {
    text += utf8.decode(e.data, { stream: true });
  };
  filter.onstop = () => {
    text += utf8.decode();
    for (let i = 0; i + 2 < parts.length; i += 3) {
      text = parts[i + 2] ? text.replaceAll(parts[i], parts[i + 1]) : text.replace(parts[i], parts[i + 1]);
    }
    filter.write(encoder.encode(text));
    filter.close();
  };
}

browser.webRequest.onBeforeRequest.addListener(
  (details) => {
    if (!engine) return;
    const type = TYPES[details.type] ?? 10;
    const sourceUrl = details.type === "main_frame" ? details.url : details.documentUrl || details.originUrl || details.url;
    const [url, urlLen] = pass(details.url);
    const [source, sourceLen] = pass(sourceUrl);
    const blocked = wasm.should_block(engine, url, urlLen, source, sourceLen, type);
    const reps = blocked ? "" : take(wasm.replacements(engine, url, urlLen, source, sourceLen, type));
    wasm.dealloc(url, urlLen);
    wasm.dealloc(source, sourceLen);
    if (blocked) return { cancel: true };
    if (reps) rewrite(details.requestId, reps.split("\0"));
  },
  { urls: ["<all_urls>"] },
  ["blocking"],
);

// ponytail: every frame gets the full ~520 KB generic stylesheet; match generic selectors against the page's
// classes and ids instead if pages feel slower
browser.webNavigation.onCommitted.addListener(({ tabId, frameId, url }) => {
  if (!engine || !url.startsWith("http")) return;
  const [ptr, len] = pass(url);
  const css = take(wasm.cosmetic_css(engine, ptr, len));
  wasm.dealloc(ptr, len);
  if (!css) return;
  browser.tabs
    .insertCSS(tabId, { code: css, frameId, cssOrigin: "user", runAt: "document_start" })
    .catch(() => {});
});

init();
