use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use bumpalo::Bump;
use compact_str::CompactString;
use html5ever::interface::{Attribute, QualName};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{ParseOpts, parse_document};
use smallvec::SmallVec;

pub const MAX_HTML_BYTES: usize = 8 * 1024 * 1024;

pub type NodeRef<'arena> = &'arena Node<'arena>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomError {
    InputTooLarge {
        limit_bytes: usize,
        actual_bytes: usize,
    },
}

#[derive(Debug)]
pub struct DomArena {
    bump: Bump,
}

impl DomArena {
    pub fn new() -> Self {
        Self { bump: Bump::new() }
    }

    fn alloc<'arena>(&'arena self, kind: NodeKind<'arena>) -> NodeRef<'arena> {
        self.bump.alloc(Node {
            parent: Cell::new(None),
            children: RefCell::new(SmallVec::new()),
            kind,
        })
    }
}

impl Default for DomArena {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct Document<'arena> {
    pub root: NodeRef<'arena>,
    pub quirks_mode: QuirksMode,
    pub parse_errors: Vec<String>,
}

impl<'arena> Document<'arena> {
    pub fn descendants(&self) -> Vec<NodeRef<'arena>> {
        let mut out = Vec::new();
        collect_descendants(self.root, &mut out);
        out
    }

    pub fn text_content(&self) -> String {
        let mut out = String::new();
        collect_text(self.root, &mut out);
        out
    }

    pub fn first_element_by_tag(&self, tag: &str) -> Option<NodeRef<'arena>> {
        self.descendants().into_iter().find(|node| {
            node.as_element()
                .is_some_and(|element| element.local_name().eq_ignore_ascii_case(tag))
        })
    }
}

#[derive(Debug)]
pub struct Node<'arena> {
    parent: Cell<Option<NodeRef<'arena>>>,
    children: RefCell<SmallVec<[NodeRef<'arena>; 8]>>,
    kind: NodeKind<'arena>,
}

impl<'arena> Node<'arena> {
    pub fn kind(&self) -> &NodeKind<'arena> {
        &self.kind
    }

    pub fn parent(&self) -> Option<NodeRef<'arena>> {
        self.parent.get()
    }

    pub fn children(&self) -> SmallVec<[NodeRef<'arena>; 8]> {
        self.children.borrow().clone()
    }

    pub fn text_content(&'arena self) -> String {
        let mut out = String::new();
        collect_text(self, &mut out);
        out
    }

    pub fn as_element(&self) -> Option<&ElementData<'arena>> {
        match &self.kind {
            NodeKind::Element(element) => Some(element),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum NodeKind<'arena> {
    Document,
    Doctype {
        name: CompactString,
        public_id: CompactString,
        system_id: CompactString,
    },
    Element(ElementData<'arena>),
    Text(RefCell<CompactString>),
    Comment(CompactString),
    ProcessingInstruction {
        target: CompactString,
        data: CompactString,
    },
    Phantom(std::marker::PhantomData<&'arena ()>),
}

#[derive(Debug)]
pub struct ElementData<'arena> {
    name: QualName,
    attrs: RefCell<SmallVec<[(CompactString, CompactString); 4]>>,
    template_contents: Option<NodeRef<'arena>>,
    mathml_annotation_xml_integration_point: bool,
}

impl ElementData<'_> {
    pub fn local_name(&self) -> &str {
        self.name.local.as_ref()
    }

    pub fn namespace(&self) -> &str {
        self.name.ns.as_ref()
    }

    pub fn attr(&self, name: &str) -> Option<CompactString> {
        self.attrs
            .borrow()
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    }

    pub fn attrs(&self) -> SmallVec<[(CompactString, CompactString); 4]> {
        self.attrs.borrow().clone()
    }
}

pub fn parse_html<'arena>(
    arena: &'arena DomArena,
    html: &str,
) -> Result<Document<'arena>, DomError> {
    if html.len() > MAX_HTML_BYTES {
        return Err(DomError::InputTooLarge {
            limit_bytes: MAX_HTML_BYTES,
            actual_bytes: html.len(),
        });
    }

    let sink = DomBuilder::new(arena);
    Ok(parse_document(sink, ParseOpts::default()).one(html))
}

#[derive(Debug)]
struct DomBuilder<'arena> {
    arena: &'arena DomArena,
    document: NodeRef<'arena>,
    errors: RefCell<Vec<String>>,
    quirks_mode: Cell<QuirksMode>,
}

