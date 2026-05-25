use crate::theme::FortrustTheme;
use egui::{self, Color32, CornerRadius, Ui};

#[derive(Default)]
pub struct OmniboxState {
    pub text: String,
    pub focused: bool,
    pub show_suggestions: bool,
}

impl OmniboxState {
    pub fn render(&mut self, ui: &mut Ui, theme: &FortrustTheme) -> Option<String> {
        let mut navigate: Option<String> = None;

        egui::Frame {
            fill: Color32::from_rgba_unmultiplied(255, 255, 255, 12),
            corner_radius: CornerRadius::same(10),
            stroke: egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 25)),
            inner_margin: egui::Margin::symmetric(12, 6),
            ..Default::default()
        }
        .show(ui, |ui| {
            ui.set_min_width(320.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("🔍")
                        .size(14.0)
                        .color(theme.text_secondary),
                );

                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.text)
                        .hint_text("Search or enter address...")
                        .frame(false)
                        .desired_width(ui.available_width() - 40.0)
                        .font(egui::FontId::proportional(15.0))
                        .text_color(theme.text_primary),
                );

                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let query = self.text.trim().to_string();
                    if !query.is_empty() {
                        navigate = Some(normalize_input(&query));
                    }
                }
                self.focused = resp.has_focus();
            });
        });

        navigate
    }
}

fn normalize_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("fortrust://")
        || trimmed.starts_with("about:")
    {
        trimmed.to_owned()
    } else if trimmed.contains('.') && !trimmed.contains(' ') {
        format!("https://{}", trimmed)
    } else {
        format!("https://duckduckgo.com/?q={}", urlencoding::encode(trimmed))
    }
}
