pub mod event;
pub mod form;

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use bumpalo::Bump;
use compact_str::CompactString;
use event::EventListenerSet;
use html5ever::interface::{Attribute, QualName};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
#[allow(unused_imports)]
use html5ever::{ParseOpts, namespace_url, ns, parse_document, parse_fragment, LocalName};
use smallvec::SmallVec;

pub const MAX_HTML_BYTES: usize = 8 * 1024 * 1024;

// ─── Mutation Records ───────────────────────────────────────────────────────

/// Represents the type of DOM mutation that occurred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationType {
    ChildList,
    Attributes,
    CharacterData,
}

/// A record of a single DOM mutation, modeled after the W3C MutationRecord.
#[derive(Debug, Clone)]
pub struct MutationRecord {
    pub mutation_type: MutationType,
    /// Description of the target node (tag name or node type).
    pub target_description: String,
    /// Descriptions of added nodes.
    pub added_nodes: Vec<String>,
    /// Descriptions of removed nodes.
    pub removed_nodes: Vec<String>,
    /// Name of changed attribute, if applicable.
    pub attribute_name: Option<String>,
    /// Old value of changed attribute or character data.
    pub old_value: Option<String>,
}

/// A log that accumulates MutationRecords. Attach to a document to track changes.
#[derive(Debug, Default)]
pub struct MutationLog {
    records: RefCell<Vec<MutationRecord>>,
}

impl MutationLog {
    pub fn new() -> Self {
        Self {
            records: RefCell::new(Vec::new()),
        }
    }

    pub fn push(&self, record: MutationRecord) {
        self.records.borrow_mut().push(record);
    }

    pub fn take_records(&self) -> Vec<MutationRecord> {
        self.records.replace(Vec::new())
    }

    pub fn records(&self) -> Vec<MutationRecord> {
        self.records.borrow().clone()
    }

    pub fn len(&self) -> usize {
        self.records.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.borrow().is_empty()
    }
}

pub type NodeRef<'arena> = &'arena Node<'arena>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomError {
    InputTooLarge {
        limit_bytes: usize,
        actual_bytes: usize,
    },
    /// Attempted to append a node to itself or one of its own descendants.
    HierarchyRequest,
    /// The child node was not found in the specified parent.
    NotFound,
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
            event_listeners: EventListenerSet::new(),
            dirty: Cell::new(false),
        })
    }

    /// Create a new element node with the given tag name (HTML namespace).
    pub fn create_element<'arena>(&'arena self, tag_name: &str) -> NodeRef<'arena> {
        self.alloc(NodeKind::Element(ElementData {
            name: QualName::new(None, ns!(html), LocalName::from(tag_name)),
            attrs: RefCell::new(SmallVec::new()),
            template_contents: None,
            mathml_annotation_xml_integration_point: false,
        }))
    }

    /// Create a new text node with the given content.
    pub fn create_text_node<'arena>(&'arena self, text: &str) -> NodeRef<'arena> {
        self.alloc(NodeKind::Text(RefCell::new(CompactString::from(text))))
    }

    /// Create a new comment node with the given content.
    pub fn create_comment<'arena>(&'arena self, text: &str) -> NodeRef<'arena> {
        self.alloc(NodeKind::Comment(CompactString::from(text)))
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

    /// Find all descendant elements matching a simple CSS selector.
    /// Supports: tag name, `.class`, `#id`, and `tag.class` combinations.
    pub fn query_selector_all(&self, selector: &str) -> Vec<NodeRef<'arena>> {
        self.descendants()
            .into_iter()
            .filter(|node| matches_simple_selector(node, selector))
            .collect()
    }

    /// Find the first descendant element matching a simple CSS selector.
    pub fn query_selector(&self, selector: &str) -> Option<NodeRef<'arena>> {
        self.descendants()
            .into_iter()
            .find(|node| matches_simple_selector(node, selector))
    }

    /// Find an element by its `id` attribute.
    pub fn get_element_by_id(&self, id: &str) -> Option<NodeRef<'arena>> {
        self.descendants().into_iter().find(|node| {
            node.as_element()
                .and_then(|el| el.attr("id"))
                .is_some_and(|attr| attr == id)
        })
    }

    /// Find all elements with the given tag name.
    pub fn get_elements_by_tag_name(&self, tag: &str) -> Vec<NodeRef<'arena>> {
        self.descendants()
            .into_iter()
            .filter(|node| {
                node.as_element()
                    .is_some_and(|el| el.local_name().eq_ignore_ascii_case(tag))
            })
            .collect()
    }

    /// Find all elements with the given class name.
    pub fn get_elements_by_class_name(&self, class: &str) -> Vec<NodeRef<'arena>> {
        self.descendants()
            .into_iter()
            .filter(|node| {
                node.as_element()
                    .and_then(|el| el.attr("class"))
                    .is_some_and(|c| c.split_whitespace().any(|part| part == class))
            })
            .collect()
    }

    /// Check if any node in the document tree is marked dirty.
    pub fn is_dirty(&self) -> bool {
        self.descendants().iter().any(|node| node.dirty.get())
    }

    /// Clear the dirty flag on all nodes in the tree.
    pub fn clear_dirty(&self) {
        self.root.dirty.set(false);
        for node in self.descendants() {
            node.dirty.set(false);
        }
    }

    /// Collect the bounding rectangles of all dirty subtrees.
    /// These represent the "damage regions" that need repainting.
    /// Note: This requires a prior layout to have been computed.
    pub fn dirty_subtree_roots(&self) -> Vec<NodeRef<'arena>> {
        let mut roots = Vec::new();
        self.collect_dirty_roots(self.root, &mut roots);
        roots
    }

    fn collect_dirty_roots(&self, node: NodeRef<'arena>, out: &mut Vec<NodeRef<'arena>>) {
        if node.dirty.get() {
            // Check if any ancestor is also dirty — if so, this node
            // is already covered by the ancestor's dirty subtree.
            let mut ancestor = node.parent.get();
            while let Some(a) = ancestor {
                if a.dirty.get() {
                    return; // Covered by ancestor
                }
                ancestor = a.parent.get();
            }
            out.push(node);
            return;
        }
        for child in node.children.borrow().iter().copied() {
            self.collect_dirty_roots(child, out);
        }
    }
}

