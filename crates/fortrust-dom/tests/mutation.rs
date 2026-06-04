use fortrust_dom::{
    DomArena, DomError, MutationLog, MutationRecord, MutationType, NodeKind, parse_html,
};

// ─── Basic append_child / remove_child (existing behavior) ──────────────────

#[test]
fn element_attribute_mutation_and_append_child() {
    let arena = DomArena::new();
    let doc = parse_html(&arena, "<div id=parent></div><span id=child>txt</span>").unwrap();

    let parent = doc
        .descendants()
        .into_iter()
        .find(|n| {
            n.as_element()
                .map(|e| e.local_name() == "div")
                .unwrap_or(false)
        })
        .expect("parent div");
    let child = doc
        .descendants()
        .into_iter()
        .find(|n| {
            n.as_element()
                .map(|e| e.local_name() == "span")
                .unwrap_or(false)
        })
        .expect("child span");

    // set and remove attributes on the child
    let elem = child.as_element().unwrap();
    elem.set_attr("data-test", "42");
    assert_eq!(elem.attr("data-test").as_deref(), Some("42"));
    elem.remove_attr("data-test");
    assert_eq!(elem.attr("data-test"), None);

    // append child to parent
    parent.append_child(child);
    assert!(parent.children().iter().any(|c| std::ptr::eq(*c, child)));
    // remove child
    parent.remove_child(child);
    assert!(!parent.children().iter().any(|c| std::ptr::eq(*c, child)));
}

// ─── createElement + createTextNode + createComment ─────────────────────────

#[test]
fn create_element_basic() {
    let arena = DomArena::new();
    let node = arena.create_element("div");
    let el = node.as_element().unwrap();
    assert_eq!(el.local_name(), "div");
    assert!(node.children().is_empty());
    assert!(node.parent().is_none());
}

#[test]
fn create_text_node_basic() {
    let arena = DomArena::new();
    let node = arena.create_text_node("hello world");
    match node.kind() {
        NodeKind::Text(t) => assert_eq!(t.borrow().as_str(), "hello world"),
        _ => panic!("expected text node"),
    }
}

#[test]
fn create_comment_basic() {
    let arena = DomArena::new();
    let node = arena.create_comment("TODO: fix this");
    match node.kind() {
        NodeKind::Comment(c) => assert_eq!(c.as_str(), "TODO: fix this"),
        _ => panic!("expected comment node"),
    }
}

// ─── appendChild tests ──────────────────────────────────────────────────────

#[test]
fn append_child_basic() {
    let arena = DomArena::new();
    let parent = arena.create_element("ul");
    let child = arena.create_element("li");

    parent.append_child(child);

    assert_eq!(parent.children().len(), 1);
    assert!(std::ptr::eq(parent.children()[0], child));
    assert!(child.parent().is_some());
    assert!(std::ptr::eq(child.parent().unwrap(), parent));
}

#[test]
fn append_child_moves_between_parents() {
    let arena = DomArena::new();
    let parent1 = arena.create_element("div");
    let parent2 = arena.create_element("section");
    let child = arena.create_element("span");

    parent1.append_child(child);
    assert_eq!(parent1.children().len(), 1);

    // Move child to parent2
    parent2.append_child(child);
    assert_eq!(parent1.children().len(), 0);
    assert_eq!(parent2.children().len(), 1);
    assert!(std::ptr::eq(child.parent().unwrap(), parent2));
}

#[test]
fn append_child_checked_prevents_cycle() {
    let arena = DomArena::new();
    let grandparent = arena.create_element("div");
    let parent = arena.create_element("section");
    let child = arena.create_element("span");

    grandparent.append_child(parent);
    parent.append_child(child);

    // Trying to append grandparent as child of its descendant should fail
    let result = child.append_child_checked(grandparent);
    assert_eq!(result, Err(DomError::HierarchyRequest));

    // Trying to append a node to itself should fail
    let result = parent.append_child_checked(parent);
    assert_eq!(result, Err(DomError::HierarchyRequest));
}

// ─── removeChild tests ──────────────────────────────────────────────────────

#[test]
fn remove_child_basic() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let child1 = arena.create_element("p");
    let child2 = arena.create_element("span");

    parent.append_child(child1);
    parent.append_child(child2);
    assert_eq!(parent.children().len(), 2);

    parent.remove_child(child1);
    assert_eq!(parent.children().len(), 1);
    assert!(std::ptr::eq(parent.children()[0], child2));
    assert!(child1.parent().is_none());
}

#[test]
fn remove_child_checked_errors_on_wrong_parent() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let other = arena.create_element("section");
    let child = arena.create_element("span");

    parent.append_child(child);

    // Trying to remove from wrong parent should fail
    let result = other.remove_child_checked(child);
    assert_eq!(result, Err(DomError::NotFound));

    // Remove from correct parent should succeed
    let result = parent.remove_child_checked(child);
    assert_eq!(result, Ok(()));
    assert!(child.parent().is_none());
}

// ─── insertBefore tests ─────────────────────────────────────────────────────

