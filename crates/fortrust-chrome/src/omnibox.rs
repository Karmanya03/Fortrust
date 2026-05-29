use crate::{icons, theme::FortrustTheme};
use egui::{self, Color32, CornerRadius, Pos2, Rect, Ui, Vec2};

#[derive(Default)]
pub struct OmniboxState {
    pub text: String,
    pub focused: bool,
    pub suggestions: Vec<SuggestionItem>,
    pub selected_suggestion: i32,
    pub show_suggestions: bool,
}

pub struct SuggestionItem {
    pub kind: SuggestionKind,
    pub text: String,
    pub url: String,
}

pub enum SuggestionKind {
    History,
    Bookmark,
    Search,
    Url,
}

impl OmniboxState {
    pub fn clear_suggestions(&mut self) {
        self.suggestions.clear();
        self.selected_suggestion = -1;
        self.show_suggestions = false;
    }

    pub fn update_suggestions(&mut self, history_entries: &[SuggestionItem]) {
        let query = self.text.trim().to_lowercase();
        if query.is_empty() || !self.focused {
            self.clear_suggestions();
            return;
        }

        self.suggestions.clear();

        // Add search suggestion
        self.suggestions.push(SuggestionItem {
            kind: SuggestionKind::Search,
            text: format!("Search for \"{}\"", self.text.trim()),
            url: format!("https://duckduckgo.com/?q={}", urlencoding::encode(self.text.trim())),
        });

        // Add matching history/bookmarks
        for entry in history_entries {
            if entry.url.to_lowercase().contains(&query)
                || entry.text.to_lowercase().contains(&query)
            {
                self.suggestions.push(SuggestionItem {
                    kind: SuggestionKind::History,
                    text: entry.text.clone(),
                    url: entry.url.clone(),
                });
            }
        }

        // If the input looks like a URL, add a direct URL suggestion
        if self.text.contains('.') && !self.text.contains(' ') {
            let url = if self.text.starts_with("http://") || self.text.starts_with("https://") {
                self.text.clone()
            } else {
                format!("https://{}", self.text)
            };
            self.suggestions.push(SuggestionItem {
                kind: SuggestionKind::Url,
                text: url.clone(),
                url,
            });
        }

        self.show_suggestions = !self.suggestions.is_empty();
        self.selected_suggestion = if self.show_suggestions { 0 } else { -1 };
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        theme: &FortrustTheme,
        history_entries: &[SuggestionItem],
    ) -> Option<String> {
        let mut navigate: Option<String> = None;
        let is_url = self.text.starts_with("http://") || self.text.starts_with("https://");
        let is_secure = self.text.starts_with("https://");
        let green = theme.accent_shield;

        let border_color = if self.focused {
            theme.accent_primary
        } else {
            Color32::from_rgba_unmultiplied(50, 57, 73, 200)
        };

        // Determine suggestion dropdown position based on address bar rect
        let pill_frame = egui::Frame {
            fill: theme.surface_deepest,
            corner_radius: CornerRadius::same(25),
            stroke: egui::Stroke::new(1.0, border_color),
            inner_margin: egui::Margin::symmetric(10, 4),
            ..Default::default()
        };

        let pill_inner = pill_frame
            .show(ui, |ui| {
                ui.set_min_width(200.0);
                ui.horizontal(|ui| {
                    ui.add_space(2.0);

                    // Focus glow ring
                    if self.focused {
                        let glow_rect = ui.max_rect().expand2(Vec2::new(2.0, 2.0));
                        ui.painter().rect_filled(glow_rect, CornerRadius::same(27), Color32::from_rgba_unmultiplied(79, 158, 255, 20));
                    }

                    // Lock or search icon using SVG
                    let icon_center = egui::pos2(
                        ui.cursor().min.x + 8.0,
                        ui.cursor().center().y,
                    );
                    if is_url {
                        if is_secure {
                            icons::paint_lock_icon(ui.painter(), icon_center, 14.0, green);
                        } else {
                            ui.label(
                                egui::RichText::new("!")
                                    .size(13.0)
                                    .color(theme.accent_shield_warn)
                                    .strong(),
                            );
                        }
                    } else {
                        icons::paint_search_icon(ui.painter(), icon_center, 14.0, theme.text_muted);
                    }
                    ui.add_space(18.0);

                    let font = if is_url {
                        egui::FontId::monospace(11.0)
                    } else {
                        egui::FontId::proportional(11.5)
                    };

                    let text_edit = egui::TextEdit::singleline(&mut self.text)
                        .hint_text("Enter search or web address")
                        .frame(false)
                        .desired_width((ui.available_width() - 40.0).max(50.0))
                        .font(font)
                        .text_color(if is_url { green } else { theme.text_secondary });
                    let resp = ui.add(text_edit);

                    let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let arrow_down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                    let arrow_up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));

                    // Handle keyboard navigation
                    if arrow_down && self.show_suggestions {
                        self.selected_suggestion = (self.selected_suggestion + 1).min(self.suggestions.len() as i32 - 1);
                        ui.ctx().request_repaint();
                    }
                    if arrow_up && self.show_suggestions {
                        self.selected_suggestion = (self.selected_suggestion - 1).max(0);
                        ui.ctx().request_repaint();
                    }
                    if esc {
                        self.clear_suggestions();
                        self.focused = false;
                    }

                    // Enter selects suggestion or navigates
                    if enter_pressed {
                        if self.show_suggestions && self.selected_suggestion >= 0 && (self.selected_suggestion as usize) < self.suggestions.len() {
                            let s = &self.suggestions[self.selected_suggestion as usize];
                            navigate = Some(s.url.clone());
                            self.text = s.url.clone();
                        } else {
                            let query = self.text.trim().to_string();
                            if !query.is_empty() {
                                navigate = Some(normalize_input(&query));
                            }
                        }
                        self.clear_suggestions();
                    }

                    self.focused = resp.has_focus();

                    // Update suggestions when text changes
                    if resp.changed() {
                        self.update_suggestions(history_entries);
                    }
                });
            });

        // Draw suggestion dropdown below the pill
        if self.show_suggestions && self.focused {
            let n = self.suggestions.len().min(6);
            let drop_h = (n as f32 * 32.0 + 8.0).max(0.0);
            let d_w = pill_inner.response.rect.width().max(50.0);
            let drop_rect = Rect::from_min_size(
                Pos2::new(pill_inner.response.rect.min.x, pill_inner.response.rect.max.y + 4.0),
                Vec2::new(d_w, drop_h),
            );

            // Dropdown background
            ui.painter().rect_filled(drop_rect, CornerRadius::same(12), theme.surface_sidebar);
            ui.painter().rect_stroke(drop_rect, CornerRadius::same(12), egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 15)), egui::StrokeKind::Inside);

            let mut clicked_index: Option<usize> = None;

            for i in 0..n.min(self.suggestions.len()) {
                let item_rect = Rect::from_min_size(
                    Pos2::new(drop_rect.min.x + 4.0, drop_rect.min.y + 4.0 + i as f32 * 32.0),
                    Vec2::new(drop_rect.width() - 8.0, 30.0),
                );

                let selected = i as i32 == self.selected_suggestion;
                if selected {
                    ui.painter().rect_filled(item_rect, CornerRadius::same(6), Color32::from_rgba_unmultiplied(79, 158, 255, 30));
                } else if ui.rect_contains_pointer(item_rect) {
                    ui.painter().rect_filled(item_rect, CornerRadius::same(6), Color32::from_white_alpha(8));
                }

                // Icon
                let icon_rect = Rect::from_min_size(
                    Pos2::new(item_rect.min.x + 8.0, item_rect.center().y - 7.0),
                    Vec2::new(14.0, 14.0),
                );
                match self.suggestions[i].kind {
                    SuggestionKind::Search => icons::paint_search_icon(ui.painter(), icon_rect.center(), 14.0, theme.text_muted),
                    SuggestionKind::History => icons::paint_history_icon(ui.painter(), icon_rect, theme.text_muted),
                    SuggestionKind::Bookmark => icons::paint_bookmark_icon(ui.painter(), icon_rect, theme.text_muted),
                    SuggestionKind::Url => icons::paint_globe_icon(ui.painter(), icon_rect, theme.text_muted),
                }

                // Text
                let text_color = if selected { theme.text_primary } else { theme.text_secondary };
                ui.painter().text(
                    Pos2::new(item_rect.min.x + 28.0, item_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &self.suggestions[i].text,
                    egui::FontId::proportional(12.0),
                    text_color,
                );

                // Click to select
                if ui.allocate_rect(item_rect, egui::Sense::click()).clicked() {
                    clicked_index = Some(i);
                }
            }

            if let Some(idx) = clicked_index {
                navigate = Some(self.suggestions[idx].url.clone());
                self.text = self.suggestions[idx].url.clone();
                self.clear_suggestions();
            }
        }

        // If input lost focus but not because of the dropdown, hide suggestions
        if !self.focused && !ui.rect_contains_pointer(Rect::from_min_size(Pos2::new(pill_inner.response.rect.min.x, pill_inner.response.rect.min.y), Vec2::new(pill_inner.response.rect.width(), 400.0))) {
            self.clear_suggestions();
        }

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
