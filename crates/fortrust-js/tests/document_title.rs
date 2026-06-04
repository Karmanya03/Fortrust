use fortrust_dom::{DomArena, parse_html};
use fortrust_js::{JsRuntime, EventLoop};

#[test]
fn document_title_setter_getter_roundtrip() {
    let arena_box = Box::new(DomArena::new());
    let arena: &'static DomArena = Box::leak(arena_box);

    let doc = parse_html(arena, "<!doctype html><html><head><title>Initial</title></head><body></body></html>")
        .expect("parse html");

    let mut runtime = JsRuntime::new();
    runtime.attach_document(&doc).expect("attach document");

    let mut loop_handle = EventLoop::new();
    runtime.initialize(&mut loop_handle).expect("init runtime");

    // Set new title from JS
    runtime.eval("document.setTitle('New Title')").expect("set title");
    let val = runtime.eval("document.getTitle()").expect("get title");
    let s = val.to_string(runtime.context()).expect("to string").to_std_string_escaped();
    assert_eq!(s, "New Title");
}