#[derive(Debug)]
pub struct Node<'arena> {
    parent: Cell<Option<NodeRef<'arena>>>,
    children: RefCell<SmallVec<[NodeRef<'arena>; 8]>>,
    kind: NodeKind<'arena>,
    /// Event listeners registered on this node via addEventListener.
    pub event_listeners: EventListenerSet,
    /// Dirty flag — set when this node or its subtree has been mutated and needs re-render.
    pub dirty: Cell<bool>,
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

    /// Returns a short description of this node (for MutationRecord reporting).
    pub fn describe(&self) -> String {
        match &self.kind {
            NodeKind::Document => "#document".to_string(),
            NodeKind::Doctype { name, .. } => format!("<!DOCTYPE {name}>"),
            NodeKind::Element(el) => el.local_name().to_string(),
            NodeKind::Text(_) => "#text".to_string(),
            NodeKind::Comment(_) => "#comment".to_string(),
            NodeKind::ProcessingInstruction { target, .. } => format!("?{target}"),
            NodeKind::Phantom(_) => "#phantom".to_string(),
        }
    }

    /// Check if `candidate` is an ancestor of this node (inclusive).
    fn is_ancestor_of(&'arena self, candidate: NodeRef<'arena>) -> bool {
        if std::ptr::eq(self, candidate) {
            return true;
        }
        for child in self.children.borrow().iter() {
            if child.is_ancestor_of(candidate) {
                return true;
            }
        }
        false
    }

    /// Append a child node. If the child already has a parent, it is detached first.
    /// Returns Err(HierarchyRequest) if appending would create a cycle.
    pub fn append_child_checked(
        &'arena self,
        child: NodeRef<'arena>,
    ) -> Result<(), DomError> {
        // Prevent circular references: child must not be an ancestor of self
        if child.is_ancestor_of(self) {
            return Err(DomError::HierarchyRequest);
        }
        self.append_child(child);
        Ok(())
    }

    /// Append child (unchecked, existing API preserved for backward compat).
    pub fn append_child(&'arena self, child: NodeRef<'arena>) {
        // detach child from its current parent
        if let Some(parent) = child.parent() {
            parent
                .children
                .borrow_mut()
                .retain(|c| !std::ptr::eq(*c, child));
        }
        child.parent.set(Some(self));
        self.children.borrow_mut().push(child);
    }

    /// Remove a child from this node. Returns Err(NotFound) if child is not a child of self.
    pub fn remove_child_checked(
        &'arena self,
        child: NodeRef<'arena>,
    ) -> Result<(), DomError> {
        let mut children = self.children.borrow_mut();
        let pos = children.iter().position(|c| std::ptr::eq(*c, child));
        match pos {
            Some(i) => {
                children.remove(i);
                child.parent.set(None);
                Ok(())
            }
            None => Err(DomError::NotFound),
        }
    }

    /// Remove child (unchecked, existing API preserved).
    pub fn remove_child(&'arena self, child: NodeRef<'arena>) {
        self.children
            .borrow_mut()
            .retain(|c| !std::ptr::eq(*c, child));
        child.parent.set(None);
    }

    /// Insert `new_child` before `reference` in this node's children.
    /// If `reference` is None, appends to the end (same as append_child).
    /// Returns Err(HierarchyRequest) if it would create a cycle,
    /// or Err(NotFound) if reference is not a child of self.
    pub fn insert_before(
        &'arena self,
        new_child: NodeRef<'arena>,
        reference: Option<NodeRef<'arena>>,
    ) -> Result<(), DomError> {
        // Prevent circular references
        if new_child.is_ancestor_of(self) {
            return Err(DomError::HierarchyRequest);
        }

        let Some(ref_node) = reference else {
            // No reference → append
            self.append_child(new_child);
            return Ok(());
        };

        // Verify reference is a child of self
        {
            let children = self.children.borrow();
            if !children.iter().any(|c| std::ptr::eq(*c, ref_node)) {
                return Err(DomError::NotFound);
            }
        }

        // Detach new_child from current parent
        if let Some(parent) = new_child.parent() {
            parent
                .children
                .borrow_mut()
                .retain(|c| !std::ptr::eq(*c, new_child));
        }
        new_child.parent.set(Some(self));

        // Re-find index (may have shifted if new_child was already a child of self)
        let mut children = self.children.borrow_mut();
        let insert_idx = children
            .iter()
            .position(|c| std::ptr::eq(*c, ref_node))
            .unwrap_or(children.len());
        children.insert(insert_idx, new_child);
        Ok(())
    }

    /// Replace `old_child` with `new_child`. Returns Err(NotFound) if old_child is not here,
    /// or Err(HierarchyRequest) if it would create a cycle.
    pub fn replace_child(
        &'arena self,
        new_child: NodeRef<'arena>,
        old_child: NodeRef<'arena>,
    ) -> Result<NodeRef<'arena>, DomError> {
        if new_child.is_ancestor_of(self) {
            return Err(DomError::HierarchyRequest);
        }

        // Verify old_child is a child of self
        {
            let children = self.children.borrow();
            if !children.iter().any(|c| std::ptr::eq(*c, old_child)) {
                return Err(DomError::NotFound);
            }
        }

        // Detach new_child from its current parent
        if let Some(parent) = new_child.parent() {
            parent
                .children
                .borrow_mut()
                .retain(|c| !std::ptr::eq(*c, new_child));
        }

        // Replace at the position (re-find since removal above may shift)
        let mut children = self.children.borrow_mut();
        let replace_idx = children
            .iter()
            .position(|c| std::ptr::eq(*c, old_child))
            .ok_or(DomError::NotFound)?;
        children[replace_idx] = new_child;
        new_child.parent.set(Some(self));
        old_child.parent.set(None);
        Ok(old_child)
    }

    /// Remove all children and set the text content of this node.
    pub fn set_text_content(&'arena self, arena: &'arena DomArena, text: &str) {
        // Detach all existing children
        let old_children = self.children.replace(SmallVec::new());
        for child in &old_children {
            child.parent.set(None);
        }
        // If text is non-empty, create a text node child
        if !text.is_empty() {
            let text_node = arena.create_text_node(text);
            text_node.parent.set(Some(self));
            self.children.borrow_mut().push(text_node);
        }
        self.mark_dirty();
    }

    /// Mark this node and all its ancestors as dirty (needing re-render).
    pub fn mark_dirty(&'arena self) {
        self.dirty.set(true);
        let mut current = self.parent.get();
        while let Some(node) = current {
            if node.dirty.get() {
                break; // Already marked
            }
            node.dirty.set(true);
            current = node.parent.get();
        }
    }

    /// Check and clear the dirty flag.
    pub fn take_dirty(&self) -> bool {
        let was_dirty = self.dirty.get();
        self.dirty.set(false);
        was_dirty
    }

    /// Count depth in the tree (number of ancestors).
    pub fn depth(&'arena self) -> usize {
        let mut depth = 0usize;
        let mut current = self.parent.get();
        while let Some(node) = current {
            depth += 1;
            current = node.parent.get();
        }
        depth
    }

    /// Parse an HTML fragment and replace this node's children with the parsed nodes.
    pub fn set_inner_html(&'arena self, arena: &'arena DomArena, html: &str) {
        // Detach all existing children
        let old_children = self.children.replace(SmallVec::new());
        for child in &old_children {
            child.parent.set(None);
        }

        if html.is_empty() {
            self.mark_dirty();
            return;
        }

        // Determine context element for fragment parsing
        let context_name = match &self.kind {
            NodeKind::Element(el) => el.name.clone(),
            _ => QualName::new(None, ns!(html), LocalName::from("body")),
        };

        // Parse the fragment
        let sink = DomBuilder::new(arena);
        let dom = parse_fragment(sink, ParseOpts::default(), context_name, Vec::new(), false)
            .one(html);

        // Move the parsed children from the fragment root's <html> into self
        // Fragment parsing creates a document with the parsed nodes inside
        let parsed_children = collect_fragment_children(dom.root);
        for child in parsed_children {
            child.parent.set(Some(self));
            self.children.borrow_mut().push(child);
        }
        self.mark_dirty();
    }

    /// Append child and mark dirty.
    pub fn append_child_dirty(&'arena self, child: NodeRef<'arena>) {
        self.append_child(child);
        self.mark_dirty();
    }

    /// Remove child and mark dirty.
    pub fn remove_child_dirty(&'arena self, child: NodeRef<'arena>) {
        self.remove_child(child);
        self.mark_dirty();
    }

    /// Find the first descendant element matching a simple CSS selector.
    pub fn query_selector(&self, selector: &str) -> Option<NodeRef<'arena>> {
        collect_descendants_list(self).into_iter().find(|node| matches_simple_selector(node, selector))
    }

    /// Find all descendant elements matching a simple CSS selector.
    pub fn query_selector_all(&self, selector: &str) -> Vec<NodeRef<'arena>> {
        collect_descendants_list(self).into_iter().filter(|node| matches_simple_selector(node, selector)).collect()
    }

    /// Find all elements with the given tag name among descendants.
    pub fn get_elements_by_tag_name(&self, tag: &str) -> Vec<NodeRef<'arena>> {
        collect_descendants_list(self).into_iter().filter(|node| {
            node.as_element().is_some_and(|el| el.local_name().eq_ignore_ascii_case(tag))
        }).collect()
    }

    /// Find all elements with the given class name among descendants.
    pub fn get_elements_by_class_name(&self, class: &str) -> Vec<NodeRef<'arena>> {
        collect_descendants_list(self).into_iter().filter(|node| {
            node.as_element().and_then(|el| el.attr("class")).is_some_and(|c| c.split_whitespace().any(|part| part == class))
        }).collect()
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
    
    pub fn set_attr(&self, name: &str, value: &str) {
        let mut attrs = self.attrs.borrow_mut();
        let name_cs = CompactString::from(name);
        for (key, val) in attrs.iter_mut() {
            if key.eq_ignore_ascii_case(name) {
                *val = CompactString::from(value);
                return;
            }
        }
        attrs.push((name_cs, CompactString::from(value)));
    }

    pub fn remove_attr(&self, name: &str) {
        let mut attrs = self.attrs.borrow_mut();
        attrs.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
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

/// Build the ancestor chain from a node up to the root (inclusive).
/// Returns nodes ordered from the given node up to root.
pub fn ancestor_chain<'arena>(node: NodeRef<'arena>) -> Vec<NodeRef<'arena>> {
    let mut chain = Vec::new();
    let mut current: Option<NodeRef<'arena>> = Some(node);
    while let Some(n) = current {
        chain.push(n);
        current = n.parent.get();
    }
    chain
}

