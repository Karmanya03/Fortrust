use std::cell::RefCell;
use std::collections::HashMap;

use fortrust_core::ImageRegistry;
use fortrust_dom::{Document, DomArena, DomError, NodeRef, parse_html};
use fortrust_layout::{LayoutConstraints, LayoutEngine, LayoutTree, Rect};
use fortrust_paint::{DisplayList, PaintOptions, Painter};
use fortrust_style::animation::AnimationController;
use fortrust_style::{Color, StyleEngine, StyleError, Stylesheet};

#[derive(Debug, Clone, PartialEq)]
pub enum RenderError {
    Dom(DomError),
    Style(StyleError),
    EmptyDocument,
}

impl From<DomError> for RenderError {
    fn from(error: DomError) -> Self {
        Self::Dom(error)
    }
}

impl From<StyleError> for RenderError {
    fn from(error: StyleError) -> Self {
        Self::Style(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone)]
pub struct RenderedPage {
    pub layout: LayoutTree,
    pub display_list: DisplayList,
    pub text_content: String,
    pub parse_error_count: usize,
    /// CSS injected by cosmetic filtering rules (ad blocking element hiding, etc.)
    pub injected_css: Vec<String>,
    /// Decoded images referenced by the page. Index = the `image_ref` stored on
    /// layout boxes and emitted in `DrawImage` paint commands.
    pub images: ImageRegistry,
}

#[derive(Debug, Clone, Default)]
pub struct StaticRenderer {
    painter: Painter,
    /// Cached state for animation-driven re-rendering.
    anim_state: RefCell<Option<Box<AnimRenderCache>>>,
}

/// Cached state for animation-driven re-rendering.
#[derive(Clone)]
struct AnimRenderCache {
    #[allow(dead_code)]
    author_css: Vec<String>,
    cosmetic_css: Vec<String>,
    viewport: Viewport,
    images: ImageRegistry,
    layout: LayoutTree,
    viewport_fill: Color,
    clock: f32,
    controllers: HashMap<String, AnimationController>,
    keyframes: Vec<fortrust_style::KeyframesRule>,
}

impl std::fmt::Debug for AnimRenderCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimRenderCache")
            .field("viewport", &self.viewport)
            .field("clock", &self.clock)
            .field("controller_count", &self.controllers.len())
            .field("keyframe_count", &self.keyframes.len())
            .finish()
    }
}

impl StaticRenderer {
    pub fn new() -> Self {
        Self {
            painter: Painter::new(),
            anim_state: RefCell::new(None),
        }
    }

    pub fn render(
        &self,
        html: &str,
        author_css: &[&str],
        viewport: Viewport,
    ) -> Result<RenderedPage, RenderError> {
        self.render_with_images(html, author_css, viewport, ImageRegistry::new())
    }

    pub fn render_with_images(
        &self,
        html: &str,
        author_css: &[&str],
        viewport: Viewport,
        images: ImageRegistry,
    ) -> Result<RenderedPage, RenderError> {
        let arena = DomArena::new();
        let document = parse_html(&arena, html)?;
        self.render_document_with_images(&document, author_css, &[], viewport, images)
    }

    pub fn render_document(
        &self,
        document: &Document<'_>,
        author_css: &[&str],
        cosmetic_css: &[&str],
        viewport: Viewport,
    ) -> Result<RenderedPage, RenderError> {
        self.render_document_with_images(document, author_css, cosmetic_css, viewport, ImageRegistry::new())
    }

