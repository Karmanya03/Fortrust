pub mod animation;

use std::collections::HashMap;
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
pub enum FlexDirection {
    Row,
    Column,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub inset: bool,
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
}

// ── CSS Transform types ─────────────────────────────────────────────────────

/// A single CSS transform function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransformFunction {
    /// `translateX(px)`
    TranslateX(f32),
    /// `translateY(px)`
    TranslateY(f32),
    /// `translate(x, y)`
    Translate(f32, f32),
    /// `rotate(deg)`
    Rotate(f32),
    /// `scaleX(factor)`
    ScaleX(f32),
    /// `scaleY(factor)`
    ScaleY(f32),
    /// `scale(x, y)`
    Scale(f32, f32),
    /// `skewX(deg)`
    SkewX(f32),
    /// `skewY(deg)`
    SkewY(f32),
    /// `matrix(a, b, c, d, tx, ty)` — full 2D affine transform
    Matrix(f32, f32, f32, f32, f32, f32),
}

impl TransformFunction {
    /// Decompose this transform into a 2D affine matrix [a, b, c, d, tx, ty].
    /// The matrix maps (x,y) → (ax + cy + tx, bx + dy + ty).
    pub fn to_matrix(&self) -> [f32; 6] {
        match self {
            Self::TranslateX(tx) => [1.0, 0.0, 0.0, 1.0, *tx, 0.0],
            Self::TranslateY(ty) => [1.0, 0.0, 0.0, 1.0, 0.0, *ty],
            Self::Translate(tx, ty) => [1.0, 0.0, 0.0, 1.0, *tx, *ty],
            Self::Rotate(deg) => {
                let rad = deg.to_radians();
                let cos = rad.cos();
                let sin = rad.sin();
                [cos, sin, -sin, cos, 0.0, 0.0]
            }
            Self::ScaleX(sx) => [*sx, 0.0, 0.0, 1.0, 0.0, 0.0],
            Self::ScaleY(sy) => [1.0, 0.0, 0.0, *sy, 0.0, 0.0],
            Self::Scale(sx, sy) => [*sx, 0.0, 0.0, *sy, 0.0, 0.0],
            Self::SkewX(deg) => {
                let tan = deg.to_radians().tan();
                [1.0, 0.0, tan, 1.0, 0.0, 0.0]
            }
            Self::SkewY(deg) => {
                let tan = deg.to_radians().tan();
                [1.0, tan, 0.0, 1.0, 0.0, 0.0]
            }
            Self::Matrix(a, b, c, d, tx, ty) => [*a, *b, *c, *d, *tx, *ty],
        }
    }
}

/// A list of CSS transform functions (applied left-to-right).
#[derive(Debug, Clone, PartialEq)]
pub struct CssTransform {
    pub functions: Vec<TransformFunction>,
}

impl CssTransform {
    pub fn none() -> Self {
        Self { functions: Vec::new() }
    }

    pub fn is_none(&self) -> bool {
        self.functions.is_empty()
    }

    /// Compute the combined 2D affine matrix by multiplying all transform
    /// functions in order. Returns the identity matrix if the list is empty.
    pub fn combined_matrix(&self) -> [f32; 6] {
        let mut result = [1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0]; // identity
        for func in &self.functions {
            let m = func.to_matrix();
            result = multiply_matrices(result, m);
        }
        result
    }
}

/// Multiply two 2D affine matrices: result = a * b.
fn multiply_matrices(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[2] * b[1],     // result.a
        a[1] * b[0] + a[3] * b[1],     // result.b
        a[0] * b[2] + a[2] * b[3],     // result.c
        a[1] * b[2] + a[3] * b[3],     // result.d
        a[0] * b[4] + a[2] * b[5] + a[4], // result.tx
        a[1] * b[4] + a[3] * b[5] + a[5], // result.ty
    ]
}

