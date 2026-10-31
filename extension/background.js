const LIST_URL = "https://easylist.to/easylist/easylist.txt";

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
let wasm, engine;

function pass(str) {
  const bytes = encoder.encode(str);
  const ptr = wasm.alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
  return [ptr, bytes.length];
}

// ponytail: list is fetched on every startup and requests before it loads pass through; cache it in storage if that matters
async function init() {
  const [bytes, rules] = await Promise.all([
    fetch(browser.runtime.getURL("adblock.wasm")).then((r) => r.arrayBuffer()),
    fetch(LIST_URL).then((r) => r.text()),
  ]);
  wasm = (await WebAssembly.instantiate(bytes)).instance.exports;
  const [ptr, len] = pass(rules);
  engine = wasm.engine_new(ptr, len);
  wasm.dealloc(ptr, len);
}

browser.webRequest.onBeforeRequest.addListener(
  (details) => {
    if (!engine) return;
    const [url, urlLen] = pass(details.url);
    const [source, sourceLen] = pass(details.documentUrl || details.originUrl || details.url);
    const blocked = wasm.should_block(engine, url, urlLen, source, sourceLen, TYPES[details.type] ?? 10);
    wasm.dealloc(url, urlLen);
    wasm.dealloc(source, sourceLen);
    if (blocked) return { cancel: true };
  },
  { urls: ["<all_urls>"] },
  ["blocking"],
);

init();
