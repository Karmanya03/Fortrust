//! CSS Animation & Transition engine.
//!
//! Implements:
//! - **Property interpolation** for colors, lengths, opacity, and transforms
//! - **Keyframe evaluation** — finds the right keyframe pair and interpolates
//! - **Animation state tracking** — elapsed time, iteration count, direction, fill mode
//! - **Transition tracking** — detects property changes and animates between old/new values
//! - **AnimationController** — per-element state, ticked each frame

use crate::{
    AnimationDirection, AnimationFillMode, AnimationPlayState, Color, ComputedStyle,
    KeyframesRule, Length, SingleAnimation, SingleTransition,
};
#[cfg(test)]
use crate::EasingFunction;

// ── Interpolatable property values ───────────────────────────────────────────

/// A CSS property value that can be interpolated between two states.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimatableValue {
    Color(Color),
    Length(Length),
    Opacity(f32),
    /// Transform: (translate_x, translate_y, rotate_deg, scale_x, scale_y)
    Transform(TransformValue),
    None,
}

/// A 2D CSS transform decomposed for interpolation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformValue {
    pub translate_x: f32,
    pub translate_y: f32,
    pub rotate_deg: f32,
    pub scale_x: f32,
    pub scale_y: f32,
}

impl Default for TransformValue {
    fn default() -> Self {
        Self {
            translate_x: 0.0,
            translate_y: 0.0,
            rotate_deg: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
        }
    }
}