impl<'arena> DomBuilder<'arena> {
    fn new(arena: &'arena DomArena) -> Self {
        Self {
            arena,
            document: arena.alloc(NodeKind::Document),
            errors: RefCell::new(Vec::new()),
            quirks_mode: Cell::new(QuirksMode::NoQuirks),
        }
    }

    fn attach(&self, parent: NodeRef<'arena>, child: NodeRef<'arena>) {
        child.parent.set(Some(parent));
        parent.children.borrow_mut().push(child);
    }

    fn append_text(&self, parent: NodeRef<'arena>, text: StrTendril) {
        if text.is_empty() {
            return;
        }

        let node = self
            .arena
            .alloc(NodeKind::Text(RefCell::new(CompactString::from(
                text.as_ref(),
            ))));
        self.attach(parent, node);
    }

    fn append_child(&self, parent: NodeRef<'arena>, child: NodeOrText<NodeRef<'arena>>) {
        match child {
            NodeOrText::AppendText(text) => {
                if let Some(last) = parent.children.borrow().last()
                    && let NodeKind::Text(existing) = &last.kind
                {
                    existing.borrow_mut().push_str(text.as_ref());
                    return;
                }
                self.append_text(parent, text);
            }
            NodeOrText::AppendNode(node) => {
                self.detach(node);
                self.attach(parent, node);
            }
        }
    }

    fn detach(&self, node: NodeRef<'arena>) {
        let Some(parent) = node.parent() else {
            return;
        };

        parent
            .children
            .borrow_mut()
            .retain(|child| !std::ptr::eq(*child, node));
        node.parent.set(None);
    }
}

impl<'arena> TreeSink for DomBuilder<'arena> {
    type Handle = NodeRef<'arena>;
    type Output = Document<'arena>;
    type ElemName<'a>
        = &'a QualName
    where
        Self: 'a;

    fn finish(self) -> Self::Output {
        Document {
            root: self.document,
            quirks_mode: self.quirks_mode.get(),
            parse_errors: self.errors.into_inner(),
        }
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        self.errors.borrow_mut().push(msg.into_owned());
    }

    fn get_document(&self) -> Self::Handle {
        self.document
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        let Some(element) = target.as_element() else {
            panic!("html5ever requested an element name for a non-element node");
        };
        &element.name
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<Attribute>,
        flags: ElementFlags,
    ) -> Self::Handle {
        let template_contents = if flags.template {
            Some(self.arena.alloc(NodeKind::Document))
        } else {
            None
        };

        self.arena.alloc(NodeKind::Element(ElementData {
            name,
            attrs: RefCell::new(attrs_to_smallvec(attrs)),
            template_contents,
            mathml_annotation_xml_integration_point: flags.mathml_annotation_xml_integration_point,
        }))
    }

    fn create_comment(&self, text: StrTendril) -> Self::Handle {
        self.arena
            .alloc(NodeKind::Comment(CompactString::from(text.as_ref())))
    }

    fn create_pi(&self, target: StrTendril, data: StrTendril) -> Self::Handle {
        self.arena.alloc(NodeKind::ProcessingInstruction {
            target: CompactString::from(target.as_ref()),
            data: CompactString::from(data.as_ref()),
        })
    }