#[test]
fn insert_before_at_beginning() {
    let arena = DomArena::new();
    let parent = arena.create_element("ul");
    let first = arena.create_element("li");
    let new_first = arena.create_element("li");

    parent.append_child(first);
    parent.insert_before(new_first, Some(first)).unwrap();

    let children = parent.children();
    assert_eq!(children.len(), 2);
    assert!(std::ptr::eq(children[0], new_first));
    assert!(std::ptr::eq(children[1], first));
}

#[test]
fn insert_before_at_middle() {
    let arena = DomArena::new();
    let parent = arena.create_element("ul");
    let a = arena.create_element("li");
    let b = arena.create_element("li");
    let mid = arena.create_element("li");

    parent.append_child(a);
    parent.append_child(b);
    parent.insert_before(mid, Some(b)).unwrap();

    let children = parent.children();
    assert_eq!(children.len(), 3);
    assert!(std::ptr::eq(children[0], a));
    assert!(std::ptr::eq(children[1], mid));
    assert!(std::ptr::eq(children[2], b));
}

#[test]
fn insert_before_none_appends() {
    let arena = DomArena::new();
    let parent = arena.create_element("ul");
    let existing = arena.create_element("li");
    let new_child = arena.create_element("li");

    parent.append_child(existing);
    parent.insert_before(new_child, None).unwrap();

    let children = parent.children();
    assert_eq!(children.len(), 2);
    assert!(std::ptr::eq(children[0], existing));
    assert!(std::ptr::eq(children[1], new_child));
}

#[test]
fn insert_before_invalid_reference_errors() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let child = arena.create_element("span");
    let unrelated = arena.create_element("p");
    let new_child = arena.create_element("a");

    parent.append_child(child);

    // unrelated is not a child of parent
    let result = parent.insert_before(new_child, Some(unrelated));
    assert_eq!(result, Err(DomError::NotFound));
}

#[test]
fn insert_before_prevents_cycle() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let child = arena.create_element("span");

    parent.append_child(child);

    let result = child.insert_before(parent, Some(child));
    assert_eq!(result, Err(DomError::HierarchyRequest));
}

// ─── replaceChild tests ─────────────────────────────────────────────────────

#[test]
fn replace_child_basic() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let old = arena.create_element("span");
    let new = arena.create_element("strong");

    parent.append_child(old);
    let removed = parent.replace_child(new, old).unwrap();

    assert!(std::ptr::eq(removed, old));
    assert!(old.parent().is_none());
    assert_eq!(parent.children().len(), 1);
    assert!(std::ptr::eq(parent.children()[0], new));
    assert!(std::ptr::eq(new.parent().unwrap(), parent));
}

#[test]
fn replace_child_not_found_errors() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let child = arena.create_element("span");
    let new = arena.create_element("strong");
    let unrelated = arena.create_element("p");

    parent.append_child(child);

    let result = parent.replace_child(new, unrelated);
    assert!(matches!(result, Err(DomError::NotFound)));
}

#[test]
fn replace_child_prevents_cycle() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let child = arena.create_element("span");

    parent.append_child(child);

    let result = child.replace_child(parent, child);
    assert!(matches!(result, Err(DomError::HierarchyRequest)));
}

// ─── set_text_content tests ─────────────────────────────────────────────────

#[test]
fn set_text_content_clears_children() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let child1 = arena.create_element("p");
    let child2 = arena.create_element("span");

    parent.append_child(child1);
    parent.append_child(child2);
    assert_eq!(parent.children().len(), 2);

    parent.set_text_content(&arena, "hello");

    assert_eq!(parent.children().len(), 1);
    match parent.children()[0].kind() {
        NodeKind::Text(t) => assert_eq!(t.borrow().as_str(), "hello"),
        _ => panic!("expected text node"),
    }
    // Old children should be detached
    assert!(child1.parent().is_none());
    assert!(child2.parent().is_none());
}

#[test]
fn set_text_content_empty_removes_all() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let child = arena.create_element("p");
    parent.append_child(child);

    parent.set_text_content(&arena, "");

    assert!(parent.children().is_empty());
    assert!(child.parent().is_none());
}

// ─── set_inner_html tests ───────────────────────────────────────────────────

#[test]
fn set_inner_html_replaces_children() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let old_child = arena.create_element("p");
    parent.append_child(old_child);

    parent.set_inner_html(&arena, "<strong>bold</strong> text");

    // Old child should be detached
    assert!(old_child.parent().is_none());

    // New children should be parsed
    let children = parent.children();
    assert!(!children.is_empty());

    // Collect text content
    let text = parent.text_content();
    assert_eq!(text, "bold text");
}

#[test]
fn set_inner_html_empty_clears() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");
    let child = arena.create_element("span");
    parent.append_child(child);

    parent.set_inner_html(&arena, "");

    assert!(parent.children().is_empty());
    assert!(child.parent().is_none());
}