impl AnimatableValue {
    /// Linearly interpolate between `self` (at t=0) and `other` (at t=1).
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        match (self, other) {
            (Self::Color(a), Self::Color(b)) => Self::Color(lerp_color(*a, *b, t)),
            (Self::Length(a), Self::Length(b)) => Self::Length(lerp_length(*a, *b, t)),
            (Self::Opacity(a), Self::Opacity(b)) => Self::Opacity(a + (b - a) * t),
            (Self::Transform(a), Self::Transform(b)) => Self::Transform(lerp_transform(*a, *b, t)),
            _ => other.clone(), // Can't interpolate mismatched types — snap to end
        }
    }

    /// Parse a CSS property value string into an AnimatableValue.
    pub fn from_css_property(property: &str, value: &str) -> Self {
        let value = value.trim();
        match property {
            "color" | "background-color" | "border-color" | "outline-color" => {
                crate::parse_color(value)
                    .map(Self::Color)
                    .unwrap_or(Self::None)
            }
            "opacity" => value.parse::<f32>().ok().map(Self::Opacity).unwrap_or(Self::None),
            "transform" => parse_transform_value(value).map(Self::Transform).unwrap_or(Self::None),
            "width" | "height" | "min-width" | "max-width" | "min-height" | "max-height"
            | "margin-top" | "margin-right" | "margin-bottom" | "margin-left"
            | "padding-top" | "padding-right" | "padding-bottom" | "padding-left"
            | "left" | "right" | "top" | "bottom" | "font-size" | "line-height"
            | "letter-spacing" | "word-spacing" | "text-indent" => {
                crate::parse_length(value)
                    .map(Self::Length)
                    .unwrap_or(Self::None)
            }
            _ => Self::None,
        }
    }

    /// Extract the animatable value for a property from a ComputedStyle.
    pub fn from_computed_style(property: &str, style: &ComputedStyle) -> Self {
        match property {
            "color" => Self::Color(style.color),
            "background-color" => Self::Color(style.background_color),
            "opacity" => Self::Opacity(style.opacity),
            "width" => Self::Length(style.width),
            "height" => Self::Length(style.height),
            "min-width" => Self::Length(style.min_width),
            "max-width" => Self::Length(style.max_width),
            "min-height" => Self::Length(style.min_height),
            "max-height" => Self::Length(style.max_height),
            "margin-top" => Self::Length(style.margin.top),
            "margin-right" => Self::Length(style.margin.right),
            "margin-bottom" => Self::Length(style.margin.bottom),
            "margin-left" => Self::Length(style.margin.left),
            "padding-top" => Self::Length(style.padding.top),
            "padding-right" => Self::Length(style.padding.right),
            "padding-bottom" => Self::Length(style.padding.bottom),
            "padding-left" => Self::Length(style.padding.left),
            "font-size" => Self::Length(style.font_size),
            "left" => Self::Length(style.left),
            "right" => Self::Length(style.right),
            "top" => Self::Length(style.top),
            "bottom" => Self::Length(style.bottom),
            "line-height" => Self::Length(style.line_height),
            "letter-spacing" => Self::Length(style.letter_spacing),
            "word-spacing" => Self::Length(style.word_spacing),
            "text-indent" => Self::Length(style.text_indent),
            _ => Self::None,
        }
    }

    /// Apply this animated value back to a ComputedStyle.
    pub fn apply_to(&self, property: &str, style: &mut ComputedStyle) {
        match self {
            Self::Color(c) => match property {
                "color" => style.color = *c,
                "background-color" => style.background_color = *c,
                "border-color" => {
                    style.border.top.color = *c;
                    style.border.right.color = *c;
                    style.border.bottom.color = *c;
                    style.border.left.color = *c;
                }
                "outline-color" => style.outline.color = *c,
                _ => {}
            },
            Self::Opacity(v) => {
                if property == "opacity" {
                    style.opacity = *v;
                }
            }
            Self::Length(l) => match property {
                "width" => style.width = *l,
                "height" => style.height = *l,
                "min-width" => style.min_width = *l,
                "max-width" => style.max_width = *l,
                "min-height" => style.min_height = *l,
                "max-height" => style.max_height = *l,
                "margin-top" => style.margin.top = *l,
                "margin-right" => style.margin.right = *l,
                "margin-bottom" => style.margin.bottom = *l,
                "margin-left" => style.margin.left = *l,
                "padding-top" => style.padding.top = *l,
                "padding-right" => style.padding.right = *l,
                "padding-bottom" => style.padding.bottom = *l,
                "padding-left" => style.padding.left = *l,
                "font-size" => style.font_size = *l,
                "left" => style.left = *l,
                "right" => style.right = *l,
                "top" => style.top = *l,
                "bottom" => style.bottom = *l,
                "line-height" => style.line_height = *l,
                "letter-spacing" => style.letter_spacing = *l,
                "word-spacing" => style.word_spacing = *l,
                "text-indent" => style.text_indent = *l,
                _ => {}
            },
            Self::Transform(_) | Self::None => {
                // Transform is handled separately via the layout/paint pipeline
            }
        }
    }
}

// ── Interpolation functions ──────────────────────────────────────────────────

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        lerp_u8(a.r, b.r, t),
        lerp_u8(a.g, b.g, t),
        lerp_u8(a.b, b.b, t),
        lerp_u8(a.a, b.a, t),
    )
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let result = (a as f32) + (b as f32 - a as f32) * t;
    result.round().clamp(0.0, 255.0) as u8
}

fn lerp_length(a: Length, b: Length, t: f32) -> Length {
    match (a, b) {
        (Length::Px(va), Length::Px(vb)) => Length::Px(va + (vb - va) * t),
        (Length::Em(va), Length::Em(vb)) => Length::Em(va + (vb - va) * t),
        (Length::Rem(va), Length::Rem(vb)) => Length::Rem(va + (vb - va) * t),
        (Length::Percent(va), Length::Percent(vb)) => Length::Percent(va + (vb - va) * t),
        (Length::Px(va), Length::Zero) | (Length::Zero, Length::Px(va)) => {
            Length::Px(if a == Length::Zero { va * t } else { va * (1.0 - t) })
        }
        _ => b, // Can't interpolate mismatched units — snap to end
    }
}

