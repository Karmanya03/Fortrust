use crate::theme::FortrustTheme;
use chrono::Local;
use egui::{self, Color32, CornerRadius, Rect, Vec2};

pub struct SpeedDialState {
    pub tiles: Vec<SpeedDialTile>,
    pub gradient_t: f32,
    pub search_query: String,
    pub search_engine: SearchEngine,
    pub ads_blocked: u32,
    pub trackers_blocked: u32,
    pub tab_count: usize,
}

impl Default for SpeedDialState {
    fn default() -> Self {
        Self {
            tiles: vec![
                SpeedDialTile::new("ChatGPT", "https://chat.openai.com", Some(Color32::from_rgb(16, 163, 127))),
                SpeedDialTile::new("GitHub", "https://github.com", Some(Color32::from_rgb(36, 41, 47))),
                SpeedDialTile::new("DuckDuckGo", "https://duckduckgo.com", Some(Color32::from_rgb(222, 84, 30))),
                SpeedDialTile::new("Wikipedia", "https://en.wikipedia.org", Some(Color32::from_rgb(255, 255, 255))),
                SpeedDialTile::new("Reddit", "https://reddit.com", Some(Color32::from_rgb(255, 69, 0))),
                SpeedDialTile::new("YouTube", "https://youtube.com", Some(Color32::from_rgb(255, 0, 0))),
                SpeedDialTile::new("X", "https://x.com", Some(Color32::from_rgb(29, 155, 240))),
                SpeedDialTile::new("Stack Overflow", "https://stackoverflow.com", Some(Color32::from_rgb(244, 130, 37))),
            ],
            gradient_t: 0.0,
            search_query: String::new(),
            search_engine: SearchEngine::default(),
            ads_blocked: 0,
            trackers_blocked: 0,
            tab_count: 1,
        }
    }
}

pub struct SpeedDialTile {
    pub title: String,
    pub url: String,
    pub color: Option<Color32>,
    pub hover_scale: f32,
}

impl SpeedDialTile {
    pub fn new(title: &str, url: &str, color: Option<Color32>) -> Self {
        Self {
            title: title.to_owned(),
            url: url.to_owned(),
            color,
            hover_scale: 1.0,
        }
    }
}

#[derive(Clone, PartialEq, Default)]
pub enum SearchEngine {
    #[default]
    DuckDuckGo,
    Google,
    Brave,
}

impl SearchEngine {
    pub fn all() -> Vec<Self> {
        vec![Self::DuckDuckGo, Self::Google, Self::Brave]
    }

    pub fn name(&self) -> &str {
        match self {
            Self::DuckDuckGo => "DDG",
            Self::Google => "Google",
            Self::Brave => "Brave",
        }
    }

    pub fn icon(&self) -> &str {
        match self {
            Self::DuckDuckGo => "🦆",
            Self::Google => "🌐",
            Self::Brave => "🦁",
        }
    }
}