    pub fn render_document_with_images(
        &self,
        document: &Document<'_>,
        author_css: &[&str],
        cosmetic_css: &[&str],
        viewport: Viewport,
        images: ImageRegistry,
    ) -> Result<RenderedPage, RenderError> {
        let mut style = StyleEngine::new();
        for embedded_css in embedded_styles(document) {
            style.add_stylesheet(Stylesheet::parse(&embedded_css)?);
        }
        for css in author_css {
            style.add_stylesheet(Stylesheet::parse(css)?);
        }
        for css in cosmetic_css {
            style.add_stylesheet(Stylesheet::parse(css)?);
        }

        // Determine viewport background: prefer html, then body, then transparent
        let viewport_fill = document.first_element_by_tag("html")
            .map(|n| style.compute_style(n, None))
            .filter(|s| s.background_color.a > 0)
            .map(|s| s.background_color)
            .or_else(|| {
                document.first_element_by_tag("body")
                    .map(|n| style.compute_style(n, None))
                    .filter(|s| s.background_color.a > 0)
                    .map(|s| s.background_color)
            })
            .unwrap_or(Color::TRANSPARENT);

        let root = render_root(document).ok_or(RenderError::EmptyDocument)?;
        let layout = LayoutEngine::new(style)
            .layout(
                root,
                LayoutConstraints {
                    viewport_width: viewport.width,
                    viewport_height: viewport.height,
                    containing_block: None,
                },
                &images,
            )
            .ok_or(RenderError::EmptyDocument)?;

        let display_list = self.painter.paint(
            &layout,
            &images,
            PaintOptions {
                viewport: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: viewport.width,
                    height: viewport.height,
                },
                include_debug_borders: false,
                viewport_fill,
            },
        );

