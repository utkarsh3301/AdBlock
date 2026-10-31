use crate::{Engine, RequestType};

const TYPES: [RequestType; 11] = [
    RequestType::Document,
    RequestType::Subdocument,
    RequestType::Script,
    RequestType::Stylesheet,
    RequestType::Image,
    RequestType::Font,
    RequestType::Media,
    RequestType::Xhr,
    RequestType::Websocket,
    RequestType::Ping,
    RequestType::Other,
];

unsafe fn str_from<'a>(ptr: *const u8, len: usize) -> &'a str {
    std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("")
}

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len));
}

#[no_mangle]
pub unsafe extern "C" fn engine_new(rules: *const u8, len: usize) -> *mut Engine {
    Box::into_raw(Box::new(Engine::new(str_from(rules, len))))
}

#[no_mangle]
pub unsafe extern "C" fn should_block(
    engine: *const Engine,
    url: *const u8,
    url_len: usize,
    source: *const u8,
    source_len: usize,
    kind: usize,
) -> bool {
    let kind = TYPES.get(kind).copied().unwrap_or(RequestType::Other);
    (*engine).should_block(str_from(url, url_len), str_from(source, source_len), kind)
}

// Returns ptr << 32 | len of a buffer the caller frees with dealloc(ptr, len).
#[no_mangle]
pub unsafe extern "C" fn cosmetic_css(engine: *const Engine, url: *const u8, url_len: usize) -> u64 {
    let css = (*engine).cosmetic_css(str_from(url, url_len)).into_bytes().into_boxed_slice();
    let len = css.len();
    ((Box::into_raw(css) as *mut u8 as u64) << 32) | len as u64
}
