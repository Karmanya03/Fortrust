use fortrust_dom::{NodeKind, NodeRef};
use fortrust_style::{ComputedStyle, Display, Length, Overflow, StyleEngine};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }

    pub fn expand_by(&self, amount: f32) -> Rect {
        Rect {
            x: self.x - amount,
            y: self.y - amount,
            width: self.width + amount * 2.0,
            height: self.height + amount * 2.0,
        }
    }

    pub fn shrink(&self, amount: f32) -> Rect {
        Rect {
            x: self.x + amount,
            y: self.y + amount,
            width: (self.width - amount * 2.0).max(0.0),
            height: (self.height - amount * 2.0).max(0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutConstraints {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub containing_block: Option<Rect>,
}

impl LayoutConstraints {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            viewport_width: width,
            viewport_height: height,
            containing_block: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxKind {
    Block,
    Flex,
    Inline,
    Text,
    Replaced,
    Anonymous,
    Positioned,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsedEdges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl UsedEdges {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    fn horizontal(self) -> f32 {
        self.left + self.right
    }

    fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBox {
    pub kind: BoxKind,
    pub node_name: String,
    pub text: Option<String>,
    pub replaced_size: Option<(f32, f32)>,
    pub rect: Rect,
    pub margin: UsedEdges,
    pub padding: UsedEdges,
    pub border: UsedEdges,
    pub style: ComputedStyle,
    pub children: Vec<LayoutBox>,
    pub overflow_clip: Option<Rect>,
    pub z_index: i32,
    pub positioned_offset: Option<(f32, f32)>,
}

#[derive(Debug, Clone)]
pub struct LayoutTree {
    pub root: LayoutBox,
    pub positioned_children: Vec<LayoutBox>,
}

#[derive(Debug, Clone)]
pub struct LayoutEngine {
    style: StyleEngine,
    line_height_px: f32,
}

impl LayoutEngine {
    pub fn new(style: StyleEngine) -> Self {
        Self {
            style,
            line_height_px: 20.0,
        }
    }

    pub fn layout<'arena>(
        &self,
        root: NodeRef<'arena>,
        constraints: LayoutConstraints,
    ) -> Option<LayoutTree> {
        let style = ComputedStyle::initial();
        let mut root_box = self.build_box(root, None, &style)?;
        let width = constraints.viewport_width.max(0.0);
        let mut positioned = Vec::new();
        let _ = layout_block_box(
            &mut root_box,
            &mut positioned,
            0.0,
            0.0,
            width,
            None,
            self.line_height_px,
        );
        positioned.sort_by_key(|box_: &LayoutBox| box_.z_index);
        Some(LayoutTree {
            root: root_box,
            positioned_children: positioned,
        })
    }

    fn build_box<'arena>(
        &self,
        node: NodeRef<'arena>,
        parent_style: Option<&ComputedStyle>,
        fallback_parent: &ComputedStyle,
    ) -> Option<LayoutBox> {
        match node.kind() {
            NodeKind::Text(text) => {
                let text = text.borrow().trim().to_owned();
                if text.is_empty() {
                    return None;
                }
                let style = ComputedStyle::inherit_from(parent_style.or(Some(fallback_parent)));
                Some(LayoutBox {
                    kind: BoxKind::Text,
                    node_name: "#text".to_owned(),
                    text: Some(text),
                    replaced_size: None,
                    rect: zero_rect(),
                    margin: UsedEdges::ZERO,
                    padding: UsedEdges::ZERO,
                    border: UsedEdges::ZERO,
                    style,
                    children: Vec::new(),
                    overflow_clip: None,
                    z_index: 0,
                    positioned_offset: None,
                })
            }
            _ => {
                let style = self.style.compute_style(node, parent_style);
                if style.display == Display::None
                    || style.visibility == fortrust_style::Visibility::Hidden
                {
                    return None;
                }

                let element = node.as_element();
                let is_replaced =
                    element.is_some_and(|element| element.local_name().eq_ignore_ascii_case("img"));

                let is_positioned = style.is_absolutely_positioned();
                let kind = if is_positioned {
                    BoxKind::Positioned
                } else if is_replaced {
                    BoxKind::Replaced
                } else {
                    match style.display {
                        Display::Block => BoxKind::Block,
                        Display::Flex => BoxKind::Flex,
                        Display::Inline => BoxKind::Inline,
                        Display::InlineBlock => BoxKind::Inline,
                        Display::None => return None,
                        _ => BoxKind::Block,
                    }
                };
                let margin = used_edges(style.margin, 16.0);
                let padding = used_edges(style.padding, 16.0);
                let border = used_edges(style.border.clone().into(), 16.0);
                let node_name = element
                    .map(|element| element.local_name().to_owned())
                    .unwrap_or_else(|| "#document".to_owned());
                let replaced_size = if is_replaced {
                    parse_replaced_size(element.unwrap())
                } else {
                    None
                };
                let text_content = if is_replaced {
                    element
                        .and_then(|element| element.attr("alt"))
                        .map(|alt| alt.trim().to_owned())
                        .filter(|alt| !alt.is_empty())
                } else {
                    None
                };

                let children = if is_positioned {
                    Vec::new()
                } else {
                    node.children()
                        .into_iter()
                        .filter_map(|child| self.build_box(child, Some(&style), &style))
                        .collect()
                };

                let overflow_clip = if style.is_overflow_hidden() {
                    Some(Rect {
                        x: 0.0,
                        y: 0.0,
                        width: style.width.to_px(16.0, 1280.0, 16.0),
                        height: style.height.to_px(16.0, 1280.0, 16.0),
                    })
                } else {
                    None
                };

                Some(LayoutBox {
                    kind,
                    node_name,
                    text: text_content,
                    replaced_size,
                    rect: zero_rect(),
                    margin,
                    padding,
                    border,
                    style: style.clone(),
                    children,
                    overflow_clip,
                    z_index: style.z_index,
                    positioned_offset: None,
                })
            }
        }
    }
}

fn layout_block_box(
    layout_box: &mut LayoutBox,
    positioned: &mut Vec<LayoutBox>,
    x: f32,
    y: f32,
    available_width: f32,
    containing_block: Option<&Rect>,
    line_height_px: f32,
) -> f32 {
    let pb_width = containing_block.map(|c| c.width).unwrap_or(available_width);
    let computed_width = used_width(layout_box.style.width, pb_width);
    let has_computed_width = computed_width.is_some();
    let box_width = computed_width
        .unwrap_or(available_width)
        .max(
            layout_box
                .style
                .min_width
                .to_px(line_height_px, pb_width, 16.0),
        )
        .min(match layout_box.style.max_width {
            Length::None => f32::MAX,
            l => l.to_px(line_height_px, pb_width, 16.0),
        });
    let margin_h = layout_box.margin.horizontal();
    let border_h = layout_box.border.horizontal();
    let padding_h = layout_box.padding.horizontal();
    let _total_width = box_width + margin_h + border_h + padding_h;

    let used_x = x + layout_box.margin.left + layout_box.border.left + layout_box.padding.left;

    if layout_box.kind == BoxKind::Positioned {
        let offset = containing_block
            .map(|cb| {
                let left = layout_box.style.left.to_px(line_height_px, cb.width, 16.0);
                let top = layout_box.style.top.to_px(line_height_px, cb.height, 16.0);
                (cb.x + left, cb.y + top)
            })
            .unwrap_or((used_x, y));
        layout_box.rect = Rect {
            x: offset.0,
            y: offset.1,
            width: box_width,
            height: 0.0,
        };
        layout_box.positioned_offset = Some(offset);
        positioned.push(layout_box.clone());
        return 0.0;
    }

    // Compute relative positioning offset (applied to rect after layout)
    let (rel_dx, rel_dy) = if layout_box.style.position == fortrust_style::Position::Relative {
        let cb_w = containing_block.map(|c| c.width).unwrap_or(available_width);
        let cb_h = containing_block.map(|c| c.height).unwrap_or(available_width);
        let left = layout_box.style.left.to_px(line_height_px, cb_w, 16.0);
        let right = layout_box.style.right.to_px(line_height_px, cb_w, 16.0);
        let top = layout_box.style.top.to_px(line_height_px, cb_h, 16.0);
        let bottom = layout_box.style.bottom.to_px(line_height_px, cb_h, 16.0);
        let dx = if left != 0.0 { left } else { -right };
        let dy = if top != 0.0 { top } else { -bottom };
        (dx, dy)
    } else {
        (0.0, 0.0)
    };

    let content_width =
        box_width - layout_box.padding.horizontal() - layout_box.border.horizontal();

    let child_y = y + layout_box.margin.top + layout_box.border.top + layout_box.padding.top;

    if layout_box.style.overflow_x != Overflow::Visible
        || layout_box.style.overflow_y != Overflow::Visible
    {
        let clip_w = if has_computed_width {
            box_width
        } else {
            available_width
        };
        layout_box.overflow_clip = Some(Rect {
            x: used_x,
            y: child_y,
            width: clip_w.max(0.0),
            height: layout_box
                .style
                .height
                .to_px(line_height_px, pb_width, 16.0)
                .max(0.0),
        });
    }

    if layout_box.children.is_empty() && layout_box.kind != BoxKind::Replaced {
        let height = layout_box
            .style
            .height
            .to_px(line_height_px, pb_width, 16.0)
            .max(line_height_px_if_leaf(layout_box, line_height_px));
        layout_box.rect = Rect {
            x: used_x + rel_dx,
            y: y + rel_dy,
            width: box_width,
            height,
        };
        return height
            + layout_box.margin.top
            + layout_box.margin.bottom
            + layout_box.border.vertical()
            + layout_box.padding.vertical();
    }

    if layout_box.kind == BoxKind::Flex {
        let h = layout_flex_children(
            layout_box,
            positioned,
            child_x(layout_box, used_x),
            child_y,
            content_width,
            containing_block,
            line_height_px,
        );
        layout_box.rect = Rect {
            x: used_x + rel_dx,
            y: y + rel_dy,
            width: box_width,
            height: h,
        };
        return h
            + layout_box.margin.top
            + layout_box.margin.bottom
            + layout_box.border.vertical()
            + layout_box.padding.vertical();
    }

    if layout_box.kind == BoxKind::Inline {
        layout_inline_children(layout_box, child_x(layout_box, x), child_y, line_height_px);
        layout_box.rect = Rect {
            x: used_x + rel_dx,
            y: y + rel_dy,
            width: box_width,
            height: line_height_px,
        };
        return line_height_px + layout_box.margin.top + layout_box.margin.bottom;
    }

    if layout_box.kind == BoxKind::Replaced {
        let (rw, rh) = layout_box.replaced_size.unwrap_or((300.0, 150.0));
        layout_box.rect = Rect {
            x: used_x + rel_dx,
            y: y + rel_dy,
            width: rw,
            height: rh,
        };
        return rh
            + layout_box.margin.vertical()
            + layout_box.border.vertical()
            + layout_box.padding.vertical();
    }

    let mut cursor_y = child_y;
    let mut idx = 0;
    while idx < layout_box.children.len() {
        let is_inline = |b: &LayoutBox| {
            matches!(b.kind, BoxKind::Inline | BoxKind::Text | BoxKind::Replaced)
        };

        if is_inline(&layout_box.children[idx]) {
            let mut row = Vec::new();
            let mut row_w = 0.0f32;
            while idx < layout_box.children.len() && is_inline(&layout_box.children[idx]) {
                let child = &layout_box.children[idx];
                let child_w = intrinsic_inline_width(child, line_height_px);
                if !row.is_empty() && row_w + child_w > content_width {
                    break;
                }
                row.push(idx);
                row_w += child_w;
                idx += 1;
            }

            let mut cursor_x = child_x(layout_box, used_x);
            for index in row {
                let child = &mut layout_box.children[index];
                let width = intrinsic_inline_width(child, line_height_px);
                let height = if child.kind == BoxKind::Replaced {
                    child.replaced_size.map(|(_, h)| h).unwrap_or(line_height_px)
                } else {
                    line_height_px
                };
                child.rect = Rect {
                    x: cursor_x,
                    y: cursor_y,
                    width,
                    height,
                };
                if child.kind == BoxKind::Inline {
                    layout_inline_children(child, cursor_x, cursor_y, line_height_px);
                }
                cursor_x += width;
            }
            cursor_y += line_height_px;
        } else {
            let child = &mut layout_box.children[idx];
            let child_containing = Rect {
                x: used_x,
                y: child_y,
                width: content_width.max(0.0),
                height: f32::MAX,
            };
            let ch = layout_block_box(
                child,
                positioned,
                child_x(child, used_x),
                cursor_y,
                content_width,
                Some(&child_containing),
                line_height_px,
            );
            cursor_y += ch;
            idx += 1;
        }
    }

    let total_content_h = cursor_y - child_y;
    let explicit_h = layout_box
        .style
        .height
        .to_px(line_height_px, pb_width, 16.0);
    let final_h = if explicit_h > 0.0 {
        explicit_h
    } else {
        total_content_h
    };
    layout_box.rect = Rect {
        x: used_x + rel_dx,
        y: y + rel_dy,
        width: box_width,
        height: final_h,
    };
    final_h
        + layout_box.margin.top
        + layout_box.margin.bottom
        + layout_box.border.vertical()
        + layout_box.padding.vertical()
}

fn child_x(layout_box: &LayoutBox, used_x: f32) -> f32 {
    used_x + layout_box.padding.left + layout_box.border.left
}

fn layout_flex_children(
    layout_box: &mut LayoutBox,
    positioned: &mut Vec<LayoutBox>,
    content_x: f32,
    content_y: f32,
    content_width: f32,
    containing_block: Option<&Rect>,
    line_height_px: f32,
) -> f32 {
    let mut cursor_x = content_x;
    let mut cursor_y = content_y;
    let mut row_height = 0.0f32;
    let mut max_y = content_y;

    for child in &mut layout_box.children {
        let child_width = flex_item_width(child, content_width, line_height_px)
            .min(content_width.max(0.0))
            .max(0.0);
        if cursor_x > content_x && cursor_x + child_width > content_x + content_width {
            cursor_x = content_x;
            cursor_y += row_height.max(line_height_px);
            row_height = 0.0;
        }

        let child_height = layout_block_box(
            child,
            positioned,
            cursor_x,
            cursor_y,
            child_width,
            containing_block,
            line_height_px,
        );
        cursor_x += child.rect.width + child.margin.horizontal();
        row_height = row_height.max(child_height);
        max_y = max_y.max(cursor_y + child_height);
    }

    (max_y - content_y).max(row_height)
}

fn flex_item_width(layout_box: &LayoutBox, parent_width: f32, line_height_px: f32) -> f32 {
    used_width(layout_box.style.width, parent_width).unwrap_or_else(|| {
        if matches!(layout_box.kind, BoxKind::Inline | BoxKind::Text) {
            intrinsic_inline_width(layout_box, line_height_px)
        } else {
            parent_width / 3.0
        }
    })
}

#[allow(dead_code)]
fn place_inline_row(
    children: &mut [LayoutBox],
    row: &[usize],
    x: f32,
    y: f32,
    line_height_px: f32,
) -> f32 {
    let mut cursor_x = x;
    for index in row {
        let child = &mut children[*index];
        let width = intrinsic_inline_width(child, line_height_px);
        child.rect = Rect {
            x: cursor_x,
            y,
            width,
            height: line_height_px,
        };
        if child.kind == BoxKind::Inline {
            layout_inline_children(child, cursor_x, y, line_height_px);
        }
        cursor_x += width;
    }
    line_height_px
}

fn layout_inline_children(layout_box: &mut LayoutBox, x: f32, y: f32, line_height_px: f32) {
    let mut cursor_x = x;
    for child in &mut layout_box.children {
        let width = intrinsic_inline_width(child, line_height_px);
        child.rect = Rect {
            x: cursor_x,
            y,
            width,
            height: line_height_px,
        };
        if child.kind == BoxKind::Inline {
            layout_inline_children(child, cursor_x, y, line_height_px);
        }
        cursor_x += width;
    }
}

fn intrinsic_inline_width(layout_box: &LayoutBox, line_height_px: f32) -> f32 {
    if layout_box.kind == BoxKind::Text {
        let font_size = layout_box.style.font_size.to_px(line_height_px, 1280.0, 16.0);
        let letter_spacing = layout_box.style.letter_spacing.to_px(line_height_px, 1280.0, 16.0);
        let word_spacing = layout_box.style.word_spacing.to_px(line_height_px, 1280.0, 16.0);
        text_width(
            layout_box.text.as_deref().unwrap_or_default(),
            font_size,
            letter_spacing,
            word_spacing,
        )
    } else if layout_box.kind == BoxKind::Replaced {
        layout_box
            .replaced_size
            .map(|(width, _)| width)
            .unwrap_or(300.0)
    } else {
        layout_box
            .children
            .iter()
            .map(|child| intrinsic_inline_width(child, line_height_px))
            .sum::<f32>()
            .max(line_height_px)
    }
}

fn text_width(text: &str, font_size_px: f32, letter_spacing: f32, word_spacing: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let char_count = text.chars().count() as f32;
    let word_count = text.split_whitespace().count().max(1) as f32;
    // Average char width ≈ 0.55em for typical Latin text
    let total_chars_width = char_count * font_size_px * 0.55;
    let total_letter_spacing = char_count * letter_spacing;
    let total_word_spacing = (word_count - 1.0) * word_spacing;
    total_chars_width + total_letter_spacing + total_word_spacing
}

fn line_height_px_if_leaf(layout_box: &LayoutBox, line_height_px: f32) -> f32 {
    if layout_box.children.is_empty() {
        line_height_px
    } else {
        0.0
    }
}

fn used_width(length: Length, parent_width: f32) -> Option<f32> {
    match length {
        Length::Px(value) => Some(value),
        Length::Percent(value) => Some(parent_width * value / 100.0),
        Length::Em(value) => Some(value * 16.0),
        _ => None,
    }
}

#[allow(dead_code)]
fn used_height(length: Length) -> Option<f32> {
    match length {
        Length::Px(value) => Some(value),
        Length::Em(value) => Some(value * 16.0),
        Length::Percent(_) | Length::Auto | Length::None => None,
        _ => None,
    }
}

fn used_edges(edges: fortrust_style::EdgeSizes, parent_width: f32) -> UsedEdges {
    UsedEdges {
        top: used_edge(edges.top, parent_width),
        right: used_edge(edges.right, parent_width),
        bottom: used_edge(edges.bottom, parent_width),
        left: used_edge(edges.left, parent_width),
    }
}

fn used_edge(length: Length, parent_width: f32) -> f32 {
    match length {
        Length::Px(value) => value,
        Length::Em(value) => value * 16.0,
        Length::Percent(value) => parent_width * value / 100.0,
        Length::Auto | Length::None => 0.0,
        _ => 0.0,
    }
}

fn zero_rect() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    }
}

fn parse_replaced_size(element: &fortrust_dom::ElementData<'_>) -> Option<(f32, f32)> {
    let width = parse_dimension_attr(element.attr("width")?.as_str())?;
    let height = parse_dimension_attr(element.attr("height")?.as_str())?;
    Some((width, height))
}

fn parse_dimension_attr(value: &str) -> Option<f32> {
    let parsed = value.trim().parse::<f32>().ok()?;
    if parsed.is_finite() && parsed > 0.0 {
        Some(parsed.min(4096.0))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use fortrust_dom::{DomArena, parse_html};
    use fortrust_style::{Display, Stylesheet};

    use super::*;

    #[test]
    fn lays_out_block_children_vertically() {
        let arena = DomArena::new();
        let document = parse_html(&arena, "<body><div>A</div><div>B</div></body>").unwrap();
        let mut style = StyleEngine::new();
        style.add_stylesheet(Stylesheet::parse("div { height: 24px; margin: 2px; }").unwrap());
        let body = document.first_element_by_tag("body").unwrap();
        let tree = LayoutEngine::new(style)
            .layout(
                body,
                LayoutConstraints {
                    viewport_width: 800.0,
                    viewport_height: 600.0,
                    containing_block: None,
                },
            )
            .unwrap();

        assert_eq!(tree.root.kind, BoxKind::Block);
        assert_eq!(tree.root.children.len(), 2);
        assert!(tree.root.children[1].rect.y > tree.root.children[0].rect.y);
    }

    #[test]
    fn skips_display_none_subtrees() {
        let arena = DomArena::new();
        let document =
            parse_html(&arena, "<body><script>secret()</script><p>Shown</p></body>").unwrap();
        let body = document.first_element_by_tag("body").unwrap();
        let tree = LayoutEngine::new(StyleEngine::new())
            .layout(
                body,
                LayoutConstraints {
                    viewport_width: 400.0,
                    viewport_height: 300.0,
                    containing_block: None,
                },
            )
            .unwrap();

        assert_eq!(tree.root.children.len(), 1);
        assert_eq!(tree.root.children[0].node_name, "p");
    }

    #[test]
    fn places_inline_text_on_one_row() {
        let arena = DomArena::new();
        let document = parse_html(&arena, "<body>Hello <strong>secure</strong></body>").unwrap();
        let body = document.first_element_by_tag("body").unwrap();
        let tree = LayoutEngine::new(StyleEngine::new())
            .layout(
                body,
                LayoutConstraints {
                    viewport_width: 400.0,
                    viewport_height: 300.0,
                    containing_block: None,
                },
            )
            .unwrap();

        assert_eq!(tree.root.children[0].kind, BoxKind::Text);
        assert_eq!(tree.root.children[1].style.display, Display::Inline);
        assert_eq!(tree.root.children[0].rect.y, tree.root.children[1].rect.y);
    }

    #[test]
    fn resolves_percent_width_against_available_width() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<body><main>Content</main></body>"#).unwrap();
        let mut style = StyleEngine::new();
        style.add_stylesheet(Stylesheet::parse("main { width: 50%; height: 20px; }").unwrap());
        let main = document.first_element_by_tag("main").unwrap();
        let tree = LayoutEngine::new(style)
            .layout(
                main,
                LayoutConstraints {
                    viewport_width: 600.0,
                    viewport_height: 400.0,
                    containing_block: None,
                },
            )
            .unwrap();

        assert_eq!(tree.root.rect.width, 300.0);
    }

    #[test]
    fn sizes_replaced_images_from_attributes() {
        let arena = DomArena::new();
        let document = parse_html(
            &arena,
            r#"<body><img src="/logo.png" alt="Fortrust" width="120" height="80"></body>"#,
        )
        .unwrap();
        let body = document.first_element_by_tag("body").unwrap();
        let tree = LayoutEngine::new(StyleEngine::new())
            .layout(
                body,
                LayoutConstraints {
                    viewport_width: 400.0,
                    viewport_height: 300.0,
                    containing_block: None,
                },
            )
            .unwrap();

        let img = &tree.root.children[0];
        assert_eq!(img.kind, BoxKind::Replaced);
        assert_eq!(img.rect.width, 120.0);
        assert_eq!(img.rect.height, 80.0);
        assert_eq!(img.text.as_deref(), Some("Fortrust"));
    }

    #[test]
    fn lays_out_flex_children_in_rows() {
        let arena = DomArena::new();
        let document = parse_html(
            &arena,
            r#"<body><nav><a>One</a><a>Two</a><a>Three</a></nav></body>"#,
        )
        .unwrap();
        let mut style = StyleEngine::new();
        style.add_stylesheet(
            Stylesheet::parse("nav { display: flex; } a { width: 50px; }").unwrap(),
        );
        let nav = document.first_element_by_tag("nav").unwrap();
        let tree = LayoutEngine::new(style)
            .layout(
                nav,
                LayoutConstraints {
                    viewport_width: 180.0,
                    viewport_height: 120.0,
                    containing_block: None,
                },
            )
            .unwrap();

        assert_eq!(tree.root.kind, BoxKind::Flex);
        assert_eq!(tree.root.children.len(), 3);
        assert!(tree.root.children[1].rect.x > tree.root.children[0].rect.x);
        assert_eq!(tree.root.children[0].rect.y, tree.root.children[1].rect.y);
    }

    #[test]
    fn flex_children_wrap_when_width_is_exhausted() {
        let arena = DomArena::new();
        let document = parse_html(
            &arena,
            r#"<body><nav><a>One</a><a>Two</a><a>Three</a></nav></body>"#,
        )
        .unwrap();
        let mut style = StyleEngine::new();
        style.add_stylesheet(
            Stylesheet::parse("nav { display: flex; } a { width: 80px; height: 20px; }").unwrap(),
        );
        let nav = document.first_element_by_tag("nav").unwrap();
        let tree = LayoutEngine::new(style)
            .layout(
                nav,
                LayoutConstraints {
                    viewport_width: 130.0,
                    viewport_height: 120.0,
                    containing_block: None,
                },
            )
            .unwrap();

        assert!(tree.root.children[1].rect.y > tree.root.children[0].rect.y);
    }
}