        Ok(RenderedPage {
            layout,
            display_list,
            text_content: document.text_content(),
            parse_error_count: document.parse_errors.len(),
            injected_css: cosmetic_css.iter().map(|&s| s.to_owned()).collect(),
            images,
        })
    }

    /// Re-render the document only if the DOM has been mutated (dirty flags set).
    /// Returns `Some(RenderedPage)` if a re-render was performed, `None` if the
    /// document was clean and no re-render was needed.
    pub fn render_if_dirty(
        &self,
        document: &Document<'_>,
        author_css: &[&str],
        cosmetic_css: &[&str],
        viewport: Viewport,
        images: ImageRegistry,
    ) -> Result<Option<RenderedPage>, RenderError> {
        if !document.is_dirty() {
            return Ok(None);
        }
        let page = self.render_document_with_images(
            document, author_css, cosmetic_css, viewport, images,
        )?;
        document.clear_dirty();
        Ok(Some(page))
    }

    /// Render with damage tracking: only repaints regions affected by dirty subtrees.
    /// Returns the damage rectangles that were repainted, suitable for
    /// partial framebuffer updates.
    pub fn render_with_damage_tracking(
        &self,
        document: &Document<'_>,
        author_css: &[&str],
        cosmetic_css: &[&str],
        viewport: Viewport,
        images: ImageRegistry,
    ) -> Result<(RenderedPage, Vec<Rect>), RenderError> {
        let dirty_roots = document.dirty_subtree_roots();
        if dirty_roots.is_empty() {
            // No dirty nodes — render everything as baseline
            let page = self.render_document_with_images(
                document, author_css, cosmetic_css, viewport, images,
            )?;
            let full_viewport = Rect {
                x: 0.0, y: 0.0, width: viewport.width, height: viewport.height,
            };
            document.clear_dirty();
            return Ok((page, vec![full_viewport]));
        }

        // Full re-render (style + layout must be recalculated for correct
        // positioning even of unchanged nodes). But we track damage rects
        // from the dirty subtrees' layout boxes.
        let page = self.render_document_with_images(
            document, author_css, cosmetic_css, viewport, images,
        )?;

        // Collect damage rectangles from the layout tree by finding
        // layout boxes whose node_name matches dirty subtree roots.
        let mut damage_rects = Vec::new();
        for dirty_node in &dirty_roots {
            let tag = dirty_node.as_element()
                .map(|el| el.local_name().to_owned())
                .unwrap_or_else(|| "#text".to_owned());
            collect_damage_rects(&page.layout.root, &tag, &mut damage_rects);
        }

        // If we couldn't find matching layout boxes, damage the full viewport
        if damage_rects.is_empty() {
            damage_rects.push(Rect {
                x: 0.0, y: 0.0, width: viewport.width, height: viewport.height,
            });
        }

        document.clear_dirty();
        Ok((page, damage_rects))
    }

    /// Render and cache animation state. After this, `tick_animations` can be
    /// called to advance animation time and produce updated frames.
    pub fn render_with_animation_cache(
        &self,
        html: &str,
        author_css: &[&str],
        cosmetic_css: &[&str],
        viewport: Viewport,
        images: ImageRegistry,
    ) -> Result<RenderedPage, RenderError> {
        let arena = DomArena::new();
        let document = parse_html(&arena, html)?;

        let mut style = StyleEngine::new();
        for embedded_css in embedded_styles(&document) {
            style.add_stylesheet(Stylesheet::parse(&embedded_css)?);
        }
        for css in author_css {
            style.add_stylesheet(Stylesheet::parse(css)?);
        }
        for css in cosmetic_css {
            style.add_stylesheet(Stylesheet::parse(css)?);
        }

        let viewport_fill = document
            .first_element_by_tag("html")
            .map(|n| style.compute_style(n, None))
            .filter(|s| s.background_color.a > 0)
            .map(|s| s.background_color)
            .or_else(|| {
                document
                    .first_element_by_tag("body")
                    .map(|n| style.compute_style(n, None))
                    .filter(|s| s.background_color.a > 0)
                    .map(|s| s.background_color)
            })
            .unwrap_or(Color::TRANSPARENT);

        let root = render_root(&document).ok_or(RenderError::EmptyDocument)?;

        let clock = 0.0;
        let keyframes = style.keyframes.clone();

        let layout = LayoutEngine::new(style)
            .layout(
                root,
                LayoutConstraints {
                    viewport_width: viewport.width,
                    viewport_height: viewport.height,
                    containing_block: None,
                },
                &images,
            )
            .ok_or(RenderError::EmptyDocument)?;

        // Build animation controllers from the layout tree (not DOM), so
        // `tag_name#counter` keys match during tick_animations.
        let controllers = scan_controllers(&layout);

        let display_list = self.painter.paint(
            &layout,
            &images,
            PaintOptions {
                viewport: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: viewport.width,
                    height: viewport.height,
                },
                include_debug_borders: false,
                viewport_fill,
            },
        );

        let page = RenderedPage {
            layout: layout.clone(),
            display_list,
            text_content: document.text_content(),
            parse_error_count: document.parse_errors.len(),
            injected_css: cosmetic_css.iter().map(|&s| s.to_owned()).collect(),
            images: images.clone(),
        };

        *self.anim_state.borrow_mut() = Some(Box::new(AnimRenderCache {
            author_css: author_css.iter().map(|&s| s.to_owned()).collect(),
            cosmetic_css: cosmetic_css.iter().map(|&s| s.to_owned()).collect(),
            viewport,
            images,
            layout,
            viewport_fill,
            clock,
            controllers,
            keyframes,
        }));

        Ok(page)
    }

    /// Advance animation time and re-render if any animations are active.
    /// Returns `Some(RenderedPage)` if the display list changed, `None` if no
    /// animations were active and nothing was re-rendered.
    pub fn tick_animations(&self, dt: f32) -> Option<RenderedPage> {
        let mut cache_borrow = self.anim_state.borrow_mut();
        let cache = cache_borrow.as_mut()?;
        cache.clock += dt;

        let mut any_active = false;
        for controller in cache.controllers.values_mut() {
            controller.tick(dt);
            if controller.is_active() {
                any_active = true;
            }
        }
        if any_active {
            apply_controllers_to_layout(
                &mut cache.layout,
                &cache.controllers,
                &cache.keyframes,
            );
        }

        if !any_active {
            return None;
        }

        let display_list = self.painter.paint(
            &cache.layout,
            &cache.images,
            PaintOptions {
                viewport: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: cache.viewport.width,
                    height: cache.viewport.height,
                },
                include_debug_borders: false,
                viewport_fill: cache.viewport_fill,
            },
        );

        Some(RenderedPage {
            layout: cache.layout.clone(),
            display_list,
            text_content: String::new(),
            parse_error_count: 0,
            injected_css: cache.cosmetic_css.clone(),
            images: cache.images.clone(),
        })
    }

    /// Returns true if there are active animations that need ticking.
    pub fn has_active_animations(&self) -> bool {
        self.anim_state
            .borrow()
            .as_ref()
            .is_some_and(|c| c.controllers.values().any(|ctrl| ctrl.is_active()))
    }

    /// Clear cached animation state.
    pub fn clear_animation_cache(&self) {
        *self.anim_state.borrow_mut() = None;
    }
}