fn lerp_transform(a: TransformValue, b: TransformValue, t: f32) -> TransformValue {
    TransformValue {
        translate_x: a.translate_x + (b.translate_x - a.translate_x) * t,
        translate_y: a.translate_y + (b.translate_y - a.translate_y) * t,
        rotate_deg: a.rotate_deg + (b.rotate_deg - a.rotate_deg) * t,
        scale_x: a.scale_x + (b.scale_x - a.scale_x) * t,
        scale_y: a.scale_y + (b.scale_y - a.scale_y) * t,
    }
}

fn parse_transform_value(value: &str) -> Option<TransformValue> {
    let mut tv = TransformValue::default();
    let value = value.trim();
    if value == "none" || value.is_empty() {
        return Some(tv);
    }

    let mut remaining = value;
    while !remaining.is_empty() {
        remaining = remaining.trim_start();
        if let Some(rest) = remaining.strip_prefix("translateX(") {
            let (val, rest) = extract_paren_value(rest)?;
            tv.translate_x = parse_px_or_zero(&val);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("translateY(") {
            let (val, rest) = extract_paren_value(rest)?;
            tv.translate_y = parse_px_or_zero(&val);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("translate(") {
            let (val, rest) = extract_paren_value(rest)?;
            let parts: Vec<&str> = val.split([',', ' ']).map(str::trim).filter(|s| !s.is_empty()).collect();
            tv.translate_x = parts.first().map(|s| parse_px_or_zero(s)).unwrap_or(0.0);
            tv.translate_y = parts.get(1).map(|s| parse_px_or_zero(s)).unwrap_or(0.0);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("rotate(") {
            let (val, rest) = extract_paren_value(rest)?;
            tv.rotate_deg = parse_deg_or_zero(&val);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("scaleX(") {
            let (val, rest) = extract_paren_value(rest)?;
            tv.scale_x = val.trim().parse::<f32>().unwrap_or(1.0);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("scaleY(") {
            let (val, rest) = extract_paren_value(rest)?;
            tv.scale_y = val.trim().parse::<f32>().unwrap_or(1.0);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix("scale(") {
            let (val, rest) = extract_paren_value(rest)?;
            let parts: Vec<&str> = val.split([',', ' ']).map(str::trim).filter(|s| !s.is_empty()).collect();
            let sx = parts.first().and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0);
            let sy = parts.get(1).and_then(|s| s.parse::<f32>().ok()).unwrap_or(sx);
            tv.scale_x = sx;
            tv.scale_y = sy;
            remaining = rest;
        } else {
            // Skip unknown function
            if let Some(paren) = remaining.find('(') {
                if let Some(close) = remaining[paren..].find(')') {
                    remaining = &remaining[paren + close + 1..];
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }
    Some(tv)
}

fn extract_paren_value(s: &str) -> Option<(String, &str)> {
    let close = s.find(')')?;
    Some((s[..close].to_owned(), &s[close + 1..]))
}

fn parse_px_or_zero(s: &str) -> f32 {
    let s = s.trim().to_lowercase();
    let s = s.strip_suffix("px").unwrap_or(&s);
    s.trim().parse::<f32>().unwrap_or(0.0)
}

fn parse_deg_or_zero(s: &str) -> f32 {
    let s = s.trim().to_lowercase();
    let s = if let Some(stripped) = s.strip_suffix("deg") {
        stripped
    } else if let Some(stripped) = s.strip_suffix("turn") {
        return stripped.trim().parse::<f32>().unwrap_or(0.0) * 360.0;
    } else if let Some(stripped) = s.strip_suffix("rad") {
        return stripped.trim().parse::<f32>().unwrap_or(0.0) * 180.0 / std::f32::consts::PI;
    } else {
        &s
    };
    s.trim().parse::<f32>().unwrap_or(0.0)
}

// ── Active Animation State ──────────────────────────────────────────────────

/// State of a single running CSS animation on an element.
#[derive(Debug, Clone)]
pub struct ActiveAnimation {
    pub animation: SingleAnimation,
    pub elapsed: f32,
    pub iteration: f32,
    pub finished: bool,
}

impl ActiveAnimation {
    pub fn new(animation: SingleAnimation) -> Self {
        Self {
            animation,
            elapsed: 0.0,
            iteration: 0.0,
            finished: false,
        }
    }

    /// Advance the animation by `dt` seconds. Returns `true` if still running.
    pub fn tick(&mut self, dt: f32) -> bool {
        if self.finished || self.animation.play_state == AnimationPlayState::Paused {
            return false;
        }
        self.elapsed += dt;

        let total_duration = self.animation.duration.max(0.001);
        let delay = self.animation.delay;

        if self.elapsed < delay {
            return true; // Still in delay period
        }

        let active_time = self.elapsed - delay;
        self.iteration = active_time / total_duration;

        if self.animation.iteration_count >= 0.0 && self.iteration >= self.animation.iteration_count {
            self.iteration = self.animation.iteration_count;
            self.finished = true;
            return false;
        }
        true
    }

    /// Compute the current progress [0.0, 1.0] accounting for direction.
    pub fn progress(&self) -> f32 {
        let delay = self.animation.delay;
        if self.elapsed < delay {
            // In delay — use fill-mode to determine value
            return match self.animation.fill_mode {
                AnimationFillMode::Backwards | AnimationFillMode::Both => 0.0,
                _ => 0.0,
            };
        }

        let total_duration = self.animation.duration.max(0.001);
        let active_time = self.elapsed - delay;
        let raw_progress = (active_time / total_duration).min(1.0);
        let current_iteration = active_time / total_duration;
        let iteration_index = current_iteration.floor() as i32;

        match self.animation.direction {
            AnimationDirection::Normal => raw_progress,
            AnimationDirection::Reverse => 1.0 - raw_progress,
            AnimationDirection::Alternate => {
                if iteration_index % 2 == 0 {
                    raw_progress
                } else {
                    1.0 - raw_progress
                }
            }
            AnimationDirection::AlternateReverse => {
                if iteration_index % 2 == 0 {
                    1.0 - raw_progress
                } else {
                    raw_progress
                }
            }
        }
    }
}

/// State of a single CSS transition on a property.
#[derive(Debug, Clone)]
pub struct ActiveTransition {
    pub property: String,
    pub from: AnimatableValue,
    pub to: AnimatableValue,
    pub transition: SingleTransition,
    pub elapsed: f32,
    pub finished: bool,
}

impl ActiveTransition {
    pub fn new(property: String, from: AnimatableValue, to: AnimatableValue, transition: SingleTransition) -> Self {
        Self {
            property,
            from,
            to,
            transition,
            elapsed: 0.0,
            finished: false,
        }
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        if self.finished {
            return false;
        }
        self.elapsed += dt;

        let total_duration = self.transition.duration.max(0.001);
        let delay = self.transition.delay;

        if self.elapsed < delay {
            return true;
        }

        let active_time = self.elapsed - delay;
        if active_time >= total_duration {
            self.elapsed = delay + total_duration;
            self.finished = true;
            return false;
        }
        true
    }

    pub fn progress(&self) -> f32 {
        let delay = self.transition.delay;
        if self.elapsed < delay {
            return 0.0;
        }
        let total_duration = self.transition.duration.max(0.001);
        let active_time = self.elapsed - delay;
        (active_time / total_duration).min(1.0)
    }
}

// ── AnimationController ──────────────────────────────────────────────────────

/// Per-element animation controller. Tracks active animations and transitions,
/// and applies interpolated values to a `ComputedStyle` each frame.
#[derive(Debug, Clone)]
pub struct AnimationController {
    pub animations: Vec<ActiveAnimation>,
    pub transitions: Vec<ActiveTransition>,
}

impl Default for AnimationController {
    fn default() -> Self {
        Self::new()
    }
}

impl AnimationController {
    pub fn new() -> Self {
        Self {
            animations: Vec::new(),
            transitions: Vec::new(),
        }
    }

    /// Returns `true` if any animations or transitions are currently running.
    pub fn is_active(&self) -> bool {
        self.animations.iter().any(|a| !a.finished)
            || self.transitions.iter().any(|t| !t.finished)
    }

    /// Start a CSS animation using the given keyframes rule.
    pub fn start_animation(&mut self, animation: SingleAnimation) {
        // Don't duplicate if already running the same animation name
        if self.animations.iter().any(|a| a.animation.name == animation.name && !a.finished) {
            return;
        }
        self.animations.push(ActiveAnimation::new(animation));
    }

    /// Start a CSS transition on a property change.
    pub fn start_transition(
        &mut self,
        property: String,
        from: AnimatableValue,
        to: AnimatableValue,
        transition: SingleTransition,
    ) {
        // Cancel any existing transition on the same property
        self.transitions.retain(|t| t.property != property);
        self.transitions.push(ActiveTransition::new(property, from, to, transition));
    }

    /// Advance all animations and transitions by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        for anim in &mut self.animations {
            anim.tick(dt);
        }
        for trans in &mut self.transitions {
            trans.tick(dt);
        }
        // Clean up finished items (keep for fill-mode evaluation)
        self.animations.retain(|a| !a.finished || a.animation.fill_mode != AnimationFillMode::None);
        self.transitions.retain(|t| !t.finished);
    }

    /// Apply all active animation/transition values to a `ComputedStyle`.
    /// `keyframes_lookup` maps animation names to their `KeyframesRule`.
    pub fn apply_to_style(&self, style: &mut ComputedStyle, keyframes_lookup: &[KeyframesRule]) {
        // Apply transitions first (they have higher priority for their property)
        for trans in &self.transitions {
            let raw_progress = trans.progress();
            let eased = trans.transition.timing_function.apply(raw_progress);
            let value = trans.from.lerp(&trans.to, eased);
            value.apply_to(&trans.property, style);
        }

        // Apply animations (keyframe-based)
        for active in &self.animations {
            let progress = active.progress();
            let eased = active.animation.timing_function.apply(progress);

            // Find the keyframes rule
            let Some(rule) = keyframes_lookup.iter().find(|k| k.name == active.animation.name) else {
                continue;
            };

            // Find the two keyframes to interpolate between
            let mut sorted_keyframes: Vec<&crate::Keyframe> = rule.keyframes.iter().collect();
            sorted_keyframes.sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap());

            if sorted_keyframes.is_empty() {
                continue;
            }

            // Find surrounding keyframes
            let (from_kf, to_kf, local_t) = find_keyframe_pair(&sorted_keyframes, eased);

            // Interpolate each declared property
            let from_decls = &from_kf.declarations;
            let to_decls = &to_kf.declarations;

            for (prop, from_val) in from_decls {
                let to_val = to_decls.iter()
                    .find(|(p, _)| p == prop)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or(from_val);
                let from_anim = AnimatableValue::from_css_property(prop, from_val);
                let to_anim = AnimatableValue::from_css_property(prop, to_val);
                if from_anim != AnimatableValue::None && to_anim != AnimatableValue::None {
                    let interpolated = from_anim.lerp(&to_anim, local_t);
                    interpolated.apply_to(prop, style);
                }
            }
            // Also apply properties only in the "to" keyframe
            for (prop, to_val) in to_decls {
                if !from_decls.iter().any(|(p, _)| p == prop) {
                    let to_anim = AnimatableValue::from_css_property(prop, to_val);
                    if to_anim != AnimatableValue::None {
                        // Interpolate from the style's current value
                        let from_anim = AnimatableValue::from_computed_style(prop, style);
                        if from_anim != AnimatableValue::None {
                            let interpolated = from_anim.lerp(&to_anim, local_t);
                            interpolated.apply_to(prop, style);
                        } else {
                            to_anim.apply_to(prop, style);
                        }
                    }
                }
            }
        }
    }
}

/// Find the two keyframes surrounding the given progress value.
/// Returns (from_keyframe, to_keyframe, local_t_between_them).
fn find_keyframe_pair<'a>(
    sorted: &[&'a crate::Keyframe],
    progress: f32,
) -> (&'a crate::Keyframe, &'a crate::Keyframe, f32) {
    if sorted.len() == 1 {
        return (sorted[0], sorted[0], 1.0);
    }

    // Before first keyframe
    if progress <= sorted[0].offset {
        return (sorted[0], sorted[0], 0.0);
    }

    // After last keyframe
    if progress >= sorted.last().unwrap().offset {
        let last = sorted.last().unwrap();
        return (last, last, 1.0);
    }

    // Find the pair
    for i in 0..sorted.len() - 1 {
        let from = sorted[i];
        let to = sorted[i + 1];
        if progress >= from.offset && progress <= to.offset {
            let range = to.offset - from.offset;
            let local_t = if range > 0.0001 {
                (progress - from.offset) / range
            } else {
                1.0
            };
            return (from, to, local_t);
        }
    }

    let last = sorted.last().unwrap();
    (last, last, 1.0)
}

// ── requestAnimationFrame support ────────────────────────────────────────────

/// A simple frame scheduler for `requestAnimationFrame` callbacks.
/// The JS runtime registers callback IDs here, and the render loop
/// drains them each frame.
#[derive(Debug, Default)]
pub struct RafQueue {
    callbacks: Vec<u64>,
    next_id: u64,
}

impl RafQueue {
    pub fn new() -> Self {
        Self {
            callbacks: Vec::new(),
            next_id: 1,
        }
    }

    /// Register a requestAnimationFrame callback. Returns the callback ID
    /// (which can be passed to `cancelAnimationFrame`).
    pub fn request(&mut self, _callback_id: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.callbacks.push(_callback_id);
        id
    }

    /// Cancel a previously requested animation frame.
    pub fn cancel(&mut self, id: u64) {
        self.callbacks.retain(|&cb| cb != id);
    }

    /// Drain all pending callbacks for this frame. Returns callback IDs
    /// that the JS runtime should invoke.
    pub fn drain(&mut self) -> Vec<u64> {
        self.callbacks.drain(..).collect()
    }

    /// Check if there are pending callbacks.
    pub fn has_pending(&self) -> bool {
        !self.callbacks.is_empty()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_color_basic() {
        let black = Color::rgb(0, 0, 0);
        let white = Color::rgb(255, 255, 255);
        let mid = lerp_color(black, white, 0.5);
        assert_eq!(mid.r, 128);
        assert_eq!(mid.g, 128);
        assert_eq!(mid.b, 128);
    }

    #[test]
    fn lerp_length_px() {
        let a = Length::Px(10.0);
        let b = Length::Px(20.0);
        let mid = lerp_length(a, b, 0.5);
        assert_eq!(mid, Length::Px(15.0));
    }

    #[test]
    fn lerp_opacity() {
        let a = AnimatableValue::Opacity(0.0);
        let b = AnimatableValue::Opacity(1.0);
        let mid = a.lerp(&b, 0.25);
        assert_eq!(mid, AnimatableValue::Opacity(0.25));
    }

    #[test]
    fn lerp_transform_translate() {
        let a = TransformValue { translate_x: 0.0, translate_y: 0.0, ..Default::default() };
        let b = TransformValue { translate_x: 100.0, translate_y: 50.0, ..Default::default() };
        let mid = lerp_transform(a, b, 0.5);
        assert_eq!(mid.translate_x, 50.0);
        assert_eq!(mid.translate_y, 25.0);
    }

    #[test]
    fn parse_transform_translate() {
        let tv = parse_transform_value("translateX(10px) translateY(20px)").unwrap();
        assert_eq!(tv.translate_x, 10.0);
        assert_eq!(tv.translate_y, 20.0);
    }

    #[test]
    fn parse_transform_rotate() {
        let tv = parse_transform_value("rotate(45deg)").unwrap();
        assert_eq!(tv.rotate_deg, 45.0);
    }

    #[test]
    fn parse_transform_scale() {
        let tv = parse_transform_value("scale(2)").unwrap();
        assert_eq!(tv.scale_x, 2.0);
        assert_eq!(tv.scale_y, 2.0);
    }

    #[test]
    fn parse_transform_compound() {
        let tv = parse_transform_value("translateX(10px) rotate(90deg) scale(1.5)").unwrap();
        assert_eq!(tv.translate_x, 10.0);
        assert_eq!(tv.rotate_deg, 90.0);
        assert_eq!(tv.scale_x, 1.5);
    }

    #[test]
    fn animation_progress_normal_direction() {
        let mut anim = ActiveAnimation::new(SingleAnimation {
            name: "test".into(),
            duration: 1.0,
            timing_function: EasingFunction::Linear,
            delay: 0.0,
            iteration_count: 1.0,
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::None,
            play_state: AnimationPlayState::Running,
        });
        anim.elapsed = 0.5;
        let p = anim.progress();
        assert!((p - 0.5).abs() < 0.01);
    }

    #[test]
    fn animation_progress_reverse() {
        let mut anim = ActiveAnimation::new(SingleAnimation {
            name: "test".into(),
            duration: 1.0,
            timing_function: EasingFunction::Linear,
            delay: 0.0,
            iteration_count: 1.0,
            direction: AnimationDirection::Reverse,
            fill_mode: AnimationFillMode::None,
            play_state: AnimationPlayState::Running,
        });
        anim.elapsed = 0.5;
        let p = anim.progress();
        assert!((p - 0.5).abs() < 0.01, "progress was {p}");
    }

    #[test]
    fn animation_finishes_after_duration() {
        let mut anim = ActiveAnimation::new(SingleAnimation {
            name: "test".into(),
            duration: 1.0,
            timing_function: EasingFunction::Linear,
            delay: 0.0,
            iteration_count: 1.0,
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::None,
            play_state: AnimationPlayState::Running,
        });
        assert!(anim.tick(0.5));
        assert!(!anim.finished);
        assert!(!anim.tick(0.6)); // Past duration
        assert!(anim.finished);
    }

    #[test]
    fn transition_progress() {
        let mut trans = ActiveTransition::new(
            "opacity".into(),
            AnimatableValue::Opacity(0.0),
            AnimatableValue::Opacity(1.0),
            SingleTransition {
                property: "opacity".into(),
                duration: 2.0,
                timing_function: EasingFunction::Linear,
                delay: 0.0,
            },
        );
        trans.tick(1.0);
        let p = trans.progress();
        assert!((p - 0.5).abs() < 0.01);
    }

    #[test]
    fn controller_is_active() {
        let mut ctrl = AnimationController::new();
        assert!(!ctrl.is_active());
        ctrl.start_animation(SingleAnimation {
            name: "fade".into(),
            duration: 1.0,
            ..Default::default()
        });
        assert!(ctrl.is_active());
    }

    #[test]
    fn controller_apply_transition() {
        let mut ctrl = AnimationController::new();
        ctrl.start_transition(
            "opacity".into(),
            AnimatableValue::Opacity(0.0),
            AnimatableValue::Opacity(1.0),
            SingleTransition {
                property: "opacity".into(),
                duration: 1.0,
                timing_function: EasingFunction::Linear,
                delay: 0.0,
            },
        );
        ctrl.tick(0.5);

        let mut style = ComputedStyle::initial();
        style.opacity = 0.0;
        ctrl.apply_to_style(&mut style, &[]);
        assert!((style.opacity - 0.5).abs() < 0.1);
    }

    #[test]
    fn controller_apply_keyframe_animation() {
        let keyframes = vec![crate::KeyframesRule {
            name: "fadeIn".into(),
            keyframes: vec![
                crate::Keyframe { offset: 0.0, declarations: vec![("opacity".into(), "0".into())] },
                crate::Keyframe { offset: 1.0, declarations: vec![("opacity".into(), "1".into())] },
            ],
        }];

        let mut ctrl = AnimationController::new();
        ctrl.start_animation(SingleAnimation {
            name: "fadeIn".into(),
            duration: 1.0,
            timing_function: EasingFunction::Linear,
            delay: 0.0,
            iteration_count: 1.0,
            direction: AnimationDirection::Normal,
            fill_mode: AnimationFillMode::Both,
            play_state: AnimationPlayState::Running,
        });
        ctrl.tick(0.5);

        let mut style = ComputedStyle::initial();
        ctrl.apply_to_style(&mut style, &keyframes);
        assert!((style.opacity - 0.5).abs() < 0.15, "opacity was {}", style.opacity);
    }

    #[test]
    fn raf_queue_request_and_drain() {
        let mut raf = RafQueue::new();
        assert!(!raf.has_pending());
        raf.request(1);
        raf.request(2);
        assert!(raf.has_pending());
        let callbacks = raf.drain();
        assert_eq!(callbacks, vec![1, 2]);
        assert!(!raf.has_pending());
    }

    #[test]
    fn easing_function_apply() {
        assert_eq!(EasingFunction::Linear.apply(0.5), 0.5);
        assert_eq!(EasingFunction::Linear.apply(0.0), 0.0);
        assert_eq!(EasingFunction::Linear.apply(1.0), 1.0);

        // Step functions
        assert_eq!(EasingFunction::StepStart.apply(0.5), 0.0);
        assert_eq!(EasingFunction::StepEnd.apply(0.5), 1.0);
    }

    #[test]
    fn animatable_value_from_css() {
        let v = AnimatableValue::from_css_property("opacity", "0.5");
        assert_eq!(v, AnimatableValue::Opacity(0.5));

        let v = AnimatableValue::from_css_property("color", "rgb(255, 0, 0)");
        assert_eq!(v, AnimatableValue::Color(Color::rgb(255, 0, 0)));

        let v = AnimatableValue::from_css_property("width", "100px");
        assert_eq!(v, AnimatableValue::Length(Length::Px(100.0)));
    }

    #[test]
    fn apply_to_style_opacity() {
        let mut style = ComputedStyle::initial();
        let val = AnimatableValue::Opacity(0.75);
        val.apply_to("opacity", &mut style);
        assert_eq!(style.opacity, 0.75);
    }

    #[test]
    fn apply_to_style_color() {
        let mut style = ComputedStyle::initial();
        let val = AnimatableValue::Color(Color::rgb(128, 64, 32));
        val.apply_to("color", &mut style);
        assert_eq!(style.color, Color::rgb(128, 64, 32));
    }

    #[test]
    fn keyframe_pair_finding() {
        let kf0 = crate::Keyframe { offset: 0.0, declarations: vec![] };
        let kf50 = crate::Keyframe { offset: 0.5, declarations: vec![] };
        let kf100 = crate::Keyframe { offset: 1.0, declarations: vec![] };
        let sorted = vec![&kf0, &kf50, &kf100];

        let (from, to, t) = find_keyframe_pair(&sorted, 0.25);
        assert_eq!(from.offset, 0.0);
        assert_eq!(to.offset, 0.5);
        assert!((t - 0.5).abs() < 0.01);

        let (from, to, t) = find_keyframe_pair(&sorted, 0.75);
        assert_eq!(from.offset, 0.5);
        assert_eq!(to.offset, 1.0);
        assert!((t - 0.5).abs() < 0.01);
    }
}
