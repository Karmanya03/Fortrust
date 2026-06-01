use fortrust_core::ImageRegistry;
use fortrust_dom::{Document, DomArena, DomError, NodeRef, parse_html};
use fortrust_layout::{LayoutConstraints, LayoutEngine, LayoutTree, Rect};
use fortrust_paint::{DisplayList, PaintOptions, Painter};
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
}

impl StaticRenderer {
    pub fn new() -> Self {
        Self {
            painter: Painter::new(),
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
}
