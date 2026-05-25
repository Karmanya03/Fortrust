use fortrust_layout::{BoxKind, LayoutBox, LayoutTree, Rect};
use fortrust_style::{Color, FontWeight, Length};

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayCommand {
    FillRect {
        rect: Rect,
        color: Color,
    },
    DrawText {
        rect: Rect,
        text: String,
        color: Color,
        font_size_px: f32,
        font_weight: FontWeight,
    },
    ClipPush(Rect),
    ClipPop,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayList {
    commands: Vec<DisplayCommand>,
}

impl DisplayList {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, command: DisplayCommand) {
        self.commands.push(command);
    }

    pub fn commands(&self) -> &[DisplayCommand] {
        &self.commands
    }

    pub fn into_commands(self) -> Vec<DisplayCommand> {
        self.commands
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintOptions {
    pub viewport: Rect,
    pub include_debug_borders: bool,
}

impl PaintOptions {
    pub fn viewport(width: f32, height: f32) -> Self {
        Self {
            viewport: Rect {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            include_debug_borders: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Painter;

impl Painter {
    pub fn new() -> Self {
        Self
    }

    pub fn paint(&self, tree: &LayoutTree, options: PaintOptions) -> DisplayList {
        let mut list = DisplayList::new();
        list.push(DisplayCommand::ClipPush(options.viewport));
        paint_box(&tree.root, &mut list, options);
        list.push(DisplayCommand::ClipPop);
        list
    }
}

fn paint_box(layout_box: &LayoutBox, list: &mut DisplayList, options: PaintOptions) {
    if !is_visible_rect(layout_box.rect) {
        return;
    }

    if layout_box.style.background_color.a > 0 {
        list.push(DisplayCommand::FillRect {
            rect: layout_box.rect,
            color: layout_box.style.background_color,
        });
    }

    if options.include_debug_borders && layout_box.kind != BoxKind::Text {
        paint_debug_border(layout_box.rect, list);
    }

    if layout_box.kind == BoxKind::Text
        && let Some(text) = &layout_box.text
        && !text.is_empty()
    {
        list.push(DisplayCommand::DrawText {
            rect: layout_box.rect,
            text: text.clone(),
            color: layout_box.style.color,
            font_size_px: font_size_px(layout_box.style.font_size),
            font_weight: layout_box.style.font_weight,
        });
    }

    if layout_box.kind == BoxKind::Replaced {
        let placeholder = layout_box
            .text
            .as_deref()
            .filter(|text| !text.is_empty())
            .map(|alt| format!("[image: {alt}]"))
            .unwrap_or_else(|| "[image]".to_owned());

        list.push(DisplayCommand::DrawText {
            rect: layout_box.rect,
            text: placeholder,
            color: layout_box.style.color,
            font_size_px: font_size_px(layout_box.style.font_size).min(layout_box.rect.height),
            font_weight: layout_box.style.font_weight,
        });
    }

    for child in &layout_box.children {
        paint_box(child, list, options);
    }
}

fn paint_debug_border(rect: Rect, list: &mut DisplayList) {
    let color = Color::rgb(80, 214, 184);
    let stroke = 1.0;
    let top = Rect {
        height: stroke,
        ..rect
    };
    let bottom = Rect {
        y: rect.y + rect.height - stroke,
        height: stroke,
        ..rect
    };
    let left = Rect {
        width: stroke,
        ..rect
    };
    let right = Rect {
        x: rect.x + rect.width - stroke,
        width: stroke,
        ..rect
    };

    for edge in [top, right, bottom, left] {
        if is_visible_rect(edge) {
            list.push(DisplayCommand::FillRect { rect: edge, color });
        }
    }
}

fn font_size_px(length: Length) -> f32 {
    match length {
        Length::Px(value) => value,
        Length::Em(value) => value * 16.0,
        Length::Percent(value) => 16.0 * value / 100.0,
        Length::Auto | Length::None => 16.0,
        _ => 16.0,
    }
}

fn is_visible_rect(rect: Rect) -> bool {
    rect.width > 0.0 && rect.height > 0.0
}

#[cfg(test)]
mod tests {
    use fortrust_dom::{DomArena, parse_html};
    use fortrust_layout::{LayoutConstraints, LayoutEngine};
    use fortrust_style::{StyleEngine, Stylesheet};

    use super::*;

    #[test]
    fn paints_background_before_text() {
        let arena = DomArena::new();
        let document = parse_html(
            &arena,
            r#"<body><p style="background-color: #112233; color: white">Hello</p></body>"#,
        )
        .unwrap();
        let body = document.first_element_by_tag("body").unwrap();
        let tree = LayoutEngine::new(StyleEngine::new())
            .layout(
                body,
                LayoutConstraints {
                    viewport_width: 320.0,
                    viewport_height: 240.0,
                    containing_block: None,
                },
            )
            .unwrap();

        let list = Painter::new().paint(&tree, PaintOptions::viewport(320.0, 240.0));
        let commands = list.commands();
        let fill_index = commands
            .iter()
            .position(|command| matches!(command, DisplayCommand::FillRect { .. }))
            .unwrap();
        let text_index = commands
            .iter()
            .position(|command| matches!(command, DisplayCommand::DrawText { text, .. } if text == "Hello"))
            .unwrap();

        assert!(fill_index < text_index);
    }

    #[test]
    fn text_command_uses_computed_text_style() {
        let arena = DomArena::new();
        let document = parse_html(&arena, r#"<body><strong>Secure</strong></body>"#).unwrap();
        let body = document.first_element_by_tag("body").unwrap();
        let tree = LayoutEngine::new(StyleEngine::new())
            .layout(
                body,
                LayoutConstraints {
                    viewport_width: 320.0,
                    viewport_height: 240.0,
                    containing_block: None,
                },
            )
            .unwrap();

        let list = Painter::new().paint(&tree, PaintOptions::viewport(320.0, 240.0));
        let text = list
            .commands()
            .iter()
            .find_map(|command| match command {
                DisplayCommand::DrawText {
                    text, font_weight, ..
                } => Some((text, font_weight)),
                _ => None,
            })
            .unwrap();

        assert_eq!(text.0, "Secure");
        assert_eq!(*text.1, FontWeight::Bold);
    }

    #[test]
    fn paints_replaced_elements_as_placeholders() {
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
                    viewport_width: 320.0,
                    viewport_height: 240.0,
                    containing_block: None,
                },
            )
            .unwrap();

        let list = Painter::new().paint(&tree, PaintOptions::viewport(320.0, 240.0));
        assert!(list.commands().iter().any(|command| matches!(
            command,
            DisplayCommand::DrawText { text, .. } if text.contains("image")
        )));
    }

    #[test]
    fn clips_to_viewport() {
        let arena = DomArena::new();
        let document = parse_html(&arena, "<body>Hello</body>").unwrap();
        let body = document.first_element_by_tag("body").unwrap();
        let tree = LayoutEngine::new(StyleEngine::new())
            .layout(
                body,
                LayoutConstraints {
                    viewport_width: 100.0,
                    viewport_height: 80.0,
                    containing_block: None,
                },
            )
            .unwrap();

        let list = Painter::new().paint(&tree, PaintOptions::viewport(100.0, 80.0));
        assert!(matches!(
            list.commands().first(),
            Some(DisplayCommand::ClipPush(Rect {
                width: 100.0,
                height: 80.0,
                ..
            }))
        ));
        assert!(matches!(
            list.commands().last(),
            Some(DisplayCommand::ClipPop)
        ));
    }

    #[test]
    fn debug_borders_are_optional() {
        let arena = DomArena::new();
        let document = parse_html(&arena, "<body><main>Hi</main></body>").unwrap();
        let body = document.first_element_by_tag("body").unwrap();
        let tree = LayoutEngine::new(StyleEngine::new())
            .layout(
                body,
                LayoutConstraints {
                    viewport_width: 160.0,
                    viewport_height: 100.0,
                    containing_block: None,
                },
            )
            .unwrap();

        let plain = Painter::new().paint(&tree, PaintOptions::viewport(160.0, 100.0));
        let debug = Painter::new().paint(
            &tree,
            PaintOptions {
                viewport: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 160.0,
                    height: 100.0,
                },
                include_debug_borders: true,
            },
        );

        assert!(debug.len() > plain.len());
    }

    #[test]
    fn paints_author_background_color() {
        let arena = DomArena::new();
        let document = parse_html(&arena, "<body><main>Hi</main></body>").unwrap();
        let mut style = StyleEngine::new();
        style.add_stylesheet(
            Stylesheet::parse("main { background-color: red; height: 20px; }").unwrap(),
        );
        let main = document.first_element_by_tag("main").unwrap();
        let tree = LayoutEngine::new(style)
            .layout(
                main,
                LayoutConstraints {
                    viewport_width: 160.0,
                    viewport_height: 100.0,
                    containing_block: None,
                },
            )
            .unwrap();

        let list = Painter::new().paint(&tree, PaintOptions::viewport(160.0, 100.0));
        assert!(list.commands().iter().any(|command| matches!(
            command,
            DisplayCommand::FillRect {
                color: Color {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255
                },
                ..
            }
        )));
    }
}