/// Scans a layout tree for elements with CSS `animation` or `transition`
/// properties and creates `AnimationController` entries.
/// Uses `tag_name#counter` keys to support multiple elements with the same tag.
fn scan_controllers(layout: &LayoutTree) -> HashMap<String, AnimationController> {
    let mut controllers = HashMap::new();
    let mut counters: HashMap<String, usize> = HashMap::new();
    scan_box(&layout.root, &mut counters, &mut controllers);
    controllers
}

fn scan_box(
    box_: &fortrust_layout::LayoutBox,
    counters: &mut HashMap<String, usize>,
    controllers: &mut HashMap<String, AnimationController>,
) {
    if !box_.style.animations.is_empty() || !box_.style.transitions.is_empty() {
        let count = counters.entry(box_.node_name.clone()).or_insert(0);
        let key = format!("{}#{}", box_.node_name, count);
        *count += 1;

        let mut controller = AnimationController::new();
        for anim in &box_.style.animations {
            controller.start_animation(anim.clone());
        }
        for trans in &box_.style.transitions {
            let property = &trans.property;
            let val = fortrust_style::animation::AnimatableValue::from_computed_style(
                property,
                &box_.style,
            );
            controller.start_transition(
                property.clone(),
                val.clone(),
                val,
                trans.clone(),
            );
        }
        controllers.insert(key, controller);
    }
    for child in &box_.children {
        scan_box(child, counters, controllers);
    }
}

/// Apply AnimationControllers to the layout tree using `tag_name#counter` keys.
fn apply_controllers_to_layout(
    layout: &mut LayoutTree,
    controllers: &HashMap<String, AnimationController>,
    keyframes: &[fortrust_style::KeyframesRule],
) {
    let mut counters: HashMap<String, usize> = HashMap::new();
    apply_to_box_animated(&mut layout.root, &mut counters, controllers, keyframes);
}

fn apply_to_box_animated(
    box_: &mut fortrust_layout::LayoutBox,
    counters: &mut HashMap<String, usize>,
    controllers: &HashMap<String, AnimationController>,
    keyframes: &[fortrust_style::KeyframesRule],
) {
    let count = counters.entry(box_.node_name.clone()).or_insert(0);
    let key = format!("{}#{}", box_.node_name, count);
    *count += 1;

    if let Some(controller) = controllers.get(&key) {
        controller.apply_to_style(&mut box_.style, keyframes);
    }
    for child in &mut box_.children {
        apply_to_box_animated(child, counters, controllers, keyframes);
    }
}

fn render_root<'arena>(document: &Document<'arena>) -> Option<NodeRef<'arena>> {
    document
        .first_element_by_tag("body")
        .or_else(|| document.first_element_by_tag("html"))
        .or_else(|| {
            document
                .descendants()
                .into_iter()
                .find(|node| node.as_element().is_some())
        })
}

fn embedded_styles(document: &Document<'_>) -> Vec<String> {
    document
        .descendants()
        .into_iter()
        .filter(|node| {
            node.as_element()
                .is_some_and(|element| element.local_name().eq_ignore_ascii_case("style"))
        })
        .map(|node| node.text_content())
        .filter(|css| !css.trim().is_empty())
        .collect()
}

