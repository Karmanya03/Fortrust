use fortrust_dom::{Document, DomArena, DomError, NodeRef, parse_html};
use fortrust_layout::{LayoutConstraints, LayoutEngine, LayoutTree, Rect};
use fortrust_paint::{DisplayList, PaintOptions, Painter};
use fortrust_style::{StyleEngine, StyleError, Stylesheet};

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
        let arena = DomArena::new();
        let document = parse_html(&arena, html)?;
        self.render_document(&document, author_css, viewport)
    }

    pub fn render_document(
        &self,
        document: &Document<'_>,
        author_css: &[&str],
        viewport: Viewport,
    ) -> Result<RenderedPage, RenderError> {
        let mut style = StyleEngine::new();
        for embedded_css in embedded_styles(document) {
            style.add_stylesheet(Stylesheet::parse(&embedded_css)?);
        }
        for css in author_css {
            style.add_stylesheet(Stylesheet::parse(css)?);
        }

        let root = render_root(document).ok_or(RenderError::EmptyDocument)?;
        let layout = LayoutEngine::new(style)
            .layout(
                root,
                LayoutConstraints {
                    viewport_width: viewport.width,
                    viewport_height: viewport.height,
                    containing_block: None,
                },
            )
            .ok_or(RenderError::EmptyDocument)?;
        let display_list = self.painter.paint(
            &layout,
            PaintOptions {
                viewport: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: viewport.width,
                    height: viewport.height,
                },
                include_debug_borders: false,
            },
        );

        Ok(RenderedPage {
            layout,
            display_list,
            text_content: document.text_content(),
            parse_error_count: document.parse_errors.len(),
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
}