/// Parse a CSS `transform` value string into a `CssTransform`.
pub(crate) fn parse_css_transform(value: &str) -> Option<CssTransform> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") || value.is_empty() {
        return Some(CssTransform::none());
    }

    let mut functions = Vec::new();
    let mut remaining = value;

    while !remaining.is_empty() {
        remaining = remaining.trim_start();
        if remaining.is_empty() { break; }

        if let Some(rest) = remaining.strip_prefix("translateX(") {
            let (val, rest) = extract_paren_value(rest)?;
            functions.push(TransformFunction::TranslateX(parse_px_value(&val)?));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("translateY(") {
            let (val, rest) = extract_paren_value(rest)?;
            functions.push(TransformFunction::TranslateY(parse_px_value(&val)?));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("translate(") {
            let (val, rest) = extract_paren_value(rest)?;
            let parts: Vec<&str> = val.split(|c: char| c == ',' || c.is_whitespace())
                .map(str::trim).filter(|s| !s.is_empty()).collect();
            let x = parse_px_value(parts.first()?)?;
            let y = parts.get(1).and_then(|s| parse_px_value(s)).unwrap_or(0.0);
            functions.push(TransformFunction::Translate(x, y));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("rotate(") {
            let (val, rest) = extract_paren_value(rest)?;
            functions.push(TransformFunction::Rotate(parse_angle_value(&val)?));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("scaleX(") {
            let (val, rest) = extract_paren_value(rest)?;
            functions.push(TransformFunction::ScaleX(val.trim().parse::<f32>().ok()?));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("scaleY(") {
            let (val, rest) = extract_paren_value(rest)?;
            functions.push(TransformFunction::ScaleY(val.trim().parse::<f32>().ok()?));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("scale(") {
            let (val, rest) = extract_paren_value(rest)?;
            let parts: Vec<&str> = val.split(|c: char| c == ',' || c.is_whitespace())
                .map(str::trim).filter(|s| !s.is_empty()).collect();
            let x = parts.first()?.trim().parse::<f32>().ok()?;
            let y = parts.get(1).and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(x);
            functions.push(TransformFunction::Scale(x, y));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("skewX(") {
            let (val, rest) = extract_paren_value(rest)?;
            functions.push(TransformFunction::SkewX(parse_angle_value(&val)?));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("skewY(") {
            let (val, rest) = extract_paren_value(rest)?;
            functions.push(TransformFunction::SkewY(parse_angle_value(&val)?));
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("matrix(") {
            let (val, rest) = extract_paren_value(rest)?;
            let nums: Vec<f32> = val.split(',')
                .filter_map(|s| s.trim().parse::<f32>().ok())
                .collect();
            if nums.len() == 6 {
                functions.push(TransformFunction::Matrix(nums[0], nums[1], nums[2], nums[3], nums[4], nums[5]));
            }
            remaining = rest;
        } else {
            // Skip unknown function
            if let Some(paren) = remaining.find('(') {
                if let Some(close) = remaining[paren..].find(')') {
                    remaining = &remaining[paren + close + 1..];
                } else { break; }
            } else { break; }
        }
    }

    if functions.is_empty() { return None; }
    Some(CssTransform { functions })
}

fn extract_paren_value(s: &str) -> Option<(String, &str)> {
    let close = s.find(')')?;
    Some((s[..close].to_owned(), &s[close + 1..]))
}

fn parse_px_value(s: &str) -> Option<f32> {
    let s = s.trim().to_lowercase();
    let s = s.strip_suffix("px").unwrap_or(&s);
    s.trim().parse::<f32>().ok()
}

fn parse_angle_value(s: &str) -> Option<f32> {
    let s = s.trim().to_lowercase();
    if let Some(stripped) = s.strip_suffix("deg") {
        stripped.trim().parse::<f32>().ok()
    } else if let Some(stripped) = s.strip_suffix("turn") {
        stripped.trim().parse::<f32>().ok().map(|v| v * 360.0)
    } else if let Some(stripped) = s.strip_suffix("rad") {
        stripped.trim().parse::<f32>().ok().map(|v| v * 180.0 / std::f32::consts::PI)
    } else {
        s.parse::<f32>().ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineStyle {
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlineSizes {
    pub width: f32,
    pub style: OutlineStyle,
    pub color: Color,
}

impl OutlineSizes {
    pub fn none() -> Self {
        Self { width: 0.0, style: OutlineStyle::None, color: Color::TRANSPARENT }
    }
}

// ── @font-face types ─────────────────────────────────────────────────────────

/// How a font-face should behave while loading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum FontDisplay {
    #[default]
    Auto,
    Block,
    Swap,
    Fallback,
    Optional,
}


/// A single source entry in `src:` of an @font-face rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFaceSource {
    /// The URL of the font file.
    pub url: String,
    /// Optional format hint (e.g. "woff2", "woff", "truetype", "opentype").
    pub format: Option<String>,
}

/// A parsed `@font-face` rule.
#[derive(Debug, Clone, PartialEq)]
pub struct FontFaceRule {
    /// The font-family name declared in this rule.
    pub family: String,
    /// One or more font sources (tried in order).
    pub sources: Vec<FontFaceSource>,
    /// Font weight (defaults to Normal).
    pub weight: FontWeight,
    /// Font style (defaults to Normal).
    pub style: FontStyle,
    /// Font-display strategy.
    pub display: FontDisplay,
    /// Optional unicode-range (stored as raw string for now).
    pub unicode_range: Option<String>,
}

// ── Animation / Transition types ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
#[derive(Default)]
pub enum EasingFunction {
    Linear,
    #[default]
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    CubicBezier(f32, f32, f32, f32),
    StepStart,
    StepEnd,
    Steps(i32),
}


impl EasingFunction {
    pub fn apply(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
            Self::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, t),
            Self::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, t),
            Self::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
            Self::CubicBezier(x1, y1, x2, y2) => cubic_bezier(*x1, *y1, *x2, *y2, t),
            Self::StepStart => if t < 1.0 { 0.0 } else { 1.0 },
            Self::StepEnd => if t > 0.0 { 1.0 } else { 0.0 },
            Self::Steps(n) => {
                if *n <= 1 { return t; }
                let step = 1.0 / *n as f32;
                (t / step).floor() * step
            }
        }
    }
}

fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let one_minus_t = 1.0 - t;
    let x = 3.0 * one_minus_t * one_minus_t * t * x1
        + 3.0 * one_minus_t * t * t * x2
        + t * t * t;
    let y = 3.0 * one_minus_t * one_minus_t * t * y1
        + 3.0 * one_minus_t * t * t * y2
        + t * t * t;
    if x == 0.0 { return y; }
    y / x
}

#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub enum AnimationDirection {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum AnimationFillMode {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}


#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub enum AnimationPlayState {
    #[default]
    Running,
    Paused,
}


#[derive(Debug, Clone, PartialEq)]
pub struct SingleAnimation {
    pub name: String,
    pub duration: f32,
    pub timing_function: EasingFunction,
    pub delay: f32,
    pub iteration_count: f32,
    pub direction: AnimationDirection,
    pub fill_mode: AnimationFillMode,
    pub play_state: AnimationPlayState,
}

impl Default for SingleAnimation {
    fn default() -> Self {
        Self {
            name: String::new(),
            duration: 0.0,
            timing_function: EasingFunction::default(),
            delay: 0.0,
            iteration_count: 1.0,
            direction: AnimationDirection::default(),
            fill_mode: AnimationFillMode::default(),
            play_state: AnimationPlayState::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SingleTransition {
    pub property: String,
    pub duration: f32,
    pub timing_function: EasingFunction,
    pub delay: f32,
}

impl Default for SingleTransition {
    fn default() -> Self {
        Self {
            property: String::new(),
            duration: 0.0,
            timing_function: EasingFunction::default(),
            delay: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe {
    pub offset: f32,
    pub declarations: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyframesRule {
    pub name: String,
    pub keyframes: Vec<Keyframe>,
}

// ── Media Query types ─────────────────────────────────────────────────────

/// A single media feature condition (e.g. `min-width: 768px`).
#[derive(Debug, Clone, PartialEq)]
pub enum MediaFeature {
    MinWidth(f32),
    MaxWidth(f32),
    MinHeight(f32),
    MaxHeight(f32),
    PrefersColorScheme(ColorScheme),
    PrefersReducedMotion(ReducedMotion),
    Orientation(OrientationValue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorScheme { Light, Dark }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReducedMotion { Reduce, NoPreference }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrientationValue { Portrait, Landscape }

/// A media query: an optional media type + a list of feature conditions.
/// All conditions must match (AND logic) for the query to be true.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaQuery {
    /// If true, the query is negated (`@media not ...`).
    pub negated: bool,
    /// Media type: "all", "screen", "print", etc.
    pub media_type: String,
    /// Feature conditions (all must match).
    pub features: Vec<MediaFeature>,
}

/// Environment context for evaluating media queries.
#[derive(Debug, Clone)]
pub struct MediaContext {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub color_scheme: ColorScheme,
    pub reduced_motion: ReducedMotion,
}

impl Default for MediaContext {
    fn default() -> Self {
        Self {
            viewport_width: 1280.0,
            viewport_height: 720.0,
            color_scheme: ColorScheme::Light,
            reduced_motion: ReducedMotion::NoPreference,
        }
    }
}

impl MediaContext {
    pub fn evaluate(&self, query: &MediaQuery) -> bool {
        // Check media type
        let type_matches = query.media_type == "all"
            || query.media_type == "screen";

        // Check all features (AND logic)
        let features_match = query.features.iter().all(|f| self.matches_feature(f));

        let result = type_matches && features_match;
        if query.negated { !result } else { result }
    }

    fn matches_feature(&self, feature: &MediaFeature) -> bool {
        match feature {
            MediaFeature::MinWidth(w) => self.viewport_width >= *w,
            MediaFeature::MaxWidth(w) => self.viewport_width <= *w,
            MediaFeature::MinHeight(h) => self.viewport_height >= *h,
            MediaFeature::MaxHeight(h) => self.viewport_height <= *h,
            MediaFeature::PrefersColorScheme(scheme) => self.color_scheme == *scheme,
            MediaFeature::PrefersReducedMotion(motion) => self.reduced_motion == *motion,
            MediaFeature::Orientation(orient) => {
                let actual = if self.viewport_height > self.viewport_width {
                    OrientationValue::Portrait
                } else {
                    OrientationValue::Landscape
                };
                actual == *orient
            }
        }
    }
}

/// A group of rules gated behind a media query.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaRule {
    pub query: MediaQuery,
    pub rules: Vec<Rule>,
}

/// Parse a `@media` condition string into a `MediaQuery`.
pub(crate) fn parse_media_query(input: &str) -> Option<MediaQuery> {
    let input = input.trim();
    let mut negated = false;
    let mut remaining = input;

    // Handle `not` prefix
    if let Some(rest) = remaining.strip_prefix("not ") {
        negated = true;
        remaining = rest.trim();
    }

    // Parse media type
    let (media_type, features_str) = if remaining.contains('(') {
        let paren_pos = remaining.find('(')?;
        let before = remaining[..paren_pos].trim();
        let media_type = if before.is_empty() || before == "and" {
            "all".to_owned()
        } else {
            before.split_whitespace().next().unwrap_or("all").to_owned()
        };
        (media_type, &remaining[paren_pos..])
    } else {
        (remaining.trim().to_owned(), "")
    };

    // Parse features
    let mut features = Vec::new();
    let mut feat_remaining = features_str;
    while let Some(open) = feat_remaining.find('(') {
        let close = feat_remaining[open..].find(')')? + open;
        let feat_body = &feat_remaining[open + 1..close];
        if let Some(feat) = parse_media_feature(feat_body.trim()) {
            features.push(feat);
        }
        feat_remaining = &feat_remaining[close + 1..];
    }

    Some(MediaQuery { negated, media_type, features })
}

fn parse_media_feature(input: &str) -> Option<MediaFeature> {
    let input = input.trim();
    if let Some((prop, val)) = input.split_once(':') {
        let prop = prop.trim().to_lowercase();
        let val = val.trim();
        match prop.as_str() {
            "min-width" => parse_px_media(val).map(MediaFeature::MinWidth),
            "max-width" => parse_px_media(val).map(MediaFeature::MaxWidth),
            "min-height" => parse_px_media(val).map(MediaFeature::MinHeight),
            "max-height" => parse_px_media(val).map(MediaFeature::MaxHeight),
            "prefers-color-scheme" => match val.to_lowercase().as_str() {
                "dark" => Some(MediaFeature::PrefersColorScheme(ColorScheme::Dark)),
                "light" => Some(MediaFeature::PrefersColorScheme(ColorScheme::Light)),
                _ => None,
            },
            "prefers-reduced-motion" => match val.to_lowercase().as_str() {
                "reduce" => Some(MediaFeature::PrefersReducedMotion(ReducedMotion::Reduce)),
                "no-preference" => Some(MediaFeature::PrefersReducedMotion(ReducedMotion::NoPreference)),
                _ => None,
            },
            "orientation" => match val.to_lowercase().as_str() {
                "portrait" => Some(MediaFeature::Orientation(OrientationValue::Portrait)),
                "landscape" => Some(MediaFeature::Orientation(OrientationValue::Landscape)),
                _ => None,
            },
            _ => None,
        }
    } else {
        // Bare feature name (boolean)
        match input.to_lowercase().as_str() {
            "portrait" => Some(MediaFeature::Orientation(OrientationValue::Portrait)),
            "landscape" => Some(MediaFeature::Orientation(OrientationValue::Landscape)),
            _ => None,
        }
    }
}

fn parse_px_media(val: &str) -> Option<f32> {
    let val = val.trim().to_lowercase();
    let val = val.strip_suffix("px").unwrap_or(&val);
    val.trim().parse::<f32>().ok()
}

// ── ComputedStyle ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub position: Position,
    pub color: Color,
    pub background_color: Color,
    pub font_family: Vec<String>,
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
    pub flex_direction: FlexDirection,
    pub order: i32,
    pub gap: Length,
    pub row_gap: Length,
    pub column_gap: Length,
    pub box_shadow: Option<BoxShadow>,
    pub outline: OutlineSizes,
    pub transform: CssTransform,
    pub animations: Vec<SingleAnimation>,
    pub transitions: Vec<SingleTransition>,
    /// CSS custom properties (--variable-name: value). Inherited by default.
    pub custom_properties: HashMap<String, String>,
}

impl ComputedStyle {
    pub fn initial() -> Self {
        Self {
            display: Display::Inline,
            position: Position::Static,
            color: Color::BLACK,
            background_color: Color::TRANSPARENT,
            font_family: vec!["sans-serif".to_string()],
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
            flex_direction: FlexDirection::Row,
            order: 0,
            gap: Length::Px(0.0),
            row_gap: Length::Px(0.0),
            column_gap: Length::Px(0.0),
            box_shadow: None,
            outline: OutlineSizes::none(),
            transform: CssTransform::none(),
            animations: Vec::new(),
            transitions: Vec::new(),
            custom_properties: HashMap::new(),
        }
    }

    pub fn inherit_from(parent: Option<&Self>) -> Self {
        let mut style = Self::initial();
        if let Some(parent) = parent {
            style.color = parent.color;
            style.font_family = parent.font_family.clone();
            style.font_size = parent.font_size;
            style.font_weight = parent.font_weight;
            style.font_style = parent.font_style;
            style.text_align = parent.text_align;
            style.white_space = parent.white_space;
            style.visibility = parent.visibility;
            style.line_height = parent.line_height;
            style.letter_spacing = parent.letter_spacing;
            style.word_spacing = parent.word_spacing;
            // Custom properties inherit by default
            style.custom_properties = parent.custom_properties.clone();
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
    FontFamily(Vec<String>),
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
    FlexDirection(FlexDirection),
    Order(i32),
    String(String),
    BoxShadow(BoxShadow),
    Outline(OutlineSizes),
    Transform(CssTransform),
    Animation(Vec<SingleAnimation>),
    Transition(Vec<SingleTransition>),
    // Individual animation sub-properties (for non-shorthand usage)
    AnimationNames(Vec<String>),
    AnimationDurations(Vec<f32>),
    AnimationEasings(Vec<EasingFunction>),
    AnimationDelays(Vec<f32>),
    AnimationIterationCounts(Vec<f32>),
    AnimationDirections(Vec<AnimationDirection>),
    AnimationFillModes(Vec<AnimationFillMode>),
    AnimationPlayStates(Vec<AnimationPlayState>),
    // Individual transition sub-properties
    TransitionProperties(Vec<String>),
    TransitionDurations(Vec<f32>),
    TransitionEasings(Vec<EasingFunction>),
    TransitionDelays(Vec<f32>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    parts: SmallVec<[(SimpleSelector, Combinator); 3]>,
    specificity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SimpleSelector {
    tag: Option<CompactString>,
    id: Option<CompactString>,
    classes: SmallVec<[CompactString; 3]>,
    pseudo_class: Option<CompactString>,
    attributes: SmallVec<[AttrSelector; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttrSelector {
    name: CompactString,
    operator: AttrOp,
    value: Option<CompactString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrOp {
    Exists,
    Equals,
    Contains,
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
    pub keyframes: Vec<KeyframesRule>,
    pub font_face_rules: Vec<FontFaceRule>,
    pub media_rules: Vec<MediaRule>,
}

fn parse_keyframe_offset(selector: &str) -> f32 {
    let s = selector.trim();
    if s.eq_ignore_ascii_case("from") || s == "0%" { 0.0 }
    else if s.eq_ignore_ascii_case("to") || s == "100%" { 1.0 }
    else {
        s.trim_end_matches('%').trim().parse::<f32>().unwrap_or(0.0) / 100.0
    }
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
        let mut keyframes = Vec::new();
        let mut font_face_rules = Vec::new();
        let mut media_rules = Vec::new();
        let mut rest = input;
        'outer: while !rest.is_empty() {
            let trimmed = rest.trim_start();

            // ── @font-face ──
            if let Some(after_font_face) = trimmed.strip_prefix("@font-face") {
                let after_at = after_font_face.trim_start();
                if let Some(open) = after_at.find('{') {
                    let body_start = open + 1;
                    let rest_after = &after_at[body_start..];
                    if let Some(close) = rest_after.find('}') {
                        let body = &rest_after[..close];
                        if let Some(rule) = parse_font_face_body(body) {
                            font_face_rules.push(rule);
                        }
                        // Advance past the entire @font-face {...} block
                        let consumed_from_trimmed = "@font-face".len()
                            + (after_at.as_ptr() as usize - trimmed["@font-face".len()..].as_ptr() as usize)
                            + body_start + close + 1;
                        let consumed = (rest.len() - trimmed.len()) + consumed_from_trimmed;
                        rest = &rest[consumed..];
                        continue;
                    }
                }
            }

            if trimmed.starts_with("@keyframes") || trimmed.starts_with("@-webkit-keyframes") {
                let after_at = if let Some(rest) = trimmed.strip_prefix("@-webkit-keyframes") {
                    rest
                } else if let Some(rest) = trimmed.strip_prefix("@keyframes") {
                    rest
                } else {
                    continue;
                };
                let after_trimmed = after_at.trim_start();
                let name_end = after_trimmed.find(|c: char| c.is_whitespace() || c == '{').unwrap_or(after_trimmed.len());
                let name = after_trimmed[..name_end].trim().to_owned();
                let after_name = after_trimmed[name_end..].trim_start();
                if !name.is_empty()
                    && let Some(open) = after_name.find('{') {
                        let body_start = open + 1;
                        let rest_after = &after_name[body_start..];
                        let rest_after_len = rest_after.len();
                        let mut depth = 1u32;
                        let mut close = 0;
                        for (i, ch) in rest_after.char_indices() {
                            match ch {
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        close = i;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        let keyframe_body = &rest_after[..close];
                        let mut kfs = Vec::new();
                        let mut kp = 0;
                        while kp < keyframe_body.len() {
                            let ktrim = keyframe_body[kp..].trim_start();
                            let koff = keyframe_body.len() - ktrim.len();
                            if let Some(ko) = ktrim.find('{') {
                                let ksel = ktrim[..ko].trim();
                                let kbody = &ktrim[ko + 1..];
                                if let Some(kc) = kbody.find('}') {
                                    let decls_raw = &kbody[..kc];
                                    let offset_val = parse_keyframe_offset(ksel);
                                    let mut pairs = Vec::new();
                                    for line in decls_raw.split(';') {
                                        let line = line.trim();
                                        if line.is_empty() { continue; }
                                        if let Some(colon) = line.find(':') {
                                            let prop = line[..colon].trim().to_owned();
                                            let val = line[colon + 1..].trim().to_owned();
                                            if !prop.is_empty() && !val.is_empty() {
                                                pairs.push((prop, val));
                                            }
                                        }
                                    }
                                    kfs.push(Keyframe { offset: offset_val, declarations: pairs });
                                    kp += koff + ko + 1 + kc + 1;
                                } else { break; }
                            } else { break; }
                        }
                        keyframes.push(KeyframesRule { name, keyframes: kfs });
                        // consumed = everything from start of rest through the closing '}'
                        let consumed = rest.len() - rest_after_len + close + 1;
                        rest = &rest[consumed..];
                        continue;
                    }
            }

            // ── @media ──
            if let Some(after_media) = trimmed.strip_prefix("@media") {
                let after_at = after_media.trim_start();
                // Find the opening brace of the media block
                if let Some(open) = after_at.find('{') {
                    let query_str = after_at[..open].trim();
                    let body_start = open + 1;
                    let rest_after = &after_at[body_start..];
                    // Find matching closing brace (handle nested braces)
                    let mut depth = 1u32;
                    let mut close = 0;
                    for (i, ch) in rest_after.char_indices() {
                        match ch {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 { close = i; break; }
                            }
                            _ => {}
                        }
                    }
                    let media_body = &rest_after[..close];
                    // Parse the inner rules as a sub-stylesheet
                    if let Some(query) = parse_media_query(query_str) {
                        let inner_rules = parse_inner_rules(media_body, rules.len());
                        media_rules.push(MediaRule { query, rules: inner_rules });
                    }
                    let consumed = rest.len() - rest_after.len() + close + 1;
                    rest = &rest[consumed..];
                    continue;
                }
            }

            if let Some(open) = trimmed.find('{') {
                let selector_text = &trimmed[..open];
                let after_open = &trimmed[open + 1..];
                let Some(close) = after_open.find('}') else {
                    return Err(StyleError::UnclosedRule);
                };
                let body = &after_open[..close];
                let selectors = parse_selectors(selector_text);
                let declarations = parse_declarations(body);
                if !selectors.is_empty() && !declarations.is_empty() {
                    rules.push(Rule { selectors, declarations, order: rules.len() });
                }
                rest = &after_open[close + 1..];
            } else {
                break 'outer;
            }
        }

        Ok(Self { rules, keyframes, font_face_rules, media_rules })
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
    pub keyframes: Vec<KeyframesRule>,
    pub media_context: MediaContext,
}

impl StyleEngine {
    pub fn new() -> Self {
        Self {
            stylesheets: vec![Stylesheet::ua_defaults()],
            keyframes: Vec::new(),
            media_context: MediaContext::default(),
        }
    }

    pub fn with_media_context(mut self, ctx: MediaContext) -> Self {
        self.media_context = ctx;
        self
    }

    pub fn set_media_context(&mut self, ctx: MediaContext) {
        self.media_context = ctx;
    }

    pub fn add_stylesheet(&mut self, stylesheet: Stylesheet) {
        for kf in &stylesheet.keyframes {
            if !self.keyframes.iter().any(|k| k.name == kf.name) {
                self.keyframes.push(kf.clone());
            }
        }
        self.stylesheets.push(stylesheet);
    }

    pub fn get_keyframes(&self, name: &str) -> Option<&KeyframesRule> {
        self.keyframes.iter().find(|k| k.name == name)
    }

    /// Return all @font-face rules from all added stylesheets.
    pub fn font_face_rules(&self) -> Vec<&FontFaceRule> {
        self.stylesheets.iter().flat_map(|s| s.font_face_rules.iter()).collect()
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
            // Also include rules from matching @media queries
            for media_rule in &stylesheet.media_rules {
                if self.media_context.evaluate(&media_rule.query) {
                    for rule in &media_rule.rules {
                        for selector in &rule.selectors {
                            if selector.matches(node) {
                                matched.push((selector.specificity, rule.order, &rule.declarations));
                            }
                        }
                    }
                }
            }
        }

        matched.sort_by_key(|(specificity, order, _)| (*specificity, *order));

        // First pass: collect all custom property definitions
        for (_, _, declarations) in &matched {
            for decl in declarations.iter() {
                if decl.property.starts_with("--")
                    && let PropertyValue::String(val) = &decl.value {
                        let resolved = resolve_var_references(val, &style.custom_properties);
                        style.custom_properties.insert(decl.property.to_string(), resolved);
                    }
            }
        }

        // Second pass: apply regular declarations with var() resolution
        for (_, _, declarations) in &matched {
            apply_declarations(&mut style, declarations);
        }

        // Handle inline style attribute
        if let Some(inline) = element.attr("style") {
            let declarations = parse_declarations(&inline);
            // Collect inline custom properties first
            for decl in &declarations {
                if decl.property.starts_with("--")
                    && let PropertyValue::String(val) = &decl.value {
                        let resolved = resolve_var_references(val, &style.custom_properties);
                        style.custom_properties.insert(decl.property.to_string(), resolved);
                    }
            }
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
        let mut parts: SmallVec<[(SimpleSelector, Combinator); 3]> = SmallVec::new();

        // Split on whitespace, but handle `>` combinator properly
        let mut remaining = input.trim();
        let mut combinator = Combinator::Descendant;
        while !remaining.is_empty() {
            // Skip leading combinators
            if remaining.starts_with('>') {
                combinator = Combinator::Child;
                remaining = remaining[1..].trim_start();
                continue;
            }
            // Extract the next simple selector (up to whitespace, >, or end)
            let end = remaining
                .find(|c: char| c.is_whitespace() || c == '>')
                .unwrap_or(remaining.len());
            let raw = &remaining[..end];
            remaining = remaining[end..].trim_start();
            if raw.is_empty() {
                continue;
            }
            let sel = SimpleSelector::parse(raw)?;
            parts.push((sel, combinator));
            combinator = Combinator::Descendant;
        }

        if parts.is_empty() {
            return None;
        }
        let specificity = parts.iter().map(|(s, _)| s.specificity()).sum();
        Some(Self { parts, specificity })
    }

    fn matches<'arena>(&self, node: NodeRef<'arena>) -> bool {
        let mut parts_iter = self.parts.iter().rev();
        let Some((rightmost, _)) = parts_iter.next() else {
            return false;
        };
        if !rightmost.matches(node) {
            return false;
        }

        let mut current = node.parent();
        for (selector, combinator) in parts_iter {
            let found = if *combinator == Combinator::Child {
                // Direct parent only
                current.and_then(|n| if selector.matches(n) { Some(n) } else { None })
            } else {
                find_matching_ancestor(current, selector)
            };
            match found {
                Some(f) => current = f.parent(),
                None => return false,
            }
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
                attributes: SmallVec::new(),
            });
        }

        // Handle attribute selectors [attr], [attr=value], [attr~=value]
        let mut attributes: SmallVec<[AttrSelector; 2]> = SmallVec::new();
        let mut base = raw;
        while let Some(bracket_start) = base.find('[') {
            let _tag_part = &base[..bracket_start];
            let rest = &base[bracket_start..];
            if let Some(bracket_end) = rest.find(']') {
                let attr_raw = &rest[1..bracket_end];
                base = &rest[bracket_end + 1..];
                let attr = if let Some(eq_pos) = attr_raw.find('=') {
                    let name = attr_raw[..eq_pos].trim();
                    if name.ends_with('~') {
                        let actual_name = name[..name.len() - 1].trim();
                        let val = attr_raw[eq_pos + 1..].trim().trim_matches('"').trim_matches('\'');
                        AttrSelector {
                            name: CompactString::from(actual_name),
                            operator: AttrOp::Contains,
                            value: Some(CompactString::from(val)),
                        }
                    } else {
                        let val = attr_raw[eq_pos + 1..].trim().trim_matches('"').trim_matches('\'');
                        AttrSelector {
                            name: CompactString::from(name),
                            operator: AttrOp::Equals,
                            value: Some(CompactString::from(val)),
                        }
                    }
                } else {
                    AttrSelector {
                        name: CompactString::from(attr_raw.trim()),
                        operator: AttrOp::Exists,
                        value: None,
                    }
                };
                attributes.push(attr);
            } else {
                break;
            }
        }

        let raw = base;
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
            attributes,
        })
    }

    fn specificity(&self) -> u32 {
        let id = u32::from(self.id.is_some());
        let class = self.classes.len() as u32
            + self.attributes.len() as u32
            + u32::from(self.pseudo_class.is_some());
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

        // Attribute selectors: [attr], [attr=value], [attr~=value]
        for attr_sel in &self.attributes {
            let actual = element.attr(&attr_sel.name);
            match attr_sel.operator {
                AttrOp::Exists => {
                    if actual.is_none() { return false; }
                }
                AttrOp::Equals => {
                    if actual.as_deref() != attr_sel.value.as_deref() { return false; }
                }
                AttrOp::Contains => {
                    if let Some(ref val) = attr_sel.value {
                        let has = actual
                            .map(|a| a.split_whitespace().any(|part| part == val.as_str()))
                            .unwrap_or(false);
                        if !has { return false; }
                    }
                }
            }
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
                pseudo if pseudo.starts_with("nth-child(") => {
                    if let Some(parent) = node.parent() {
                        let children = parent.children();
                        let idx = children.iter().position(|c| std::ptr::addr_eq(c, node)).unwrap_or(0) + 1;
                        if pseudo == "nth-child(even)" {
                            if idx % 2 != 0 { return false; }
                        } else if pseudo == "nth-child(odd)" {
                            if idx % 2 == 0 { return false; }
                        } else if let Some(n_str) = pseudo.strip_prefix("nth-child(").and_then(|s| s.strip_suffix(")"))
                            && let Ok(n) = n_str.parse::<usize>()
                                && idx != n { return false; }
                    } else {
                        return false;
                    }
                }
                "nth-of-type" | "first-of-type" | "last-of-type" => {
                    // Stubbed for static rendering
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

/// Parse a block of CSS rules (used inside @media blocks).
fn parse_inner_rules(body: &str, start_order: usize) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let trimmed = rest.trim_start();
        if trimmed.is_empty() { break; }
        if let Some(open) = trimmed.find('{') {
            let selector_text = &trimmed[..open];
            let after_open = &trimmed[open + 1..];
            let Some(close) = after_open.find('}') else { break };
            let decl_body = &after_open[..close];
            let selectors = parse_selectors(selector_text);
            let declarations = parse_declarations(decl_body);
            if !selectors.is_empty() && !declarations.is_empty() {
                rules.push(Rule {
                    selectors,
                    declarations,
                    order: start_order + rules.len(),
                });
            }
            rest = &after_open[close + 1..];
        } else {
            break;
        }
    }
    rules
}

fn parse_declarations(input: &str) -> SmallVec<[Declaration; 6]> {
    input
        .split(';')
        .filter_map(|chunk| {
            let (property, raw_value) = chunk.split_once(':')?;
            let property = property.trim();
            let raw_trimmed = raw_value.trim();
            // Custom properties (--*) are stored as raw strings
            if property.starts_with("--") {
                return Some(Declaration {
                    property: CompactString::from(property.to_ascii_lowercase()),
                    value: PropertyValue::String(raw_trimmed.to_owned()),
                });
            }
            let property = property.to_ascii_lowercase();
            // If the value contains var(), store as raw String for later resolution
            if raw_trimmed.contains("var(") {
                return Some(Declaration {
                    property: CompactString::from(property),
                    value: PropertyValue::String(raw_trimmed.to_owned()),
                });
            }
            let value = parse_property_value(&property, raw_trimmed)?;
            Some(Declaration {
                property: CompactString::from(property),
                value,
            })
        })
        .collect()
}

// ── Animation / Transition parsing helpers ─────────────────────────────────

fn parse_css_time(value: &str) -> Option<f32> {
    let value = value.trim();
    if let Some(rest) = value.strip_suffix("ms") {
        rest.trim().parse::<f32>().ok().map(|v| v / 1000.0)
    } else if let Some(rest) = value.strip_suffix('s') {
        rest.trim().parse::<f32>().ok()
    } else {
        // bare number without a unit — treat as seconds? treat as 0?
        None
    }
}

fn parse_easing_function(value: &str) -> Option<EasingFunction> {
    let lowered = value.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "linear" => Some(EasingFunction::Linear),
        "ease" => Some(EasingFunction::Ease),
        "ease-in" => Some(EasingFunction::EaseIn),
        "ease-out" => Some(EasingFunction::EaseOut),
        "ease-in-out" => Some(EasingFunction::EaseInOut),
        "step-start" => Some(EasingFunction::StepStart),
        "step-end" => Some(EasingFunction::StepEnd),
        _ => {
            if lowered.starts_with("cubic-bezier(") {
                let inner = lowered.trim_start_matches("cubic-bezier(").trim_end_matches(')');
                let nums: Vec<f32> = inner.split(',').filter_map(|n| n.trim().parse::<f32>().ok()).collect();
                if nums.len() == 4 {
                    Some(EasingFunction::CubicBezier(nums[0], nums[1], nums[2], nums[3]))
                } else {
                    None
                }
            } else if lowered.starts_with("steps(") {
                let inner = lowered.trim_start_matches("steps(").trim_end_matches(')');
                if let Ok(n) = inner.trim().parse::<i32>() {
                    Some(EasingFunction::Steps(n))
                } else {
                    None
                }
            } else {
                None
            }
        }
    }
}

fn parse_animation_direction(value: &str) -> Option<AnimationDirection> {
    match value.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(AnimationDirection::Normal),
        "reverse" => Some(AnimationDirection::Reverse),
        "alternate" => Some(AnimationDirection::Alternate),
        "alternate-reverse" => Some(AnimationDirection::AlternateReverse),
        _ => None,
    }
}

fn parse_animation_fill_mode(value: &str) -> Option<AnimationFillMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some(AnimationFillMode::None),
        "forwards" => Some(AnimationFillMode::Forwards),
        "backwards" => Some(AnimationFillMode::Backwards),
        "both" => Some(AnimationFillMode::Both),
        _ => None,
    }
}

fn parse_animation_play_state(value: &str) -> Option<AnimationPlayState> {
    match value.trim().to_ascii_lowercase().as_str() {
        "running" => Some(AnimationPlayState::Running),
        "paused" => Some(AnimationPlayState::Paused),
        _ => None,
    }
}

fn is_easing_keyword(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out"
        | "step-start" | "step-end"
    )
}

fn is_direction_keyword(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "normal" | "reverse" | "alternate" | "alternate-reverse"
    )
}

fn is_fill_mode_keyword(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "none" | "forwards" | "backwards" | "both"
    )
}

fn is_play_state_keyword(w: &str) -> bool {
    matches!(
        w.to_ascii_lowercase().as_str(),
        "running" | "paused"
    )
}

fn is_animation_ident(w: &str) -> bool {
    let lowered = w.to_ascii_lowercase();
    !(lowered == "none"
        || is_easing_keyword(w)
        || is_direction_keyword(w)
        || is_fill_mode_keyword(w)
        || is_play_state_keyword(w)
        || lowered == "infinite"
        || lowered == "initial"
        || lowered == "inherit"
        || lowered == "unset")
}

/// Parse a single animation shorthand value (non-comma-separated piece).
fn parse_single_animation(value: &str) -> Option<SingleAnimation> {
    let mut anim = SingleAnimation::default();
    let tokens: Vec<&str> = value.split_whitespace().collect();
    let mut duration_found = false;

    for token in tokens {
        let token = token.trim();
        if token.is_empty() { continue; }

        // Try to parse as a time value (duration or delay)
        if let Some(time) = parse_css_time(token) {
            if !duration_found {
                anim.duration = time;
                duration_found = true;
            } else {
                anim.delay = time;
            }
            continue;
        }

        // Try easing function
        if (token.starts_with("cubic-bezier(") || token.starts_with("steps("))
            && let Some(easing) = parse_easing_function(token) {
                anim.timing_function = easing;
                continue;
            }

        let lowered = token.to_ascii_lowercase();
        match lowered.as_str() {
            "infinite" => anim.iteration_count = f32::INFINITY,
            _ if is_easing_keyword(token) => {
                if let Some(easing) = parse_easing_function(token) {
                    anim.timing_function = easing;
                }
            }
            _ if is_direction_keyword(token) => {
                if let Some(dir) = parse_animation_direction(token) {
                    anim.direction = dir;
                }
            }
            _ if is_fill_mode_keyword(token) => {
                if let Some(fm) = parse_animation_fill_mode(token) {
                    anim.fill_mode = fm;
                }
            }
            _ if is_play_state_keyword(token) => {
                if let Some(ps) = parse_animation_play_state(token) {
                    anim.play_state = ps;
                }
            }
            _ if is_animation_ident(token) => {
                anim.name = token.to_owned();
            }
            _ => {
                // Try numeric iteration count
                if let Ok(n) = token.parse::<f32>() {
                    anim.iteration_count = n;
                }
            }
        }
    }

    if anim.name.is_empty() || anim.duration == 0.0 {
        // animation without a name or duration is invalid / no-op
        return None;
    }
    Some(anim)
}

/// Parse the `animation` shorthand: comma-separated list of single animations.
fn parse_animation_shorthand(value: &str) -> Option<PropertyValue> {
    let animations: Vec<SingleAnimation> = value
        .split(',')
        .filter_map(|part| parse_single_animation(part.trim()))
        .collect();
    if animations.is_empty() { None } else { Some(PropertyValue::Animation(animations)) }
}

/// Parse a comma-separated list of names for `animation-name`.
fn parse_animation_name_list(value: &str) -> Vec<String> {
    value.split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
        .collect()
}

/// Parse a comma-separated list of time values.
fn parse_time_list(value: &str) -> Vec<f32> {
    value.split(',')
        .filter_map(|s| parse_css_time(s.trim()))
        .collect()
}

/// Parse a comma-separated list of easing functions.
fn parse_easing_list(value: &str) -> Vec<EasingFunction> {
    value.split(',')
        .filter_map(|s| parse_easing_function(s.trim()))
        .collect()
}

/// Parse `animation-iteration-count` list (numbers and "infinite")
fn parse_iteration_count_list(value: &str) -> Vec<f32> {
    value.split(',')
        .map(|s| {
            let s = s.trim();
            if s.eq_ignore_ascii_case("infinite") { f32::INFINITY }
            else { s.parse::<f32>().unwrap_or(1.0) }
        })
        .collect()
}

/// Parse a comma-separated list of animation directions.
fn parse_direction_list(value: &str) -> Vec<AnimationDirection> {
    value.split(',')
        .filter_map(|s| parse_animation_direction(s.trim()))
        .collect()
}

/// Parse a comma-separated list of fill modes.
fn parse_fill_mode_list(value: &str) -> Vec<AnimationFillMode> {
    value.split(',')
        .filter_map(|s| parse_animation_fill_mode(s.trim()))
        .collect()
}

/// Parse a comma-separated list of play states.
fn parse_play_state_list(value: &str) -> Vec<AnimationPlayState> {
    value.split(',')
        .filter_map(|s| parse_animation_play_state(s.trim()))
        .collect()
}

/// Parse a CSS font-family list (comma-separated, with quoted names).
fn parse_font_family_list(value: &str) -> Vec<String> {
    let mut families = Vec::new();
    let mut remaining = value.trim();
    while !remaining.is_empty() {
        remaining = remaining.trim_start();
        if remaining.is_empty() { break; }
        // Check for quoted string
        let (family, rest) = if let Some(quoted) = remaining.strip_prefix('"') {
            let end = quoted.find('"').map(|i| i + 2).unwrap_or(remaining.len());
            (quoted[..end.saturating_sub(2)].to_owned(), &remaining[end..])
        } else if let Some(quoted) = remaining.strip_prefix('\'') {
            let end = quoted.find('\'').map(|i| i + 2).unwrap_or(remaining.len());
            (quoted[..end.saturating_sub(2)].to_owned(), &remaining[end..])
        } else {
            // Unquoted identifier: take until comma or end
            let end = remaining.find(',').unwrap_or(remaining.len());
            let name = remaining[..end].trim().to_owned();
            (name, &remaining[end..])
        };
        if !family.is_empty() {
            families.push(family);
        }
        // Skip past comma
        remaining = rest.strip_prefix(',').unwrap_or(rest);
    }
    families
}

/// Parse a single transition shorthand value.
fn parse_single_transition(value: &str) -> Option<SingleTransition> {
    let mut trans = SingleTransition::default();
    let tokens: Vec<&str> = value.split_whitespace().collect();
    let mut duration_found = false;
    let mut property_found = false;

    for token in &tokens {
        let token = token.trim();
        if token.is_empty() { continue; }

        if let Some(time) = parse_css_time(token) {
            if !duration_found {
                trans.duration = time;
                duration_found = true;
            } else {
                trans.delay = time;
            }
            continue;
        }

        if (token.starts_with("cubic-bezier(") || token.starts_with("steps("))
            && let Some(easing) = parse_easing_function(token) {
                trans.timing_function = easing;
                continue;
            }

        let lowered = token.to_ascii_lowercase();
        if is_easing_keyword(token) {
            if let Some(easing) = parse_easing_function(token) {
                trans.timing_function = easing;
            }
        } else if lowered == "none" || lowered == "all" || !property_found {
            trans.property = token.to_owned();
            property_found = true;
        }
    }

    if trans.property.is_empty() || trans.property == "none" {
        return None;
    }
    Some(trans)
}

/// Parse the `transition` shorthand.
fn parse_transition_shorthand(value: &str) -> Option<PropertyValue> {
    let transitions: Vec<SingleTransition> = value
        .split(',')
        .filter_map(|part| parse_single_transition(part.trim()))
        .collect();
    if transitions.is_empty() { None } else { Some(PropertyValue::Transition(transitions)) }
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
        "font-family" => {
            let families: Vec<String> = parse_font_family_list(value);
            if families.is_empty() { None } else { Some(PropertyValue::FontFamily(families)) }
        }
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
        "flex-direction" => match value.trim() {
            "row" => Some(PropertyValue::FlexDirection(FlexDirection::Row)),
            "column" => Some(PropertyValue::FlexDirection(FlexDirection::Column)),
            _ => None,
        },
        "order" => value.parse::<i32>().ok().map(PropertyValue::Order),
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            parse_border_shorthand(value)
        }
        "box-shadow" => parse_box_shadow(value),
        "outline" => parse_outline_shorthand(value),
        "outline-width" => parse_length(value).map(PropertyValue::Length),
        "outline-style" => parse_outline_style(value),
        "border-width"
        | "border-top-width"
        | "border-right-width"
        | "border-bottom-width"
        | "border-left-width" => parse_length(value).map(PropertyValue::Length),
        "transform" => parse_css_transform(value).map(PropertyValue::Transform),
        // ── Animation and transition properties ──
        "animation" => parse_animation_shorthand(value),
        "animation-name" => {
            let names = parse_animation_name_list(value);
            if names.is_empty() { None } else { Some(PropertyValue::AnimationNames(names)) }
        }
        "animation-duration" => {
            let durations = parse_time_list(value);
            if durations.is_empty() { None } else { Some(PropertyValue::AnimationDurations(durations)) }
        }
        "animation-timing-function" => {
            let easings = parse_easing_list(value);
            if easings.is_empty() { None } else { Some(PropertyValue::AnimationEasings(easings)) }
        }
        "animation-delay" => {
            let delays = parse_time_list(value);
            if delays.is_empty() { None } else { Some(PropertyValue::AnimationDelays(delays)) }
        }
        "animation-iteration-count" => {
            let counts = parse_iteration_count_list(value);
            if counts.is_empty() { None } else { Some(PropertyValue::AnimationIterationCounts(counts)) }
        }
        "animation-direction" => {
            let dirs = parse_direction_list(value);
            if dirs.is_empty() { None } else { Some(PropertyValue::AnimationDirections(dirs)) }
        }
        "animation-fill-mode" => {
            let modes = parse_fill_mode_list(value);
            if modes.is_empty() { None } else { Some(PropertyValue::AnimationFillModes(modes)) }
        }
        "animation-play-state" => {
            let states = parse_play_state_list(value);
            if states.is_empty() { None } else { Some(PropertyValue::AnimationPlayStates(states)) }
        }
        "transition" => parse_transition_shorthand(value),
        "transition-property" => {
            let props: Vec<String> = value.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"))
                .collect();
            if props.is_empty() { None } else { Some(PropertyValue::TransitionProperties(props)) }
        }
        "transition-duration" => {
            let durations = parse_time_list(value);
            if durations.is_empty() { None } else { Some(PropertyValue::TransitionDurations(durations)) }
        }
        "transition-timing-function" => {
            let easings = parse_easing_list(value);
            if easings.is_empty() { None } else { Some(PropertyValue::TransitionEasings(easings)) }
        }
        "transition-delay" => {
            let delays = parse_time_list(value);
            if delays.is_empty() { None } else { Some(PropertyValue::TransitionDelays(delays)) }
        }
        _ => None,
    }
}

/// Resolve all `var(--name)` and `var(--name, fallback)` references in a value string.
/// Uses the provided custom properties map for lookups.
fn resolve_var_references(value: &str, custom_props: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(value.len());
    let mut remaining = value;

    while let Some(var_start) = remaining.find("var(") {
        // Append everything before var()
        result.push_str(&remaining[..var_start]);
        let after = &remaining[var_start + 4..]; // skip "var("

        // Find the matching closing paren (handle nested parens)
        let mut depth = 1;
        let mut end = 0;
        for (i, ch) in after.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }

        if depth != 0 {
            // Unmatched paren — keep the original text
            result.push_str("var(");
            remaining = after;
            continue;
        }

        let var_body = &after[..end];
        remaining = &after[end + 1..]; // skip past ')'

        // Parse var body: --name or --name, fallback
        let (var_name, fallback) = if let Some(comma_pos) = var_body.find(',') {
            let name = var_body[..comma_pos].trim();
            let fb = var_body[comma_pos + 1..].trim();
            (name, Some(fb))
        } else {
            (var_body.trim(), None)
        };

        // Look up the custom property
        let resolved = custom_props
            .get(var_name)
            .map(|s| s.as_str())
            .or(fallback);

        if let Some(val) = resolved {
            // Recursively resolve var() in the resolved value
            let resolved_val = resolve_var_references(val, custom_props);
            result.push_str(&resolved_val);
        }
        // If no value and no fallback, the property is invalid at computed time
        // (we leave it empty, which effectively drops the declaration)
    }

    // Append remaining text after the last var()
    result.push_str(remaining);
    result
}

fn apply_declarations(style: &mut ComputedStyle, declarations: &[Declaration]) {
    for declaration in declarations {
        // Store custom properties
        if declaration.property.starts_with("--") {
            if let PropertyValue::String(val) = &declaration.value {
                let resolved = resolve_var_references(val, &style.custom_properties);
                style.custom_properties.insert(declaration.property.to_string(), resolved);
            }
            continue;
        }

        // Resolve var() references in String-typed values and re-parse
        let effective_value = if let PropertyValue::String(raw) = &declaration.value {
            let resolved = resolve_var_references(raw, &style.custom_properties);
            if resolved.is_empty() {
                continue; // Unresolvable var() with no fallback
            }
            match parse_property_value(&declaration.property, &resolved) {
                Some(parsed) => parsed,
                None => continue,
            }
        } else {
            declaration.value.clone()
        };

        match (declaration.property.as_str(), &effective_value) {
            ("display", PropertyValue::Display(value)) => style.display = *value,
            ("color", PropertyValue::Color(value)) => style.color = *value,
            ("background-color", PropertyValue::Color(value)) => style.background_color = *value,
            ("font-size", PropertyValue::Length(value)) => style.font_size = *value,
            ("font-weight", PropertyValue::FontWeight(value)) => style.font_weight = *value,
            ("font-style", PropertyValue::FontStyle(value)) => style.font_style = *value,
            ("font-family", PropertyValue::FontFamily(value)) => style.font_family = value.clone(),
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
            ("flex-direction", PropertyValue::FlexDirection(value)) => style.flex_direction = *value,
            ("order", PropertyValue::Order(value)) => style.order = *value,
            ("border", PropertyValue::Border(value)) => {
                style.border.top = Border { width: value.top.width, style: value.top.style, color: value.top.color };
                style.border.right = Border { width: value.right.width, style: value.right.style, color: value.right.color };
                style.border.bottom = Border { width: value.bottom.width, style: value.bottom.style, color: value.bottom.color };
                style.border.left = Border { width: value.left.width, style: value.left.style, color: value.left.color };
            }
            ("border-top", PropertyValue::Border(value)) => {
                style.border.top = Border { width: value.top.width, style: value.top.style, color: value.top.color };
            }
            ("border-right", PropertyValue::Border(value)) => {
                style.border.right = Border { width: value.right.width, style: value.right.style, color: value.right.color };
            }
            ("border-bottom", PropertyValue::Border(value)) => {
                style.border.bottom = Border { width: value.bottom.width, style: value.bottom.style, color: value.bottom.color };
            }
            ("border-left", PropertyValue::Border(value)) => {
                style.border.left = Border { width: value.left.width, style: value.left.style, color: value.left.color };
            }
            ("border-width", PropertyValue::Length(value)) => {
                let px = value.to_px(16.0, 1280.0, 16.0);
                style.border.top.width = px;
                style.border.right.width = px;
                style.border.bottom.width = px;
                style.border.left.width = px;
            }
            ("border-top-width", PropertyValue::Length(value)) => {
                style.border.top.width = value.to_px(16.0, 1280.0, 16.0);
            }
            ("border-right-width", PropertyValue::Length(value)) => {
                style.border.right.width = value.to_px(16.0, 1280.0, 16.0);
            }
            ("border-bottom-width", PropertyValue::Length(value)) => {
                style.border.bottom.width = value.to_px(16.0, 1280.0, 16.0);
            }
            ("border-left-width", PropertyValue::Length(value)) => {
                style.border.left.width = value.to_px(16.0, 1280.0, 16.0);
            }
            ("border-color", PropertyValue::Color(value)) => {
                style.border.top.color = *value;
                style.border.right.color = *value;
                style.border.bottom.color = *value;
                style.border.left.color = *value;
            }
            ("border-style", PropertyValue::Border(value)) => {
                style.border.top.style = value.top.style;
                style.border.right.style = value.right.style;
                style.border.bottom.style = value.bottom.style;
                style.border.left.style = value.left.style;
            }
            ("box-shadow", PropertyValue::BoxShadow(value)) => {
                style.box_shadow = Some(*value);
            }
            ("outline", PropertyValue::Outline(value)) => {
                style.outline = *value;
            }
            ("outline-width", PropertyValue::Length(value)) => {
                style.outline.width = value.to_px(16.0, 1280.0, 16.0);
            }
            ("outline-style", PropertyValue::Outline(value)) => {
                style.outline.style = value.style;
            }
            ("outline-color", PropertyValue::Color(value)) => {
                style.outline.color = *value;
            }
            ("transform", PropertyValue::Transform(value)) => {
                style.transform = value.clone();
            }
            // ── Animation and transition handling ──
            ("animation", PropertyValue::Animation(value)) => {
                style.animations = value.clone();
            }
            ("animation-name", PropertyValue::AnimationNames(value)) => {
                let count = value.len();
                style.animations.resize_with(count, Default::default);
                for (i, name) in value.iter().enumerate() {
                    style.animations[i].name = name.clone();
                }
            }
            ("animation-duration", PropertyValue::AnimationDurations(value)) => {
                for (i, dur) in value.iter().enumerate() {
                    let idx = i % style.animations.len().max(1);
                    if idx >= style.animations.len() {
                        style.animations.resize_with(idx + 1, Default::default);
                    }
                    style.animations[idx].duration = *dur;
                }
            }
            ("animation-timing-function", PropertyValue::AnimationEasings(value)) => {
                for (i, easing) in value.iter().enumerate() {
                    let idx = i % style.animations.len().max(1);
                    if idx >= style.animations.len() {
                        style.animations.resize_with(idx + 1, Default::default);
                    }
                    style.animations[idx].timing_function = *easing;
                }
            }
            ("animation-delay", PropertyValue::AnimationDelays(value)) => {
                for (i, delay) in value.iter().enumerate() {
                    let idx = i % style.animations.len().max(1);
                    if idx >= style.animations.len() {
                        style.animations.resize_with(idx + 1, Default::default);
                    }
                    style.animations[idx].delay = *delay;
                }
            }
            ("animation-iteration-count", PropertyValue::AnimationIterationCounts(value)) => {
                for (i, count) in value.iter().enumerate() {
                    let idx = i % style.animations.len().max(1);
                    if idx >= style.animations.len() {
                        style.animations.resize_with(idx + 1, Default::default);
                    }
                    style.animations[idx].iteration_count = *count;
                }
            }
            ("animation-direction", PropertyValue::AnimationDirections(value)) => {
                for (i, dir) in value.iter().enumerate() {
                    let idx = i % style.animations.len().max(1);
                    if idx >= style.animations.len() {
                        style.animations.resize_with(idx + 1, Default::default);
                    }
                    style.animations[idx].direction = dir.clone();
                }
            }
            ("animation-fill-mode", PropertyValue::AnimationFillModes(value)) => {
                for (i, mode) in value.iter().enumerate() {
                    let idx = i % style.animations.len().max(1);
                    if idx >= style.animations.len() {
                        style.animations.resize_with(idx + 1, Default::default);
                    }
                    style.animations[idx].fill_mode = *mode;
                }
            }
            ("animation-play-state", PropertyValue::AnimationPlayStates(value)) => {
                for (i, state) in value.iter().enumerate() {
                    let idx = i % style.animations.len().max(1);
                    if idx >= style.animations.len() {
                        style.animations.resize_with(idx + 1, Default::default);
                    }
                    style.animations[idx].play_state = state.clone();
                }
            }
            ("transition", PropertyValue::Transition(value)) => {
                style.transitions = value.clone();
            }
            ("transition-property", PropertyValue::TransitionProperties(value)) => {
                let count = value.len();
                style.transitions.resize_with(count, Default::default);
                for (i, prop) in value.iter().enumerate() {
                    style.transitions[i].property = prop.clone();
                }
            }
            ("transition-duration", PropertyValue::TransitionDurations(value)) => {
                for (i, dur) in value.iter().enumerate() {
                    let idx = i % style.transitions.len().max(1);
                    if idx >= style.transitions.len() {
                        style.transitions.resize_with(idx + 1, Default::default);
                    }
                    style.transitions[idx].duration = *dur;
                }
            }
            ("transition-timing-function", PropertyValue::TransitionEasings(value)) => {
                for (i, easing) in value.iter().enumerate() {
                    let idx = i % style.transitions.len().max(1);
                    if idx >= style.transitions.len() {
                        style.transitions.resize_with(idx + 1, Default::default);
                    }
                    style.transitions[idx].timing_function = *easing;
                }
            }
            ("transition-delay", PropertyValue::TransitionDelays(value)) => {
                for (i, delay) in value.iter().enumerate() {
                    let idx = i % style.transitions.len().max(1);
                    if idx >= style.transitions.len() {
                        style.transitions.resize_with(idx + 1, Default::default);
                    }
                    style.transitions[idx].delay = *delay;
                }
            }
            _ => {}
        }
    }
}

fn parse_box_shadow(value: &str) -> Option<PropertyValue> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("none") || trimmed.is_empty() {
        return Some(PropertyValue::BoxShadow(BoxShadow {
            inset: false, offset_x: 0.0, offset_y: 0.0, blur: 0.0, spread: 0.0, color: Color::TRANSPARENT,
        }));
    }
    let parts: Vec<String> = trimmed.split_whitespace().map(|s| s.to_owned()).collect();

    let mut inset = false;
    let mut offset_x = 0.0f32;
    let mut offset_y = 0.0f32;
    let mut blur = 0.0f32;
    let mut spread = 0.0f32;
    let mut color = Color::rgba(0, 0, 0, 128);
    let mut value_count = 0u8;

    for part in &parts {
        let lower = part.to_ascii_lowercase();
        if lower == "inset" {
            inset = true;
        } else if let Ok(v) = part.parse::<f32>() {
            value_count += 1;
            match value_count {
                1 => offset_x = v,
                2 => offset_y = v,
                3 => blur = v,
                4 => spread = v,
                _ => {}
            }
        } else if let Some(c) = parse_color(part) {
            color = c;
        }
    }

    Some(PropertyValue::BoxShadow(BoxShadow { inset, offset_x, offset_y, blur, spread, color }))
}

fn parse_outline_style(value: &str) -> Option<PropertyValue> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some(PropertyValue::Outline(OutlineSizes { width: 0.0, style: OutlineStyle::None, color: Color::TRANSPARENT })),
        "solid" => Some(PropertyValue::Outline(OutlineSizes { width: 1.0, style: OutlineStyle::Solid, color: Color::BLACK })),
        "dashed" => Some(PropertyValue::Outline(OutlineSizes { width: 1.0, style: OutlineStyle::Dashed, color: Color::BLACK })),
        "dotted" => Some(PropertyValue::Outline(OutlineSizes { width: 1.0, style: OutlineStyle::Dotted, color: Color::BLACK })),
        "double" => Some(PropertyValue::Outline(OutlineSizes { width: 2.0, style: OutlineStyle::Double, color: Color::BLACK })),
        "groove" => Some(PropertyValue::Outline(OutlineSizes { width: 1.0, style: OutlineStyle::Groove, color: Color::BLACK })),
        "ridge" => Some(PropertyValue::Outline(OutlineSizes { width: 1.0, style: OutlineStyle::Ridge, color: Color::BLACK })),
        "inset" => Some(PropertyValue::Outline(OutlineSizes { width: 1.0, style: OutlineStyle::Inset, color: Color::BLACK })),
        "outset" => Some(PropertyValue::Outline(OutlineSizes { width: 1.0, style: OutlineStyle::Outset, color: Color::BLACK })),
        _ => None,
    }
}

fn parse_outline_shorthand(value: &str) -> Option<PropertyValue> {
    let mut width = 0.0f32;
    let mut style = OutlineStyle::None;
    let mut color = Color::TRANSPARENT;
    for part in value.split_whitespace() {
        let lowered = part.to_ascii_lowercase();
        if let Ok(v) = part.parse::<f32>() {
            width = v;
        } else if let Some(c) = parse_color(part) {
            color = c;
        } else {
            match lowered.as_str() {
                "none" => style = OutlineStyle::None,
                "solid" => style = OutlineStyle::Solid,
                "dashed" => style = OutlineStyle::Dashed,
                "dotted" => style = OutlineStyle::Dotted,
                "double" => style = OutlineStyle::Double,
                _ => {}
            }
        }
    }
    Some(PropertyValue::Outline(OutlineSizes { width, style, color }))
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

pub(crate) fn parse_color(input: &str) -> Option<Color> {
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

/// Parse the body of an `@font-face { ... }` rule into a `FontFaceRule`.
fn parse_font_face_body(body: &str) -> Option<FontFaceRule> {
    let mut family = String::new();
    let mut sources = Vec::new();
    let mut weight = FontWeight::Normal;
    let mut style = FontStyle::Normal;
    let mut display = FontDisplay::Auto;
    let mut unicode_range = None;

    for decl in body.split(';') {
        let decl = decl.trim();
        if decl.is_empty() { continue; }
        let Some((prop, val)) = decl.split_once(':') else { continue; };
        let prop = prop.trim().to_ascii_lowercase();
        let val = val.trim();

        match prop.as_str() {
            "font-family" => {
                family = val.trim_matches('"').trim_matches('\'').to_owned();
            }
            "src" => {
                // Parse src: url("...") format("..."), url("...") format("...")
                let mut remaining = val;
                while !remaining.is_empty() {
                    let remaining_trimmed = remaining.trim_start().trim_start_matches(',').trim_start();
                    remaining = remaining_trimmed;
                    if remaining.is_empty() { break; }
                    if let Some(url_start) = remaining.find("url(") {
                        let after = &remaining[url_start + 4..];
                        let close = after.find(')')?;
                        let url_raw = &after[..close];
                        let url = url_raw.trim_matches('"').trim_matches('\'').to_owned();
                        remaining = &after[close + 1..];
                        // Check for format(...)
                        let format = if remaining.trim_start().starts_with("format(") {
                            let fmt_start = remaining.trim_start()["format(".len()..].to_string();
                            if let Some(fmt_close) = fmt_start.find(')') {
                                let fmt_raw = &fmt_start[..fmt_close];
                                remaining = &remaining.trim_start()["format(".len() + fmt_close + 1..];
                                Some(fmt_raw.trim_matches('"').trim_matches('\'').to_owned())
                            } else { None }
                        } else { None };
                        sources.push(FontFaceSource { url, format });
                    } else {
                        break;
                    }
                }
            }
            "font-weight" => {
                match val.to_ascii_lowercase().as_str() {
                    "bold" | "700" => weight = FontWeight::Bold,
                    "normal" | "400" => weight = FontWeight::Normal,
                    _ => {
                        if let Ok(n) = val.parse::<u16>() {
                            weight = FontWeight::Number(n);
                        }
                    }
                }
            }
            "font-style" => {
                match val.to_ascii_lowercase().as_str() {
                    "italic" => style = FontStyle::Italic,
                    "oblique" => style = FontStyle::Oblique,
                    _ => style = FontStyle::Normal,
                }
            }
            "font-display" => {
                match val.to_ascii_lowercase().as_str() {
                    "block" => display = FontDisplay::Block,
                    "swap" => display = FontDisplay::Swap,
                    "fallback" => display = FontDisplay::Fallback,
                    "optional" => display = FontDisplay::Optional,
                    _ => display = FontDisplay::Auto,
                }
            }
            "unicode-range" => {
                unicode_range = Some(val.to_owned());
            }
            _ => {}
        }
    }

    if family.is_empty() {
        return None;
    }

    Some(FontFaceRule {
        family,
        sources,
        weight,
        style,
        display,
        unicode_range,
    })
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

pub(crate) fn parse_length(input: &str) -> Option<Length> {
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
    fn parse_font_face_rule_extracts_family_and_url() {
        let css = r#"
            @font-face {
                font-family: "CustomFont";
                src: url("https://example.com/font.woff2") format("woff2"),
                     url("https://example.com/font.woff") format("woff");
                font-weight: bold;
                font-style: italic;
                font-display: swap;
                unicode-range: U+0000-00FF;
            }
            body { color: red; }
        "#;
        let sheet = Stylesheet::parse(css).unwrap();
        assert_eq!(sheet.font_face_rules.len(), 1);
        let rule = &sheet.font_face_rules[0];
        assert_eq!(rule.family, "CustomFont");
        assert_eq!(rule.sources.len(), 2);
        assert_eq!(rule.sources[0].url, "https://example.com/font.woff2");
        assert_eq!(rule.sources[0].format.as_deref(), Some("woff2"));
        assert_eq!(rule.sources[1].url, "https://example.com/font.woff");
        assert_eq!(rule.sources[1].format.as_deref(), Some("woff"));
        assert_eq!(rule.weight, FontWeight::Bold);
        assert_eq!(rule.style, FontStyle::Italic);
        assert_eq!(rule.display, FontDisplay::Swap);
        assert_eq!(rule.unicode_range.as_deref(), Some("U+0000-00FF"));
    }

    #[test]
    fn parse_font_face_without_format() {
        let css = r#"
            @font-face {
                font-family: SimpleFont;
                src: url("./simple.ttf");
            }
        "#;
        let sheet = Stylesheet::parse(css).unwrap();
        assert_eq!(sheet.font_face_rules.len(), 1);
        let rule = &sheet.font_face_rules[0];
        assert_eq!(rule.family, "SimpleFont");
        assert_eq!(rule.sources.len(), 1);
        assert_eq!(rule.sources[0].url, "./simple.ttf");
        assert_eq!(rule.sources[0].format, None);
        assert_eq!(rule.weight, FontWeight::Normal);
        assert_eq!(rule.style, FontStyle::Normal);
        assert_eq!(rule.display, FontDisplay::Auto);
    }

    #[test]
    fn style_engine_collects_font_face_rules() {
        let css = r#"
            @font-face {
                font-family: "MyFont";
                src: url("/fonts/my.woff2") format("woff2");
            }
            @font-face {
                font-family: "MyFont";
                src: url("/fonts/my-bold.woff2") format("woff2");
                font-weight: bold;
            }
        "#;
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(Stylesheet::parse(css).unwrap());
        let rules = engine.font_face_rules();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].family, "MyFont");
        assert_eq!(rules[1].weight, FontWeight::Bold);
    }

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

    // ── CSS Variables tests ─────────────────────────────────────────────

    #[test]
    fn custom_property_stored_and_inherited() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<div class="parent"><p>Child</p></div>"#).unwrap();
        let div = document.first_element_by_tag("div").unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(
            Stylesheet::parse(".parent { --brand: #ff0000; color: var(--brand); }").unwrap(),
        );

        let parent_style = engine.compute_style(div, None);
        assert_eq!(parent_style.custom_properties.get("--brand").map(|s| s.as_str()), Some("#ff0000"));
        assert_eq!(parent_style.color, Color::rgb(255, 0, 0));

        // Child inherits the custom property
        let child_style = engine.compute_style(p, Some(&parent_style));
        assert_eq!(child_style.custom_properties.get("--brand").map(|s| s.as_str()), Some("#ff0000"));
    }

    #[test]
    fn var_with_fallback() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<p>Text</p>"#).unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let mut engine = StyleEngine::new();
        // --undefined is not defined, so fallback should be used
        engine.add_stylesheet(
            Stylesheet::parse("p { color: var(--undefined, blue); }").unwrap(),
        );

        let style = engine.compute_style(p, None);
        assert_eq!(style.color, Color::rgb(0, 0, 255));
    }

    #[test]
    fn var_resolves_to_length() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<main>Content</main>"#).unwrap();
        let main = document.first_element_by_tag("main").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(
            Stylesheet::parse("main { --w: 200px; width: var(--w); }").unwrap(),
        );

        let style = engine.compute_style(main, None);
        assert_eq!(style.width, Length::Px(200.0));
    }

    #[test]
    fn var_chained_reference() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<p>Text</p>"#).unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(
            Stylesheet::parse("p { --base: #00ff00; --accent: var(--base); color: var(--accent); }").unwrap(),
        );

        let style = engine.compute_style(p, None);
        assert_eq!(style.color, Color::rgb(0, 255, 0));
    }

    #[test]
    fn inline_style_custom_property() {
        let arena = DomArena::new();
        let document = parse_html(
            &arena,
            r#"<p style="--sz: 24px; font-size: var(--sz)">Big</p>"#,
        ).unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let engine = StyleEngine::new();

        let style = engine.compute_style(p, None);
        assert_eq!(style.font_size, Length::Px(24.0));
    }

    // ── CSS Transform tests ─────────────────────────────────────────────

    #[test]
    fn parse_transform_translate() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<div>Box</div>"#).unwrap();
        let div = document.first_element_by_tag("div").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(
            Stylesheet::parse("div { transform: translateX(10px) translateY(20px); }").unwrap(),
        );

        let style = engine.compute_style(div, None);
        assert!(!style.transform.is_none());
        assert_eq!(style.transform.functions.len(), 2);
    }

    #[test]
    fn parse_transform_rotate_scale() {
        let t = parse_css_transform("rotate(45deg) scale(2)").unwrap();
        assert_eq!(t.functions.len(), 2);
        match t.functions[0] {
            TransformFunction::Rotate(deg) => assert_eq!(deg, 45.0),
            _ => panic!("Expected Rotate"),
        }
        match t.functions[1] {
            TransformFunction::Scale(sx, sy) => { assert_eq!(sx, 2.0); assert_eq!(sy, 2.0); }
            _ => panic!("Expected Scale"),
        }
    }

    #[test]
    fn transform_combined_matrix_identity() {
        let t = CssTransform::none();
        let m = t.combined_matrix();
        assert_eq!(m, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn transform_combined_matrix_translate() {
        let t = CssTransform { functions: vec![TransformFunction::Translate(10.0, 20.0)] };
        let m = t.combined_matrix();
        assert_eq!(m[4], 10.0); // tx
        assert_eq!(m[5], 20.0); // ty
    }

    // ── Media Query tests ───────────────────────────────────────────────

    #[test]
    fn parse_media_query_min_width() {
        let q = parse_media_query("(min-width: 768px)").unwrap();
        assert!(!q.negated);
        assert_eq!(q.media_type, "all");
        assert_eq!(q.features.len(), 1);
        match &q.features[0] {
            MediaFeature::MinWidth(w) => assert_eq!(*w, 768.0),
            _ => panic!("Expected MinWidth"),
        }
    }

    #[test]
    fn parse_media_query_prefers_color_scheme() {
        let q = parse_media_query("(prefers-color-scheme: dark)").unwrap();
        assert_eq!(q.features.len(), 1);
        assert_eq!(q.features[0], MediaFeature::PrefersColorScheme(ColorScheme::Dark));
    }

    #[test]
    fn media_context_evaluates_width() {
        let ctx = MediaContext { viewport_width: 1024.0, viewport_height: 768.0, ..Default::default() };
        let q = parse_media_query("(min-width: 768px)").unwrap();
        assert!(ctx.evaluate(&q));
        let q2 = parse_media_query("(min-width: 1200px)").unwrap();
        assert!(!ctx.evaluate(&q2));
    }

    #[test]
    fn media_context_evaluates_color_scheme() {
        let ctx = MediaContext { color_scheme: ColorScheme::Dark, ..Default::default() };
        let q_dark = parse_media_query("(prefers-color-scheme: dark)").unwrap();
        assert!(ctx.evaluate(&q_dark));
        let q_light = parse_media_query("(prefers-color-scheme: light)").unwrap();
        assert!(!ctx.evaluate(&q_light));
    }

    #[test]
    fn media_query_negation() {
        let ctx = MediaContext { color_scheme: ColorScheme::Dark, ..Default::default() };
        let q = parse_media_query("not (prefers-color-scheme: light)").unwrap();
        assert!(q.negated);
        assert!(ctx.evaluate(&q)); // Dark != Light, so negated = true
    }

    #[test]
    fn stylesheet_parses_media_rule() {
        let css = r#"
            p { color: black; }
            @media (min-width: 768px) {
                p { color: blue; }
            }
        "#;
        let sheet = Stylesheet::parse(css).unwrap();
        assert_eq!(sheet.rules.len(), 1); // only the non-media rule
        assert_eq!(sheet.media_rules.len(), 1);
        assert_eq!(sheet.media_rules[0].rules.len(), 1);
    }

    #[test]
    fn media_query_applies_when_matching() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<p>Text</p>"#).unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let mut engine = StyleEngine::new()
            .with_media_context(MediaContext { viewport_width: 1024.0, ..Default::default() });
        engine.add_stylesheet(
            Stylesheet::parse(r#"
                p { color: black; }
                @media (min-width: 768px) { p { color: blue; } }
            "#).unwrap(),
        );

        let style = engine.compute_style(p, None);
        assert_eq!(style.color, Color::rgb(0, 0, 255)); // blue from media query
    }

    #[test]
    fn media_query_not_applied_when_not_matching() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<p>Text</p>"#).unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let mut engine = StyleEngine::new()
            .with_media_context(MediaContext { viewport_width: 500.0, ..Default::default() });
        engine.add_stylesheet(
            Stylesheet::parse(r#"
                p { color: black; }
                @media (min-width: 768px) { p { color: blue; } }
            "#).unwrap(),
        );

        let style = engine.compute_style(p, None);
        assert_eq!(style.color, Color::BLACK); // black, not blue
    }

    #[test]
    fn media_query_orientation() {
        let ctx = MediaContext { viewport_width: 800.0, viewport_height: 600.0, ..Default::default() };
        let q = parse_media_query("(orientation: landscape)").unwrap();
        assert!(ctx.evaluate(&q));
        let q2 = parse_media_query("(orientation: portrait)").unwrap();
        assert!(!ctx.evaluate(&q2));
    }

    #[test]
    fn media_query_reduced_motion() {
        let ctx = MediaContext { reduced_motion: ReducedMotion::Reduce, ..Default::default() };
        let q = parse_media_query("(prefers-reduced-motion: reduce)").unwrap();
        assert!(ctx.evaluate(&q));
    }

    #[test]
    fn keyframes_only_parse() {
        let css = r#"@keyframes fadein { from { opacity: 0; } to { opacity: 1; } }"#;
        let sheet = Stylesheet::parse(css).unwrap();
        eprintln!("keyframes_only: rules={}, keyframes={}, media_rules={}",
            sheet.rules.len(), sheet.keyframes.len(), sheet.media_rules.len());
        for kf in &sheet.keyframes {
            eprintln!("  kf name={}, keyframe_count={}", kf.name, kf.keyframes.len());
        }
        assert_eq!(sheet.keyframes.len(), 1, "should have 1 keyframe rule");
    }

    #[test]
    fn keyframes_with_rule_parse() {
        let css = r#"
            @keyframes fadein { from { opacity: 0; } to { opacity: 1; } }
            p { color: red; }
        "#;
        let sheet = Stylesheet::parse(css).unwrap();
        eprintln!("keyframes+rule: rules={}, keyframes={}, media_rules={}",
            sheet.rules.len(), sheet.keyframes.len(), sheet.media_rules.len());
        for kf in &sheet.keyframes {
            eprintln!("  kf name={}", kf.name);
        }
        for r in &sheet.rules {
            eprintln!("  rule selectors={:?}", r.selectors.iter().map(|s| format!("{:?}", s.parts)).collect::<Vec<_>>());
        }
        assert_eq!(sheet.keyframes.len(), 1, "should have 1 keyframe rule");
        assert_eq!(sheet.rules.len(), 1, "should have 1 regular rule");
    }

    #[test]
    fn animation_parsed_from_css_rule() {
        let css = r#"
            @keyframes fadein { from { opacity: 0; } to { opacity: 1; } }
            p { animation: fadein 1s; }
        "#;
        let sheet = Stylesheet::parse(css).unwrap();

        // Now test through the full StyleEngine
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<p>Text</p>"#).unwrap();
        let p = document.first_element_by_tag("p").unwrap();
        let mut engine = StyleEngine::new();
        engine.add_stylesheet(sheet);
        let style = engine.compute_style(p, None);
        assert!(!style.animations.is_empty(), "p should have animations");
        assert_eq!(style.animations[0].name, "fadein");
    }

    #[test]
    fn animation_renders_through_full_pipeline() {
        // Use the renderer to test the full pipeline
    }
}