/// Walk the layout tree and collect rects of boxes matching the given node name.
/// Used by damage tracking to identify regions that need repainting.
fn collect_damage_rects(layout_box: &fortrust_layout::LayoutBox, node_name: &str, out: &mut Vec<Rect>) {
    if layout_box.node_name.eq_ignore_ascii_case(node_name)
        && layout_box.rect.width > 0.0 && layout_box.rect.height > 0.0 {
            out.push(layout_box.rect);
        }
    for child in &layout_box.children {
        collect_damage_rects(child, node_name, out);
    }
}

#[cfg(test)]
mod tests {
    use fortrust_core::{DecodedImage, ImageRegistry};
    use fortrust_paint::DisplayCommand;
    use fortrust_style::Color;

    use super::*;

    #[test]
    fn renders_static_html_into_display_list() {
        let page = StaticRenderer::new()
            .render(
                "<body><main>Hello <strong>Fortrust</strong></main></body>",
                &["main { background-color: #eeeeee; padding: 4px; }"],
                Viewport {
                    width: 320.0,
                    height: 240.0,
                },
            )
            .unwrap();

        assert!(page.text_content.contains("Hello"));
        assert!(page.display_list.commands().iter().any(|command| matches!(
            command,
            DisplayCommand::DrawText { text, .. } if text == "Fortrust"
        )));
    }

    #[test]
    fn applies_author_css_through_pipeline() {
        let page = StaticRenderer::new()
            .render(
                "<body><p>Private</p></body>",
                &["p { color: blue; background-color: red; height: 20px; }"],
                Viewport {
                    width: 320.0,
                    height: 240.0,
                },
            )
            .unwrap();

        assert!(page.display_list.commands().iter().any(|command| matches!(
            command,
            DisplayCommand::DrawText {
                text,
                color: Color { r: 0, g: 0, b: 255, a: 255 },
                ..
            } if text == "Private"
        )));
    }

    #[test]
    fn reports_style_errors_before_layout() {
        let error = StaticRenderer::new()
            .render(
                "<body>Hello</body>",
                &["body { color: red;"],
                Viewport {
                    width: 320.0,
                    height: 240.0,
                },
            )
            .unwrap_err();

        assert_eq!(error, RenderError::Style(StyleError::UnclosedRule));
    }

    #[test]
    fn applies_embedded_style_elements() {
        let page = StaticRenderer::new()
            .render(
                "<head><style>p { color: blue; }</style></head><body><p>Styled</p></body>",
                &[],
                Viewport {
                    width: 320.0,
                    height: 240.0,
                },
            )
            .unwrap();

        assert!(page.display_list.commands().iter().any(|command| matches!(
            command,
            DisplayCommand::DrawText {
                text,
                color: Color { r: 0, g: 0, b: 255, a: 255 },
                ..
            } if text == "Styled"
        )));
    }

    #[test]
    fn renders_borders_from_css() {
        let page = StaticRenderer::new()
            .render(
                r#"<body><div style="border: 2px dashed #ff0000; width: 80px; height: 40px;">Box</div></body>"#,
                &[],
                Viewport {
                    width: 320.0,
                    height: 240.0,
                },
            )
            .unwrap();

        let has_border = page.display_list.commands().iter().any(|cmd| {
            matches!(
                cmd,
                DisplayCommand::DrawBorder { top_width, .. } if *top_width > 0.0
            )
        });
        assert!(has_border, "Expected DrawBorder command from CSS border property");
    }

    #[test]
    fn renders_decoded_image_via_drawimage_command() {
        let mut images = ImageRegistry::new();
        // 2x2 red RGBA image
        let rgba = vec![255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255];
        images.insert(DecodedImage {
            url: "https://example.com/pixel.png".to_owned(),
            width: 2,
            height: 2,
            rgba,
        });

        let page = StaticRenderer::new()
            .render_with_images(
                r#"<body><img src="https://example.com/pixel.png" width="40" height="20"></body>"#,
                &[],
                Viewport { width: 320.0, height: 240.0 },
                images,
            )
            .unwrap();

        let has_image_cmd = page.display_list.commands().iter().any(|cmd| {
            matches!(cmd, DisplayCommand::DrawImage { image_id, natural_width, natural_height, .. }
                if *image_id == 0 && *natural_width == 2 && *natural_height == 2)
        });
        assert!(has_image_cmd, "Expected DrawImage for the decoded <img>");
    }