impl SpeedDialState {
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        theme: &FortrustTheme,
        dt: f32,
        needs_new_tab: &mut bool,
    ) -> Option<String> {
        let mut search_navigation: Option<String> = None;
        let mut tile_navigation: Option<String> = None;

        self.gradient_t = (self.gradient_t + dt * 0.02).rem_euclid(1.0);
        // Light, watercolor-like background (off-white -> pale green)
        let bg_color = lerp_color(
            Color32::from_rgb(247, 243, 238),
            Color32::from_rgb(219, 233, 224),
            (self.gradient_t * std::f32::consts::TAU).sin() * 0.5 + 0.5,
        );

        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg_color);

        ui.vertical_centered(|ui| {
            ui.add_space(16.0);

            self.render_clock(ui, theme);
            ui.add_space(4.0);
            self.render_privacy_badge(ui, theme);
            ui.add_space(8.0);
            self.render_stat_pills(ui, theme);

            ui.add_space(18.0);

            search_navigation = self.render_search_bar(ui, theme);

            ui.add_space(24.0);

            ui.separator();
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Quick Access")
                    .size(12.0)
                    .strong()
                    .color(theme.text_secondary),
            );
            ui.add_space(12.0);
            tile_navigation = self.render_tile_grid(ui, theme, dt);
        });

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));

        if *needs_new_tab {
            self.search_query.clear();
            *needs_new_tab = false;
        }

        search_navigation.or(tile_navigation)
    }

    fn render_clock(&self, ui: &mut egui::Ui, theme: &FortrustTheme) {
        let now = Local::now();
        let time_str = now.format("%H:%M:%S").to_string();
        let date_str = now.format("%A, %B %d, %Y").to_string();

        ui.set_width(760.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(time_str)
                    .size(52.0)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(date_str)
                    .size(15.0)
                    .color(theme.text_secondary),
            );
        });
    }

    fn render_privacy_badge(&self, ui: &mut egui::Ui, theme: &FortrustTheme) {
        let green = theme.accent_shield;
        egui::Frame {
            fill: Color32::from_rgba_unmultiplied(green.r(), green.g(), green.b(), 30),
            corner_radius: CornerRadius::same(255),
            stroke: egui::Stroke::new(1.0, green),
            inner_margin: egui::Margin::symmetric(12, 4),
            ..Default::default()
        }
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("🛡 Private")
                    .size(12.0)
                    .strong()
                    .color(green),
            );
        });
    }

    fn render_stat_pills(&self, ui: &mut egui::Ui, theme: &FortrustTheme) {
        let total_blocked = self.ads_blocked + self.trackers_blocked;
        ui.horizontal(|ui| {
            ui.set_width(760.0);
            ui.horizontal_centered(|ui| {
                stat_pill(ui, "Tabs", &self.tab_count.to_string(), theme);
                ui.add_space(8.0);
                stat_pill(ui, "Blocked", &total_blocked.to_string(), theme);
                ui.add_space(8.0);
                stat_pill(ui, "Trackers", &self.trackers_blocked.to_string(), theme);
            });
        });
    }

    fn render_search_bar(&mut self, ui: &mut egui::Ui, theme: &FortrustTheme) -> Option<String> {
        let mut navigate: Option<String> = None;

        egui::Frame {
            fill: Color32::WHITE,
            corner_radius: CornerRadius::same(30),
            stroke: egui::Stroke::new(0.0, Color32::TRANSPARENT),
            inner_margin: egui::Margin::symmetric(18, 12),
            shadow: egui::epaint::Shadow {
                offset: [0, 4],
                blur: 18,
                spread: 0,
                color: Color32::from_black_alpha(18),
            },
            ..Default::default()
        }
        .show(ui, |ui| {
            ui.set_width(640.0);
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("search_engine")
                    .selected_text(self.search_engine.icon())
                    .width(36.0)
                    .show_ui(ui, |ui| {
                        for engine in SearchEngine::all() {
                            ui.selectable_value(
                                &mut self.search_engine,
                                engine.clone(),
                                engine.name(),
                            );
                        }
                    });

                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Search the web")
                        .frame(false)
                        .desired_width(ui.available_width() - 84.0)
                        .font(egui::FontId::proportional(16.0))
                        .text_color(theme.text_primary),
                );

                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let query = self.search_query.trim().to_string();
                    if !query.is_empty() {
                        navigate = Some(normalize_input(&query));
                    }
                }

                // plus / add button
                if ui
                    .add_sized(
                        [36.0, 36.0],
                        egui::Button::new(egui::RichText::new("+").size(20.0)).wrap(),
                    )
                    .clicked()
                {
                    // placeholder for add action
                }
            });
        });

        navigate
    }

    fn render_tile_grid(&mut self, ui: &mut egui::Ui, theme: &FortrustTheme, dt: f32) -> Option<String> {
        let tile_size = Vec2::new(140.0, 100.0);
        let cols = 4usize;
        let mut navigate: Option<String> = None;

        egui::Grid::new("speed_dial_grid")
            .spacing(Vec2::new(16.0, 16.0))
            .show(ui, |ui| {
                for (i, tile) in self.tiles.iter_mut().enumerate() {
                    if i > 0 && i % cols == 0 {
                        ui.end_row();
                    }

                    let tile_pos = ui.next_widget_position();
                    let tile_rect = Rect::from_min_size(tile_pos, tile_size);
                    let hovered = ui.rect_contains_pointer(tile_rect);
                    tile.hover_scale += ((if hovered { 1.05 } else { 1.0 }) - tile.hover_scale)
                        * (1.0 - (-12.0 * dt).exp());
                    if (tile.hover_scale - 1.0).abs() > 0.001 {
                        ui.ctx().request_repaint();
                    }

                    let scaled = Vec2::new(tile_size.x * tile.hover_scale, tile_size.y * tile.hover_scale);
                    let tile_bg = tile.color.unwrap_or(theme.tile_bg);
                    let response = ui.add_sized(
                        scaled,
                        egui::Button::new(
                            egui::RichText::new(format!("{}\n{}", tile.title, compact_url(&tile.url)))
                                .size(12.0)
                                .color(Color32::WHITE),
                        )
                        .fill(tile_bg)
                        .stroke(egui::Stroke::new(1.0, tile_bg.gamma_multiply(1.2)))
                        .corner_radius(14),
                    );

                    if response.clicked() {
                        navigate = Some(tile.url.clone());
                    }
                }
            });

        navigate
    }
}

fn stat_pill(ui: &mut egui::Ui, label: &str, value: &str, theme: &FortrustTheme) {
    let bg = Color32::from_rgba_unmultiplied(0, 0, 0, 30);
    egui::Frame {
        fill: bg,
        corner_radius: CornerRadius::same(255),
        inner_margin: egui::Margin::symmetric(10, 4),
        ..Default::default()
    }
    .show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(11.0)
                    .color(theme.text_secondary),
            );
            ui.label(
                egui::RichText::new(value)
                    .size(11.0)
                    .strong()
                    .color(theme.text_primary),
            );
        });
    });
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t) as u8,
    )
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

fn compact_url(url: &str) -> String {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_owned()
}

#[allow(dead_code)]
fn hero_chip(ui: &mut egui::Ui, theme: &FortrustTheme, label: &str) {
    egui::Frame {
        fill: theme.glass_hover,
        corner_radius: CornerRadius::same(255),
        stroke: egui::Stroke::new(1.0, theme.glass_border),
        inner_margin: egui::Margin::symmetric(10, 5),
        ..Default::default()
    }
    .show(ui, |ui| {
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .strong()
                .color(theme.text_primary),
        );
    });
}
 
