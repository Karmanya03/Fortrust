use fortrust_dom::{DomArena, parse_html};
use fortrust_js::{JsRuntime, EventLoop};

#[test]
fn boa_document_query_selector_roundtrip() {
    // Leak the arena so we can get a 'static Document reference for the runtime bindings.
    let arena_box = Box::new(DomArena::new());
    let arena: &'static DomArena = Box::leak(arena_box);

    let doc = parse_html(arena, "<!doctype html><html><body><h1>Hello Boa</h1></body></html>")
        .expect("parse html");

    let mut runtime = JsRuntime::new();
    runtime.attach_document(&doc).expect("attach document");

    let mut loop_handle = EventLoop::new();
    runtime.initialize(&mut loop_handle).expect("init runtime");

    let value = runtime.eval("document.querySelector('h1').textContent").expect("eval");
    let s = value.to_string(runtime.context()).expect("to string").to_std_string_escaped();
    assert_eq!(s, "Hello Boa");
}