fn collect_descendants<'arena>(node: NodeRef<'arena>, out: &mut Vec<NodeRef<'arena>>) {
    for child in node.children.borrow().iter().copied() {
        out.push(child);
        collect_descendants(child, out);
    }
}

/// Collect all descendants of a node (not including the node itself).
fn collect_descendants_list<'arena>(node: &Node<'arena>) -> Vec<NodeRef<'arena>> {
    let mut out = Vec::new();
    for child in node.children.borrow().iter().copied() {
        out.push(child);
        collect_descendants(child, &mut out);
    }
    out
}

/// Match a node against a simple CSS selector string.
/// Supports: tag, `.class`, `#id`, `tag.class`, `tag#id`, `.class1.class2`.
fn matches_simple_selector(node: NodeRef<'_>, selector: &str) -> bool {
    let selector = selector.trim();
    if selector.is_empty() || selector == "*" {
        return true;
    }

    let Some(element) = node.as_element() else {
        return false;
    };

    let mut remaining = selector;
    let mut tag_required: Option<&str> = None;

    // Parse tag name at start
    if !remaining.starts_with('.') && !remaining.starts_with('#') {
        let end = remaining
            .find(['.', '#'])
            .unwrap_or(remaining.len());
        if end > 0 {
            tag_required = Some(&remaining[..end]);
            remaining = &remaining[end..];
        }
    }

    // Check tag
    if let Some(tag) = tag_required
        && !element.local_name().eq_ignore_ascii_case(tag) {
            return false;
        }

    // Parse remaining .class and #id parts
    let mut required_classes: Vec<&str> = Vec::new();
    let mut required_id: Option<&str> = None;

    while !remaining.is_empty() {
        if let Some(rest) = remaining.strip_prefix('.') {
            let end = rest
                .find(['.', '#'])
                .unwrap_or(rest.len());
            if end > 0 {
                required_classes.push(&rest[..end]);
            }
            remaining = &rest[end..];
        } else if let Some(rest) = remaining.strip_prefix('#') {
            let end = rest
                .find(['.', '#'])
                .unwrap_or(rest.len());
            if end > 0 {
                required_id = Some(&rest[..end]);
            }
            remaining = &rest[end..];
        } else {
            break;
        }
    }

    // Check ID
    if let Some(id) = required_id
        && element.attr("id").as_deref() != Some(id) {
            return false;
        }

    // Check classes
    if !required_classes.is_empty() {
        let class_attr = element.attr("class").unwrap_or_default();
        let node_classes: Vec<&str> = class_attr.split_whitespace().collect();
        for required in &required_classes {
            if !node_classes.contains(required) {
                return false;
            }
        }
    }

    true
}

