use fortrust_style::StyleEngine;
use fortrust_dom::{DomArena, parse_html};

#[test]
fn computes_display_from_ua_and_rules() {
    let arena = DomArena::new();
    let doc = parse_html(&arena, "<html><body><p id=para>hello</p></body></html>").unwrap();
    let p = doc.first_element_by_tag("p").unwrap();

    let engine = StyleEngine::new();
    let style = engine.compute_style(p, None);
    // UA defaults set p as block
    assert_eq!(style.display, fortrust_style::Display::Block);
}