    #[test]
    fn falls_back_to_placeholder_when_image_is_missing() {
        let page = StaticRenderer::new()
            .render(
                r#"<body><img src="https://missing.example/x.png" alt="Logo" width="40" height="20"></body>"#,
                &[],
                Viewport { width: 320.0, height: 240.0 },
            )
            .unwrap();

        let has_placeholder = page.display_list.commands().iter().any(|cmd| {
            matches!(cmd, DisplayCommand::DrawText { text, .. } if text.contains("Logo"))
        });
        assert!(has_placeholder, "Expected alt-text placeholder when image is absent");
    }

    #[test]
    fn layout_uses_image_natural_size_when_no_explicit_dimensions() {
        let mut images = ImageRegistry::new();
        // 100x50 green image
        let rgba = vec![0u8; 100 * 50 * 4];
        images.insert(DecodedImage {
            url: "https://example.com/banner.png".to_owned(),
            width: 100,
            height: 50,
            rgba,
        });

        let page = StaticRenderer::new()
            .render_with_images(
                r#"<body><img src="https://example.com/banner.png"></body>"#,
                &[],
                Viewport { width: 320.0, height: 240.0 },
                images,
            )
            .unwrap();

        // The img box should be 100x50 (natural) — not 300x150 (default fallback)
        let img_box = &page.layout.root.children[0];
        assert_eq!(img_box.rect.width, 100.0);
        assert_eq!(img_box.rect.height, 50.0);
    }

    #[test]
    fn render_if_dirty_skips_clean_document() {
        let arena = DomArena::new();
        let document = parse_html(&arena, "<body><p>Clean</p></body>").unwrap();
        document.clear_dirty();

        let renderer = StaticRenderer::new();
        let result = renderer.render_if_dirty(
            &document, &[], &[],
            Viewport { width: 320.0, height: 240.0 },
            ImageRegistry::new(),
        ).unwrap();

        // Document was clean — no re-render needed
        assert!(result.is_none());
    }

    #[test]
    fn render_if_dirty_rerenders_mutated_document() {
        let arena = DomArena::new();
        let document = parse_html(&arena, "<body><p>Original</p></body>").unwrap();

        // Mutate the DOM — this marks nodes dirty
        let p = document.first_element_by_tag("p").unwrap();
        p.set_text_content(&arena, "Mutated");

        assert!(document.is_dirty());

        let renderer = StaticRenderer::new();
        let result = renderer.render_if_dirty(
            &document, &[], &[],
            Viewport { width: 320.0, height: 240.0 },
            ImageRegistry::new(),
        ).unwrap();

        // Document was dirty — re-render performed
        assert!(result.is_some());
        let page = result.unwrap();
        assert!(page.text_content.contains("Mutated"));

        // After re-render, dirty flags should be cleared
        assert!(!document.is_dirty());
    }

    #[test]
    fn damage_tracking_returns_rects_for_dirty_subtrees() {
        let arena = DomArena::new();
        let document = parse_html(
            &arena,
            "<body><div id=\"a\">A</div><div id=\"b\">B</div></body>",
        ).unwrap();

        // Mutate only the second div
        let div_b = document.get_element_by_id("b").unwrap();
        div_b.set_text_content(&arena, "B-modified");

        let renderer = StaticRenderer::new();
        let (page, damage_rects) = renderer.render_with_damage_tracking(
            &document, &[], &[],
            Viewport { width: 320.0, height: 240.0 },
            ImageRegistry::new(),
        ).unwrap();

        // Should have at least one damage rect
        assert!(!damage_rects.is_empty());
        // Page should contain the mutated text
        assert!(page.text_content.contains("B-modified"));
    }
}
