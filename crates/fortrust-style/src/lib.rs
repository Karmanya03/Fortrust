use compact_str::CompactString;
use cssparser::{Parser, ParserInput, Token, match_ignore_ascii_case};
use fortrust_dom::NodeRef;
use smallvec::SmallVec;

pub const MAX_STYLESHEET_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleError {
    InputTooLarge {
        limit_bytes: usize,
        actual_bytes: usize,
    },
    UnclosedRule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    Flex,
    None,
    InlineBlock,
    Grid,
    Table,
    TableCell,
    TableRow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontWeight {
    Normal,
    Bold,
    Bolder,
    Lighter,
    Number(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overflow {
    Visible,
    Hidden,
    Scroll,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    Nowrap,
    Pre,
    PreWrap,
    PreLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Visible,
    Hidden,
    Collapse,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    Vw(f32),
    Vh(f32),
    Auto,
    Zero,
    MaxContent,
    MinContent,
    FitContent,
    None,
}

impl Length {
    pub fn to_px(&self, parent_px: f32, viewport_px: f32, font_size_px: f32) -> f32 {
        match self {
            Self::Px(v) => *v,
            Self::Em(v) => v * font_size_px,
            Self::Rem(v) => v * 16.0,
            Self::Percent(v) => parent_px * v / 100.0,
            Self::Vw(v) => viewport_px * v / 100.0,
            Self::Vh(v) => viewport_px * v / 100.0,
            Self::Auto
            | Self::Zero
            | Self::MaxContent
            | Self::MinContent
            | Self::FitContent
            | Self::None => 0.0,
        }
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BorderSizes {
    pub top: Border,
    pub right: Border,
    pub bottom: Border,
    pub left: Border,
}

impl BorderSizes {
    pub fn none() -> Self {
        Self {
            top: Border::none(),
            right: Border::none(),
            bottom: Border::none(),
            left: Border::none(),
        }
    }

    pub fn horizontal(&self) -> f32 {
        self.left.width + self.right.width
    }

    pub fn vertical(&self) -> f32 {
        self.top.width + self.bottom.width
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    pub width: f32,
    pub style: BorderStyle,
    pub color: Color,
}

impl Border {
    pub fn none() -> Self {
        Self {
            width: 0.0,
            style: BorderStyle::None,
            color: Color::TRANSPARENT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub position: Position,
    pub color: Color,
    pub background_color: Color,
    pub font_size: Length,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    pub text_align: TextAlign,
    pub white_space: WhiteSpace,
    pub visibility: Visibility,
    pub width: Length,
    pub min_width: Length,
    pub max_width: Length,
    pub height: Length,
    pub min_height: Length,
    pub max_height: Length,
    pub margin: EdgeSizes,
    pub padding: EdgeSizes,
    pub border: BorderSizes,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub left: Length,
    pub right: Length,
    pub top: Length,
    pub bottom: Length,
    pub z_index: i32,
    pub opacity: f32,
    pub line_height: Length,
    pub letter_spacing: Length,
    pub word_spacing: Length,
    pub text_indent: Length,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Length,
    pub order: i32,
    pub gap: Length,
    pub row_gap: Length,
    pub column_gap: Length,
}

impl ComputedStyle {
    pub fn initial() -> Self {
        Self {
            display: Display::Inline,
            position: Position::Static,
            color: Color::BLACK,
            background_color: Color::TRANSPARENT,
            font_size: Length::Px(16.0),
            font_weight: FontWeight::Normal,
            font_style: FontStyle::Normal,
            text_align: TextAlign::Left,
            white_space: WhiteSpace::Normal,
            visibility: Visibility::Visible,
            width: Length::Auto,
            min_width: Length::Zero,
            max_width: Length::None,
            height: Length::Auto,
            min_height: Length::Zero,
            max_height: Length::None,
            margin: EdgeSizes::zero(),
            padding: EdgeSizes::zero(),
            border: BorderSizes::none(),
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            left: Length::Auto,
            right: Length::Auto,
            top: Length::Auto,
            bottom: Length::Auto,
            z_index: 0,
            opacity: 1.0,
            line_height: Length::Em(1.2),
            letter_spacing: Length::Px(0.0),
            word_spacing: Length::Px(0.0),
            text_indent: Length::Px(0.0),
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Length::Auto,
            order: 0,
            gap: Length::Px(0.0),
            row_gap: Length::Px(0.0),
            column_gap: Length::Px(0.0),
        }
    }

    pub fn inherit_from(parent: Option<&Self>) -> Self {
        let mut style = Self::initial();
        if let Some(parent) = parent {
            style.color = parent.color;
            style.font_size = parent.font_size;
            style.font_weight = parent.font_weight;
            style.font_style = parent.font_style;
            style.text_align = parent.text_align;
            style.white_space = parent.white_space;
            style.visibility = parent.visibility;
            style.line_height = parent.line_height;
            style.letter_spacing = parent.letter_spacing;
            style.word_spacing = parent.word_spacing;
        }
        style
    }

    pub fn is_positioned(&self) -> bool {
        !matches!(self.position, Position::Static)
    }

    pub fn is_absolutely_positioned(&self) -> bool {
        matches!(self.position, Position::Absolute | Position::Fixed)
    }

    pub fn is_overflow_hidden(&self) -> bool {
        matches!(
            self.overflow_x,
            Overflow::Hidden | Overflow::Scroll | Overflow::Auto
        ) || matches!(
            self.overflow_y,
            Overflow::Hidden | Overflow::Scroll | Overflow::Auto
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeSizes {
    pub top: Length,
    pub right: Length,
    pub bottom: Length,
    pub left: Length,
}

impl EdgeSizes {
    pub fn zero() -> Self {
        Self {
            top: Length::Px(0.0),
            right: Length::Px(0.0),
            bottom: Length::Px(0.0),
            left: Length::Px(0.0),
        }
    }

    pub fn horizontal(&self, parent_px: f32, viewport_px: f32, font_size_px: f32) -> f32 {
        self.left.to_px(parent_px, viewport_px, font_size_px)
            + self.right.to_px(parent_px, viewport_px, font_size_px)
    }

    pub fn vertical(&self, parent_px: f32, viewport_px: f32, font_size_px: f32) -> f32 {
        self.top.to_px(parent_px, viewport_px, font_size_px)
            + self.bottom.to_px(parent_px, viewport_px, font_size_px)
    }
}

impl From<BorderSizes> for EdgeSizes {
    fn from(borders: BorderSizes) -> Self {
        Self {
            top: if borders.top.style == BorderStyle::None {
                Length::Px(0.0)
            } else {
                Length::Px(borders.top.width)
            },
            right: if borders.right.style == BorderStyle::None {
                Length::Px(0.0)
            } else {
                Length::Px(borders.right.width)
            },
            bottom: if borders.bottom.style == BorderStyle::None {
                Length::Px(0.0)
            } else {
                Length::Px(borders.bottom.width)
            },
            left: if borders.left.style == BorderStyle::None {
                Length::Px(0.0)
            } else {
                Length::Px(borders.left.width)
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    property: CompactString,
    value: PropertyValue,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum PropertyValue {
    Display(Display),
    Color(Color),
    Length(Length),
    Edges(EdgeSizes),
    FontWeight(FontWeight),
    FontStyle(FontStyle),
    TextAlign(TextAlign),
    WhiteSpace(WhiteSpace),
    Visibility(Visibility),
    Position(Position),
    Overflow(Overflow),
    Border(BorderSizes),
    ZIndex(i32),
    Opacity(f32),
    FlexGrow(f32),
    FlexShrink(f32),
    Order(i32),
    String(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    parts: SmallVec<[SimpleSelector; 3]>,
    specificity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SimpleSelector {
    tag: Option<CompactString>,
    id: Option<CompactString>,
    classes: SmallVec<[CompactString; 3]>,
    pseudo_class: Option<CompactString>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    selectors: SmallVec<[Selector; 2]>,
    declarations: SmallVec<[Declaration; 6]>,
    order: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stylesheet {
    rules: Vec<Rule>,
}

impl Stylesheet {
    pub fn parse(input: &str) -> Result<Self, StyleError> {
        if input.len() > MAX_STYLESHEET_BYTES {
            return Err(StyleError::InputTooLarge {
                limit_bytes: MAX_STYLESHEET_BYTES,
                actual_bytes: input.len(),
            });
        }

        let mut rules = Vec::new();
        let mut rest = input;
        while let Some(open) = rest.find('{') {
            let selector_text = rest[..open].trim();
            let after_open = &rest[open + 1..];
            let Some(close) = after_open.find('}') else {
                return Err(StyleError::UnclosedRule);
            };

            let body = &after_open[..close];
            let selectors = parse_selectors(selector_text);
            let declarations = parse_declarations(body);
            if !selectors.is_empty() && !declarations.is_empty() {
                rules.push(Rule {
                    selectors,
                    declarations,
                    order: rules.len(),
                });
            }

            rest = &after_open[close + 1..];
        }

        Ok(Self { rules })
    }

    fn ua_defaults() -> Self {
        Self::parse(
            r#"
            html, body, div, section, article, nav, main, header, footer, p,
            h1, h2, h3, h4, h5, h6, ul, ol, li, form, table, figure, figcaption,
            header, footer, aside, main, details, summary { display: block; }
            title, meta, style, script, head, link { display: none; }
            h1 { font-size: 32px; font-weight: bold; margin: 8px; }
            h2 { font-size: 24px; font-weight: bold; margin: 8px; }
            h3 { font-size: 19px; font-weight: bold; margin: 8px; }
            h4 { font-size: 16px; font-weight: bold; margin: 8px; }
            h5 { font-size: 13px; font-weight: bold; margin: 8px; }
            h6 { font-size: 11px; font-weight: bold; margin: 8px; }
            p { margin-top: 8px; margin-bottom: 8px; }
            ul, ol { margin-top: 8px; margin-bottom: 8px; padding-left: 32px; }
            li { margin-top: 4px; margin-bottom: 4px; }
            strong, b { font-weight: bold; }
            em, i { font-style: italic; }
            u { text-decoration: underline; }
            a { color: #0000ee; text-decoration: underline; }
            body { margin: 8px; color: #111111; background-color: #ffffff; }
            pre, code { white-space: pre; font-family: monospace; }
            blockquote { margin-left: 32px; margin-right: 32px; }
            table { display: table; }
            tr { display: table-row; }
            td, th { display: table-cell; padding: 4px; }
            img { max-width: 100%; }
            hr { border: 1px solid #cccccc; margin: 8px 0; }
            small { font-size: 13px; }
            "#,
        )
        .expect("built-in UA stylesheet must parse")
    }
}

#[derive(Debug, Clone)]
pub struct StyleEngine {
    stylesheets: Vec<Stylesheet>,
}

impl StyleEngine {
    pub fn new() -> Self {
        Self {
            stylesheets: vec![Stylesheet::ua_defaults()],
        }
    }

    pub fn add_stylesheet(&mut self, stylesheet: Stylesheet) {
        self.stylesheets.push(stylesheet);
    }

    pub fn compute_style<'arena>(
        &self,
        node: NodeRef<'arena>,
        parent: Option<&ComputedStyle>,
    ) -> ComputedStyle {
        let mut style = ComputedStyle::inherit_from(parent);
        let Some(element) = node.as_element() else {
            return style;
        };

        let mut matched = Vec::new();
        for stylesheet in &self.stylesheets {
            for rule in &stylesheet.rules {
                for selector in &rule.selectors {
                    if selector.matches(node) {
                        matched.push((selector.specificity, rule.order, &rule.declarations));
                    }
                }
            }
        }

        matched.sort_by_key(|(specificity, order, _)| (*specificity, *order));
        for (_, _, declarations) in matched {
            apply_declarations(&mut style, declarations);
        }

        if let Some(inline) = element.attr("style") {
            let declarations = parse_declarations(&inline);
            apply_declarations(&mut style, &declarations);
        }

        style
    }
}

impl Default for StyleEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Selector {
    fn parse(input: &str) -> Option<Self> {
        let parts = input
            .split_whitespace()
            .filter_map(SimpleSelector::parse)
            .collect::<SmallVec<_>>();
        if parts.is_empty() {
            return None;
        }

        let specificity = parts.iter().map(SimpleSelector::specificity).sum();
        Some(Self { parts, specificity })
    }

    fn matches<'arena>(&self, node: NodeRef<'arena>) -> bool {
        let mut parts = self.parts.iter().rev();
        let Some(rightmost) = parts.next() else {
            return false;
        };
        if !rightmost.matches(node) {
            return false;
        }

        let mut current = node.parent();
        for selector in parts {
            let Some(found) = find_matching_ancestor(current, selector) else {
                return false;
            };
            current = found.parent();
        }
        true
    }
}

impl SimpleSelector {
    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() || raw == "*" {
            return Some(Self {
                tag: None,
                id: None,
                classes: SmallVec::new(),
                pseudo_class: None,
            });
        }

        let mut tag = None;
        let mut id = None;
        let mut classes = SmallVec::new();
        let mut pseudo_class = None;
        let bytes = raw.as_bytes();
        let mut start = 0usize;
        let mut mode = SelectorToken::Tag;

        for index in 0..=bytes.len() {
            let boundary = index == bytes.len() || matches!(bytes[index], b'#' | b'.' | b':');
            if !boundary {
                continue;
            }

            if index > start {
                let value = CompactString::from(&raw[start..index]);
                match mode {
                    SelectorToken::Tag => tag = Some(value),
                    SelectorToken::Id => id = Some(value),
                    SelectorToken::Class => classes.push(value),
                    SelectorToken::Pseudo => pseudo_class = Some(value),
                }
            }

            if index < bytes.len() {
                mode = if bytes[index] == b'#' {
                    SelectorToken::Id
                } else if bytes[index] == b':' {
                    SelectorToken::Pseudo
                } else {
                    SelectorToken::Class
                };
                start = index + 1;
            }
        }

        Some(Self {
            tag,
            id,
            classes,
            pseudo_class,
        })
    }

    fn specificity(&self) -> u32 {
        let id = u32::from(self.id.is_some());
        let class = self.classes.len() as u32 + u32::from(self.pseudo_class.is_some());
        let tag = u32::from(self.tag.is_some());
        (id << 16) | (class << 8) | tag
    }

    fn matches<'arena>(&self, node: NodeRef<'arena>) -> bool {
        let Some(element) = node.as_element() else {
            return false;
        };

        if let Some(tag) = &self.tag
            && !element.local_name().eq_ignore_ascii_case(tag)
        {
            return false;
        }

        if let Some(id) = &self.id
            && element.attr("id").as_deref() != Some(id.as_str())
        {
            return false;
        }

        let class_attr = element.attr("class").unwrap_or_default();
        let classes = class_attr
            .split_whitespace()
            .collect::<SmallVec<[&str; 6]>>();
        if !self
            .classes
            .iter()
            .all(|class| classes.iter().any(|candidate| candidate == class))
        {
            return false;
        }

        if let Some(pseudo) = &self.pseudo_class {
            match pseudo.as_str() {
                "hover" | "active" | "focus" | "visited" | "link" => {
                    // Pseudo-classes are always considered matching for static rendering
                }
                "first-child" => {
                    if let Some(parent) = node.parent()
                        && let Some(first) = parent.children().first()
                    {
                        return std::ptr::addr_eq(first, node);
                    }
                    return false;
                }
                "last-child" => {
                    if let Some(parent) = node.parent()
                        && let Some(last) = parent.children().last()
                    {
                        return std::ptr::addr_eq(last, node);
                    }
                    return false;
                }
                "nth-child" | "nth-of-type" | "first-of-type" | "last-of-type" => {
                    // Always match for static rendering
                }
                "before" | "after" | "placeholder" | "selection" => {
                    return false;
                }
                _ => {}
            }
        }

        true
    }
}

#[derive(Debug, Clone, Copy)]
enum SelectorToken {
    Tag,
    Id,
    Class,
    Pseudo,
}

fn find_matching_ancestor<'arena>(
    mut current: Option<NodeRef<'arena>>,
    selector: &SimpleSelector,
) -> Option<NodeRef<'arena>> {
    while let Some(node) = current {
        if selector.matches(node) {
            return Some(node);
        }
        current = node.parent();
    }
    None
}

fn parse_selectors(input: &str) -> SmallVec<[Selector; 2]> {
    input.split(',').filter_map(Selector::parse).collect()
}

fn parse_declarations(input: &str) -> SmallVec<[Declaration; 6]> {
    input
        .split(';')
        .filter_map(|chunk| {
            let (property, raw_value) = chunk.split_once(':')?;
            let property = property.trim().to_ascii_lowercase();
            let value = parse_property_value(&property, raw_value.trim())?;
            Some(Declaration {
                property: CompactString::from(property),
                value,
            })
        })
        .collect()
}

fn parse_property_value(property: &str, value: &str) -> Option<PropertyValue> {
    let value = value.trim();
    let lowered = value.to_ascii_lowercase();

    match property {
        "display" => match lowered.as_str() {
            "block" => Some(PropertyValue::Display(Display::Block)),
            "inline" => Some(PropertyValue::Display(Display::Inline)),
            "flex" | "inline-flex" => Some(PropertyValue::Display(Display::Flex)),
            "grid" | "inline-grid" => Some(PropertyValue::Display(Display::Grid)),
            "none" => Some(PropertyValue::Display(Display::None)),
            "inline-block" => Some(PropertyValue::Display(Display::InlineBlock)),
            "table" => Some(PropertyValue::Display(Display::Table)),
            "table-cell" => Some(PropertyValue::Display(Display::TableCell)),
            "table-row" => Some(PropertyValue::Display(Display::TableRow)),
            _ => None,
        },
        "color" | "background-color" | "border-color" | "outline-color" => {
            parse_color(value).map(PropertyValue::Color)
        }
        "font-size" | "width" | "height" | "min-width" | "max-width" | "min-height"
        | "max-height" | "margin-top" | "margin-right" | "margin-bottom" | "margin-left"
        | "padding-top" | "padding-right" | "padding-bottom" | "padding-left" | "left"
        | "right" | "top" | "bottom" | "line-height" | "letter-spacing" | "word-spacing"
        | "text-indent" => parse_length(value).map(PropertyValue::Length),
        "margin" | "padding" => parse_edge_sizes(value).map(PropertyValue::Edges),
        "font-weight" => match lowered.as_str() {
            "bold" | "700" => Some(PropertyValue::FontWeight(FontWeight::Bold)),
            "normal" | "400" => Some(PropertyValue::FontWeight(FontWeight::Normal)),
            "bolder" => Some(PropertyValue::FontWeight(FontWeight::Bolder)),
            "lighter" => Some(PropertyValue::FontWeight(FontWeight::Lighter)),
            _ => value
                .parse::<u16>()
                .ok()
                .map(FontWeight::Number)
                .map(PropertyValue::FontWeight),
        },
        "font-style" => match lowered.as_str() {
            "normal" => Some(PropertyValue::FontStyle(FontStyle::Normal)),
            "italic" => Some(PropertyValue::FontStyle(FontStyle::Italic)),
            "oblique" => Some(PropertyValue::FontStyle(FontStyle::Oblique)),
            _ => None,
        },
        "text-align" => match lowered.as_str() {
            "left" => Some(PropertyValue::TextAlign(TextAlign::Left)),
            "right" => Some(PropertyValue::TextAlign(TextAlign::Right)),
            "center" => Some(PropertyValue::TextAlign(TextAlign::Center)),
            "justify" => Some(PropertyValue::TextAlign(TextAlign::Justify)),
            _ => None,
        },
        "white-space" => match lowered.as_str() {
            "normal" => Some(PropertyValue::WhiteSpace(WhiteSpace::Normal)),
            "nowrap" => Some(PropertyValue::WhiteSpace(WhiteSpace::Nowrap)),
            "pre" => Some(PropertyValue::WhiteSpace(WhiteSpace::Pre)),
            "pre-wrap" => Some(PropertyValue::WhiteSpace(WhiteSpace::PreWrap)),
            "pre-line" => Some(PropertyValue::WhiteSpace(WhiteSpace::PreLine)),
            _ => None,
        },
        "visibility" => match lowered.as_str() {
            "visible" => Some(PropertyValue::Visibility(Visibility::Visible)),
            "hidden" => Some(PropertyValue::Visibility(Visibility::Hidden)),
            "collapse" => Some(PropertyValue::Visibility(Visibility::Collapse)),
            _ => None,
        },
        "position" => match lowered.as_str() {
            "static" => Some(PropertyValue::Position(Position::Static)),
            "relative" => Some(PropertyValue::Position(Position::Relative)),
            "absolute" => Some(PropertyValue::Position(Position::Absolute)),
            "fixed" => Some(PropertyValue::Position(Position::Fixed)),
            "sticky" => Some(PropertyValue::Position(Position::Sticky)),
            _ => None,
        },
        "overflow" | "overflow-x" => match lowered.as_str() {
            "visible" => Some(PropertyValue::Overflow(Overflow::Visible)),
            "hidden" => Some(PropertyValue::Overflow(Overflow::Hidden)),
            "scroll" => Some(PropertyValue::Overflow(Overflow::Scroll)),
            "auto" => Some(PropertyValue::Overflow(Overflow::Auto)),
            _ => None,
        },
        "overflow-y" => match lowered.as_str() {
            "visible" => Some(PropertyValue::Overflow(Overflow::Visible)),
            "hidden" => Some(PropertyValue::Overflow(Overflow::Hidden)),
            "scroll" => Some(PropertyValue::Overflow(Overflow::Scroll)),
            "auto" => Some(PropertyValue::Overflow(Overflow::Auto)),
            _ => None,
        },
        "z-index" => value.parse::<i32>().ok().map(PropertyValue::ZIndex),
        "opacity" => value
            .parse::<f32>()
            .ok()
            .map(|v| PropertyValue::Opacity(v.clamp(0.0, 1.0))),
        "flex-grow" => value.parse::<f32>().ok().map(PropertyValue::FlexGrow),
        "flex-shrink" => value.parse::<f32>().ok().map(PropertyValue::FlexShrink),
        "order" => value.parse::<i32>().ok().map(PropertyValue::Order),
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            parse_border_shorthand(value)
        }
        "border-width"
        | "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width" => parse_length(value).map(PropertyValue::Length),
        _ => None,
    }
}

fn apply_declarations(style: &mut ComputedStyle, declarations: &[Declaration]) {
    for declaration in declarations {
        match (declaration.property.as_str(), &declaration.value) {
            ("display", PropertyValue::Display(value)) => style.display = *value,
            ("color", PropertyValue::Color(value)) => style.color = *value,
            ("background-color", PropertyValue::Color(value)) => style.background_color = *value,
            ("font-size", PropertyValue::Length(value)) => style.font_size = *value,
            ("font-weight", PropertyValue::FontWeight(value)) => style.font_weight = *value,
            ("font-style", PropertyValue::FontStyle(value)) => style.font_style = *value,
            ("text-align", PropertyValue::TextAlign(value)) => style.text_align = *value,
            ("white-space", PropertyValue::WhiteSpace(value)) => style.white_space = *value,
            ("visibility", PropertyValue::Visibility(value)) => style.visibility = *value,
            ("position", PropertyValue::Position(value)) => style.position = *value,
            ("width", PropertyValue::Length(value)) => style.width = *value,
            ("height", PropertyValue::Length(value)) => style.height = *value,
            ("min-width", PropertyValue::Length(value)) => style.min_width = *value,
            ("max-width", PropertyValue::Length(value)) => style.max_width = *value,
            ("min-height", PropertyValue::Length(value)) => style.min_height = *value,
            ("max-height", PropertyValue::Length(value)) => style.max_height = *value,
            ("margin", PropertyValue::Edges(value)) => style.margin = *value,
            ("padding", PropertyValue::Edges(value)) => style.padding = *value,
            ("margin-top", PropertyValue::Length(value)) => style.margin.top = *value,
            ("margin-right", PropertyValue::Length(value)) => style.margin.right = *value,
            ("margin-bottom", PropertyValue::Length(value)) => style.margin.bottom = *value,
            ("margin-left", PropertyValue::Length(value)) => style.margin.left = *value,
            ("padding-top", PropertyValue::Length(value)) => style.padding.top = *value,
            ("padding-right", PropertyValue::Length(value)) => style.padding.right = *value,
            ("padding-bottom", PropertyValue::Length(value)) => style.padding.bottom = *value,
            ("padding-left", PropertyValue::Length(value)) => style.padding.left = *value,
            ("left", PropertyValue::Length(value)) => style.left = *value,
            ("right", PropertyValue::Length(value)) => style.right = *value,
            ("top", PropertyValue::Length(value)) => style.top = *value,
            ("bottom", PropertyValue::Length(value)) => style.bottom = *value,
            ("overflow" | "overflow-x", PropertyValue::Overflow(value)) => {
                style.overflow_x = *value;
                if declaration.property == "overflow" {
                    style.overflow_y = *value;
                }
            }
            ("overflow-y", PropertyValue::Overflow(value)) => style.overflow_y = *value,
            ("z-index", PropertyValue::ZIndex(value)) => style.z_index = *value,
            ("opacity", PropertyValue::Opacity(value)) => style.opacity = *value,
            ("line-height", PropertyValue::Length(value)) => style.line_height = *value,
            ("letter-spacing", PropertyValue::Length(value)) => style.letter_spacing = *value,
            ("word-spacing", PropertyValue::Length(value)) => style.word_spacing = *value,
            ("text-indent", PropertyValue::Length(value)) => style.text_indent = *value,
            ("flex-grow", PropertyValue::FlexGrow(value)) => style.flex_grow = *value,
            ("flex-shrink", PropertyValue::FlexShrink(value)) => style.flex_shrink = *value,
            ("order", PropertyValue::Order(value)) => style.order = *value,
            _ => {}
        }
    }
}

fn parse_border_shorthand(value: &str) -> Option<PropertyValue> {
    let mut width = Length::Px(0.0);
    let mut style = BorderStyle::None;
    let mut color = Color::TRANSPARENT;

    for part in value.split_whitespace() {
        let lowered = part.to_ascii_lowercase();
        if let Some(len) = parse_length(part) {
            width = len;
        } else if let Some(c) = parse_color(part) {
            color = c;
        } else {
            match lowered.as_str() {
                "none" => style = BorderStyle::None,
                "solid" => style = BorderStyle::Solid,
                "dashed" => style = BorderStyle::Dashed,
                "dotted" => style = BorderStyle::Dotted,
                "double" => style = BorderStyle::Double,
                _ => {}
            }
        }
    }

    let w = width.to_px(16.0, 1280.0, 16.0);
    let border = Border {
        width: w,
        style,
        color,
    };
    let mut borders = BorderSizes::none();
    borders.top = border;
    borders.right = border;
    borders.bottom = border;
    borders.left = border;

    Some(PropertyValue::Border(borders))
}

fn parse_color(input: &str) -> Option<Color> {
    if let Some(color) = parse_function_color(input) {
        return Some(color);
    }

    let mut parser_input = ParserInput::new(input);
    let mut parser = Parser::new(&mut parser_input);
    let token = parser.next().ok()?.clone();
    match token {
        Token::IDHash(hash) | Token::Hash(hash) => parse_hex_color(&hash),
        Token::Ident(ident) => match_ignore_ascii_case! { &ident,
            "black" => Some(Color::rgb(0, 0, 0)),
            "white" => Some(Color::rgb(255, 255, 255)),
            "red" => Some(Color::rgb(255, 0, 0)),
            "green" => Some(Color::rgb(0, 128, 0)),
            "blue" => Some(Color::rgb(0, 0, 255)),
            "gray" | "grey" => Some(Color::rgb(128, 128, 128)),
            "silver" => Some(Color::rgb(192, 192, 192)),
            "maroon" => Some(Color::rgb(128, 0, 0)),
            "purple" => Some(Color::rgb(128, 0, 128)),
            "teal" => Some(Color::rgb(0, 128, 128)),
            "navy" => Some(Color::rgb(0, 0, 128)),
            "yellow" => Some(Color::rgb(255, 255, 0)),
            "orange" => Some(Color::rgb(255, 165, 0)),
            "transparent" => Some(Color::TRANSPARENT),
            _ => None,
        },
        _ => None,
    }
}

fn parse_function_color(input: &str) -> Option<Color> {
    let input = input.trim();
    let open = input.find('(')?;
    let close = input.rfind(')')?;
    if close <= open {
        return None;
    }

    let name = input[..open].trim();
    if !name.eq_ignore_ascii_case("rgb") && !name.eq_ignore_ascii_case("rgba") {
        return None;
    }

    let body = &input[open + 1..close];
    let parts = body
        .split([',', ' ', '/'])
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }

    let r = parse_color_channel(parts[0])?;
    let g = parse_color_channel(parts[1])?;
    let b = parse_color_channel(parts[2])?;
    let a = parts
        .get(3)
        .and_then(|part| parse_alpha_channel(part))
        .unwrap_or(255);

    Some(Color { r, g, b, a })
}

fn parse_color_channel(input: &str) -> Option<u8> {
    let input = input.trim();
    if let Some(percent) = input.strip_suffix('%') {
        let value = percent.parse::<f32>().ok()?;
        return Some((value.clamp(0.0, 100.0) * 2.55).round() as u8);
    }

    let value = input.parse::<f32>().ok()?;
    Some(value.clamp(0.0, 255.0).round() as u8)
}

fn parse_alpha_channel(input: &str) -> Option<u8> {
    let input = input.trim();
    if let Some(percent) = input.strip_suffix('%') {
        let value = percent.parse::<f32>().ok()?;
        return Some((value.clamp(0.0, 100.0) * 2.55).round() as u8);
    }

    let value = input.parse::<f32>().ok()?;
    if value <= 1.0 {
        Some((value.clamp(0.0, 1.0) * 255.0).round() as u8)
    } else {
        Some(value.clamp(0.0, 255.0).round() as u8)
    }
}

fn parse_hex_color(hash: &str) -> Option<Color> {
    match hash.len() {
        3 => {
            let mut chars = hash.chars();
            let r = hex_pair(chars.next()?)?;
            let g = hex_pair(chars.next()?)?;
            let b = hex_pair(chars.next()?)?;
            Some(Color::rgb(r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hash[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hash[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hash[4..6], 16).ok()?;
            Some(Color::rgb(r, g, b))
        }
        _ => None,
    }
}

fn hex_pair(ch: char) -> Option<u8> {
    let value = ch.to_digit(16)? as u8;
    Some((value << 4) | value)
}

fn parse_edge_sizes(input: &str) -> Option<EdgeSizes> {
    let values = input
        .split_whitespace()
        .map(parse_length)
        .collect::<Option<Vec<_>>>()?;

    match values.as_slice() {
        [all] => Some(EdgeSizes {
            top: *all,
            right: *all,
            bottom: *all,
            left: *all,
        }),
        [vertical, horizontal] => Some(EdgeSizes {
            top: *vertical,
            right: *horizontal,
            bottom: *vertical,
            left: *horizontal,
        }),
        [top, horizontal, bottom] => Some(EdgeSizes {
            top: *top,
            right: *horizontal,
            bottom: *bottom,
            left: *horizontal,
        }),
        [top, right, bottom, left] => Some(EdgeSizes {
            top: *top,
            right: *right,
            bottom: *bottom,
            left: *left,
        }),
        _ => None,
    }
}

fn parse_length(input: &str) -> Option<Length> {
    let trimmed = input.trim().to_ascii_lowercase();
    if trimmed == "auto" {
        return Some(Length::Auto);
    }
    if trimmed == "0" {
        return Some(Length::Px(0.0));
    }
    if trimmed == "max-content" {
        return Some(Length::MaxContent);
    }
    if trimmed == "min-content" {
        return Some(Length::MinContent);
    }
    if trimmed == "fit-content" {
        return Some(Length::FitContent);
    }

    let mut parser_input = ParserInput::new(input);
    let mut parser = Parser::new(&mut parser_input);
    let token = parser.next().ok()?.clone();
    match token {
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("px") => {
            Some(Length::Px(value))
        }
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("em") => {
            Some(Length::Em(value))
        }
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("rem") => {
            Some(Length::Rem(value))
        }
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("vw") => {
            Some(Length::Vw(value))
        }
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("vh") => {
            Some(Length::Vh(value))
        }
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("pt") => {
            Some(Length::Px(value * 1.33333))
        }
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("cm") => {
            Some(Length::Px(value * 37.7953))
        }
        Token::Dimension { value, unit, .. } if unit.eq_ignore_ascii_case("mm") => {
            Some(Length::Px(value * 3.77953))
        }
        Token::Percentage { unit_value, .. } => Some(Length::Percent(unit_value * 100.0)),
        Token::Number { value: 0.0, .. } => Some(Length::Px(0.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use fortrust_dom::{DomArena, parse_html};

    use super::*;

    #[test]
    fn ua_defaults_make_body_block_with_margin() {
        let arena = DomArena::new();
        let document = parse_html(&arena, "<body>Hi</body>").unwrap();
        let body = document.first_element_by_tag("body").unwrap();
        let style = StyleEngine::new().compute_style(body, None);

        assert_eq!(style.display, Display::Block);
        assert_eq!(style.margin.top, Length::Px(8.0));
    }

    #[test]
    fn id_specificity_beats_class_and_tag() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<p id="hero" class="muted">Hello</p>"#).unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(
            Stylesheet::parse(
                r#"
                p { color: red; }
                .muted { color: blue; }
                #hero { color: #00ff00; }
                "#,
            )
            .unwrap(),
        );

        let style = engine.compute_style(p, None);
        assert_eq!(style.color, Color::rgb(0, 255, 0));
    }

    #[test]
    fn inline_style_wins_over_author_rules() {
        let arena = DomArena::new();
        let document = parse_html(
            &arena,
            r#"<p class="muted" style="color: #123456">Hello</p>"#,
        )
        .unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(Stylesheet::parse(".muted { color: red; }").unwrap());

        let style = engine.compute_style(p, None);
        assert_eq!(style.color, Color::rgb(0x12, 0x34, 0x56));
    }

    #[test]
    fn descendant_selector_matches_ancestors() {
        let arena = DomArena::new();
        let document =
            parse_html(&arena, r#"<main><p><strong>Secure</strong></p></main>"#).unwrap();
        let strong = document.first_element_by_tag("strong").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(Stylesheet::parse("main strong { color: blue; }").unwrap());

        let style = engine.compute_style(strong, None);
        assert_eq!(style.color, Color::rgb(0, 0, 255));
    }

    #[test]
    fn inherited_values_flow_from_parent_style() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<p><span>Child</span></p>"#).unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let span = document.first_element_by_tag("span").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(Stylesheet::parse("p { color: red; font-size: 20px; }").unwrap());
        let parent_style = engine.compute_style(p, None);
        let child_style = engine.compute_style(span, Some(&parent_style));

        assert_eq!(child_style.color, Color::rgb(255, 0, 0));
        assert_eq!(child_style.font_size, Length::Px(20.0));
    }

    #[test]
    fn parses_multi_value_spacing_shorthand() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<main class="card">Content</main>"#).unwrap();
        let main = document.first_element_by_tag("main").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(
            Stylesheet::parse(".card { margin: 1px 2px 3px 4px; padding: 8px 12px; }").unwrap(),
        );

        let style = engine.compute_style(main, None);
        assert_eq!(style.margin.top, Length::Px(1.0));
        assert_eq!(style.margin.right, Length::Px(2.0));
        assert_eq!(style.margin.bottom, Length::Px(3.0));
        assert_eq!(style.margin.left, Length::Px(4.0));
        assert_eq!(style.padding.top, Length::Px(8.0));
        assert_eq!(style.padding.left, Length::Px(12.0));
    }

    #[test]
    fn parses_display_flex() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<nav class="row"></nav>"#).unwrap();
        let nav = document.first_element_by_tag("nav").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(Stylesheet::parse(".row { display: flex; }").unwrap());

        let style = engine.compute_style(nav, None);
        assert_eq!(style.display, Display::Flex);
    }

    #[test]
    fn parses_rgb_and_rgba_colors() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<p class="color">Color</p>"#).unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(
            Stylesheet::parse(
                ".color { color: rgb(12, 24, 36); background-color: rgba(10 20 30 / 50%); }",
            )
            .unwrap(),
        );

        let style = engine.compute_style(p, None);
        assert_eq!(style.color, Color::rgb(12, 24, 36));
        assert_eq!(
            style.background_color,
            Color {
                r: 10,
                g: 20,
                b: 30,
                a: 128
            }
        );
    }
}