fn collect_text(node: NodeRef<'_>, out: &mut String) {
    if let NodeKind::Text(text) = &node.kind {
        out.push_str(&text.borrow());
    }

    for child in node.children.borrow().iter().copied() {
        collect_text(child, out);
    }
}

/// After fragment parsing, collect the actual content nodes.
/// Fragment parsing produces a document node whose children contain the parsed content.
/// We recursively collect all leaf content nodes from the tree.
fn collect_fragment_children<'arena>(root: NodeRef<'arena>) -> Vec<NodeRef<'arena>> {
    // Fragment parsing wraps content in <html><body>...</body></html>
    // We need to find the deepest container that holds the user's content.
    // Strategy: walk down html > body and return its children, or just
    // return the root's direct children if the structure differs.
    let children = root.children.borrow().clone();

    // Look for <html> element
    for child in &children {
        if let NodeKind::Element(el) = &child.kind
            && el.local_name().eq_ignore_ascii_case("html") {
                // Look for <body> inside <html>
                let html_children = child.children.borrow().clone();
                for html_child in &html_children {
                    if let NodeKind::Element(body_el) = &html_child.kind
                        && body_el.local_name().eq_ignore_ascii_case("body") {
                            let body_children = html_child.children.borrow().clone();
                            // Detach from body parent
                            for bc in &body_children {
                                bc.parent.set(None);
                            }
                            return body_children.to_vec();
                        }
                }
                // No body found, return html's children
                let html_children_vec = html_children.to_vec();
                for hc in &html_children_vec {
                    hc.parent.set(None);
                }
                return html_children_vec;
            }
    }

    // No html wrapper, return direct children
    let result = children.to_vec();
    for c in &result {
        c.parent.set(None);
    }
    result
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