#[test]
fn set_inner_html_complex_fragment() {
    let arena = DomArena::new();
    let parent = arena.create_element("div");

    parent.set_inner_html(&arena, "<ul><li>one</li><li>two</li></ul>");

    let children = parent.children();
    // Should have a <ul> element
    assert!(children
        .iter()
        .any(|c| c.as_element().map(|e| e.local_name() == "ul").unwrap_or(false)));

    let text = parent.text_content();
    assert!(text.contains("one"));
    assert!(text.contains("two"));
}

// ─── createElement + appendChild workflow ───────────────────────────────────

#[test]
fn create_and_build_tree() {
    let arena = DomArena::new();
    let div = arena.create_element("div");
    let p = arena.create_element("p");
    let text = arena.create_text_node("Hello, Fortrust!");

    p.append_child(text);
    div.append_child(p);

    assert_eq!(div.children().len(), 1);
    assert_eq!(div.text_content(), "Hello, Fortrust!");
}

#[test]
fn create_element_with_attributes() {
    let arena = DomArena::new();
    let a = arena.create_element("a");
    let el = a.as_element().unwrap();
    el.set_attr("href", "https://example.com");
    el.set_attr("class", "link");

    assert_eq!(el.attr("href").as_deref(), Some("https://example.com"));
    assert_eq!(el.attr("class").as_deref(), Some("link"));

    // Update attribute
    el.set_attr("class", "link active");
    assert_eq!(el.attr("class").as_deref(), Some("link active"));

    // Remove attribute
    el.remove_attr("class");
    assert_eq!(el.attr("class"), None);
}

// ─── Circular reference prevention ─────────────────────────────────────────

#[test]
fn circular_reference_deep_hierarchy() {
    let arena = DomArena::new();
    let a = arena.create_element("div");
    let b = arena.create_element("section");
    let c = arena.create_element("article");
    let d = arena.create_element("span");

    a.append_child(b);
    b.append_child(c);
    c.append_child(d);

    // Trying to make 'a' a child of 'd' should fail (a is ancestor of d)
    let result = d.append_child_checked(a);
    assert_eq!(result, Err(DomError::HierarchyRequest));

    // Trying to make 'b' a child of 'd' should fail (b is ancestor of d)
    let result = d.append_child_checked(b);
    assert_eq!(result, Err(DomError::HierarchyRequest));

    // But appending an unrelated node should succeed
    let unrelated = arena.create_element("footer");
    let result = d.append_child_checked(unrelated);
    assert_eq!(result, Ok(()));
}

// ─── MutationLog tests ──────────────────────────────────────────────────────

#[test]
fn mutation_log_records_changes() {
    let log = MutationLog::new();
    assert!(log.is_empty());

    log.push(MutationRecord {
        mutation_type: MutationType::ChildList,
        target_description: "div".to_string(),
        added_nodes: vec!["span".to_string()],
        removed_nodes: vec![],
        attribute_name: None,
        old_value: None,
    });

    assert_eq!(log.len(), 1);
    assert!(!log.is_empty());

    let records = log.records();
    assert_eq!(records[0].mutation_type, MutationType::ChildList);
    assert_eq!(records[0].target_description, "div");
    assert_eq!(records[0].added_nodes, vec!["span".to_string()]);
}

#[test]
fn mutation_log_take_records_clears() {
    let log = MutationLog::new();

    log.push(MutationRecord {
        mutation_type: MutationType::Attributes,
        target_description: "img".to_string(),
        added_nodes: vec![],
        removed_nodes: vec![],
        attribute_name: Some("src".to_string()),
        old_value: Some("/old.png".to_string()),
    });

    log.push(MutationRecord {
        mutation_type: MutationType::CharacterData,
        target_description: "#text".to_string(),
        added_nodes: vec![],
        removed_nodes: vec![],
        attribute_name: None,
        old_value: Some("old text".to_string()),
    });

    assert_eq!(log.len(), 2);

    let taken = log.take_records();
    assert_eq!(taken.len(), 2);
    assert!(log.is_empty());
}

// ─── Integration: parse then mutate ─────────────────────────────────────────

#[test]
fn parse_then_mutate_dom() {
    let arena = DomArena::new();
    let doc = parse_html(&arena, "<div><p>original</p></div>").unwrap();

    let div = doc
        .descendants()
        .into_iter()
        .find(|n| {
            n.as_element()
                .map(|e| e.local_name() == "div")
                .unwrap_or(false)
        })
        .unwrap();

    // Add a new element
    let new_span = arena.create_element("span");
    let new_text = arena.create_text_node("added");
    new_span.append_child(new_text);
    div.append_child(new_span);

    // Verify tree now contains both original and new content
    let text = div.text_content();
    assert!(text.contains("original"));
    assert!(text.contains("added"));
    assert_eq!(div.children().len(), 2);
}

#[test]
fn node_describe_method() {
    let arena = DomArena::new();

    let el = arena.create_element("div");
    assert_eq!(el.describe(), "div");

    let text = arena.create_text_node("hi");
    assert_eq!(text.describe(), "#text");

    let comment = arena.create_comment("note");
    assert_eq!(comment.describe(), "#comment");
}