    fn append(&self, parent: &Self::Handle, child: NodeOrText<Self::Handle>) {
        self.append_child(parent, child);
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        prev_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        if element.parent().is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let node = self.arena.alloc(NodeKind::Doctype {
            name: CompactString::from(name.as_ref()),
            public_id: CompactString::from(public_id.as_ref()),
            system_id: CompactString::from(system_id.as_ref()),
        });
        self.attach(self.document, node);
    }

    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<Attribute>) {
        let Some(element) = target.as_element() else {
            return;
        };

        let mut existing = element.attrs.borrow_mut();
        for attr in attrs {
            let name = CompactString::from(attr.name.local.as_ref());
            if existing
                .iter()
                .all(|(key, _)| !key.eq_ignore_ascii_case(name.as_str()))
            {
                existing.push((name, CompactString::from(attr.value.as_ref())));
            }
        }
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        self.detach(target);
    }

    fn reparent_children(&self, node: &Self::Handle, new_parent: &Self::Handle) {
        let children = node.children.replace(SmallVec::new());
        for child in children {
            child.parent.set(Some(*new_parent));
            new_parent.children.borrow_mut().push(child);
        }
    }

    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        let Some(element) = target.as_element() else {
            panic!("html5ever requested template contents for a non-element node");
        };

        element
            .template_contents
            .expect("html5ever requested template contents for a non-template element")
    }

    fn append_before_sibling(&self, sibling: &Self::Handle, child: NodeOrText<Self::Handle>) {
        let Some(parent) = sibling.parent() else {
            return;
        };

        let mut children = parent.children.borrow_mut();
        let Some(index) = children
            .iter()
            .position(|candidate| std::ptr::eq(*candidate, *sibling))
        else {
            return;
        };

        match child {
            NodeOrText::AppendText(text) => {
                if index > 0
                    && let NodeKind::Text(existing) = &children[index - 1].kind
                {
                    existing.borrow_mut().push_str(text.as_ref());
                    return;
                }

                let node = self
                    .arena
                    .alloc(NodeKind::Text(RefCell::new(CompactString::from(
                        text.as_ref(),
                    ))));
                node.parent.set(Some(parent));
                children.insert(index, node);
            }
            NodeOrText::AppendNode(node) => {
                drop(children);
                self.detach(node);
                let mut children = parent.children.borrow_mut();
                let index = children
                    .iter()
                    .position(|candidate| std::ptr::eq(*candidate, *sibling))
                    .unwrap_or(children.len());
                node.parent.set(Some(parent));
                children.insert(index, node);
            }
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, target: &Self::Handle) -> bool {
        let Some(element) = target.as_element() else {
            panic!("html5ever requested MathML integration status for a non-element node");
        };
        element.mathml_annotation_xml_integration_point
    }

    fn mark_script_already_started(&self, _node: &Self::Handle) {}

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks_mode.set(mode);
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        std::ptr::eq(*x, *y)
    }
}

fn attrs_to_smallvec(attrs: Vec<Attribute>) -> SmallVec<[(CompactString, CompactString); 4]> {
    attrs
        .into_iter()
        .map(|attr| {
            (
                CompactString::from(attr.name.local.as_ref()),
                CompactString::from(attr.value.as_ref()),
            )
        })
        .collect()
}

fn collect_descendants<'arena>(node: NodeRef<'arena>, out: &mut Vec<NodeRef<'arena>>) {
    for child in node.children.borrow().iter().copied() {
        out.push(child);
        collect_descendants(child, out);
    }
}

fn collect_text(node: NodeRef<'_>, out: &mut String) {
    if let NodeKind::Text(text) = &node.kind {
        out.push_str(&text.borrow());
    }

    for child in node.children.borrow().iter().copied() {
        collect_text(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_malformed_html_with_spec_tree_builder() {
        let arena = DomArena::new();
        let document = parse_html(&arena, "<!doctype html><p>Hello <strong>Fortrust</p>").unwrap();

        assert!(document.first_element_by_tag("html").is_some());
        assert!(document.first_element_by_tag("body").is_some());
        assert_eq!(document.text_content(), "Hello Fortrust");
    }

    #[test]
    fn stores_element_attributes_without_heap_heavy_nodes() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<img src="/logo.png" alt="Fortrust">"#).unwrap();
        let img = document.first_element_by_tag("img").unwrap();
        let element = img.as_element().unwrap();

        assert_eq!(element.attr("src").as_deref(), Some("/logo.png"));
        assert_eq!(element.attr("ALT").as_deref(), Some("Fortrust"));
    }

    #[test]
    fn keeps_comments_out_of_text_content() {
        let arena = DomArena::new();
        let document = parse_html(&arena, "before<!-- private -->after").unwrap();

        assert_eq!(document.text_content(), "beforeafter");
        assert!(
            document
                .descendants()
                .iter()
                .any(|node| matches!(node.kind(), NodeKind::Comment(_)))
        );
    }

    #[test]
    fn rejects_unbounded_html_input() {
        let arena = DomArena::new();
        let html = "a".repeat(MAX_HTML_BYTES + 1);

        let Err(DomError::InputTooLarge {
            limit_bytes,
            actual_bytes,
        }) = parse_html(&arena, &html)
        else {
            panic!("oversized HTML should be rejected");
        };

        assert_eq!(limit_bytes, MAX_HTML_BYTES);
        assert_eq!(actual_bytes, MAX_HTML_BYTES + 1);
    }
}
