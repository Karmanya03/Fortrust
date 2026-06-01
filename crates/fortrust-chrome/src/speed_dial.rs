use crate::{icons, theme::FortrustTheme};
use egui::{self, Color32, CornerRadius, Pos2, Rect, Vec2, Stroke, Align2};

pub struct SpeedDialState {
    pub tiles: Vec<SpeedDialTile>,
    pub search_query: String,
    pub ads_blocked: u32,
    pub trackers_blocked: u32,
    pub fingerprint_attempts: u32,
    pub https_upgrades: u64,
    pub tab_count: usize,
    pub animation_frame: f32,
    pub star_positions: Vec<StarData>,
    pub show_add_dialog: bool,
    pub add_dialog_url: String,
    pub add_dialog_title: String,
    pub blocked_requests: u64,
    pub load_time_savings: f32,
    pub doh_enabled: bool,
    pub fingerprinting_protection: bool,
}

const SPEED_DIAL_SETTINGS_KEY: &str = "chrome.speed_dial.tiles";

impl SpeedDialState {
    pub fn persist_tiles(&self, storage: &fortrust_storage::StorageDatabase) {
        let data: String = self.tiles.iter()
            .map(|t| format!("{}|{}", t.title.replace('|', " "), t.url.replace('|', " ")))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = storage.settings.store(SPEED_DIAL_SETTINGS_KEY, &fortrust_storage::SettingValue::from(data));
    }

    pub fn load_persisted_tiles(&mut self, storage: &fortrust_storage::StorageDatabase) {
        let Some(val) = storage.settings.load(SPEED_DIAL_SETTINGS_KEY) else { return; };
        let Some(data) = val.as_string() else { return; };
        if data.is_empty() { return; }
        let loaded: Vec<SpeedDialTile> = data.lines()
            .filter_map(|line| {
                let mut parts = line.splitn(2, '|');
                let title = parts.next()?.to_string();
                let url = parts.next()?.to_string();
                Some(SpeedDialTile::new(&title, &url, None))
            })
            .collect();
        if !loaded.is_empty() {
            self.tiles = loaded;
        }
    }
}

pub struct SpeedDialTile {
    pub title: String,
    pub url: String,
    pub color: Option<Color32>,
    pub hover_scale: f32,
    pub initials: String,
}

impl SpeedDialTile {
    pub fn new(title: &str, url: &str, color: Option<Color32>) -> Self {
        let initials = title.split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase();
        Self {
            title: title.to_owned(),
            url: url.to_owned(),
            color,
            hover_scale: 1.0,
            initials,
        }
    }
}

pub struct StarData {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub a: f32,
    pub twinkle: f32,
    pub speed: f32,
}

fn get_time_components() -> (u32, u32, u32) {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs();
    let hours = ((total_secs / 3600 + 8) % 24) as u32;
    let minutes = ((total_secs / 60) % 60) as u32;
    let seconds = (total_secs % 60) as u32;
    (hours, minutes, seconds)
}

impl Default for SpeedDialState {
    fn default() -> Self {
        let mut rng = 42.0f32;
        let mut star = || {
            rng = (rng * 1.618 + 13.37) % 1.0;
            rng
        };

        Self {
            tiles: vec![
                SpeedDialTile::new("GitHub", "https://github.com", Some(Color32::from_rgb(36, 41, 47))),
                SpeedDialTile::new("MDN Docs", "https://developer.mozilla.org", Some(Color32::from_rgb(240, 120, 80))),
                SpeedDialTile::new("Hacker News", "https://news.ycombinator.com", Some(Color32::from_rgb(255, 100, 50))),
                SpeedDialTile::new("Crates.io", "https://crates.io", Some(Color32::from_rgb(160, 120, 240))),
                SpeedDialTile::new("Rust Book", "https://doc.rust-lang.org/book", Some(Color32::from_rgb(80, 200, 140))),
                SpeedDialTile::new("DuckDuckGo", "https://duckduckgo.com", Some(Color32::from_rgb(222, 84, 30))),
            ],
            search_query: String::new(),
            ads_blocked: 0,
            trackers_blocked: 0,
            fingerprint_attempts: 0,
            https_upgrades: 0,
            tab_count: 1,
            animation_frame: 0.0,
            show_add_dialog: false,
            add_dialog_url: String::new(),
            add_dialog_title: String::new(),
            star_positions: (0..260).map(|_| {
                StarData {
                    x: star(),
                    y: star() * 0.72,
                    r: star() * 1.1 + 0.2,
                    a: star() * 0.55 + 0.08,
                    twinkle: star() * std::f32::consts::TAU,
                    speed: star() * 0.004 + 0.001,
                }
            }).collect(),
            blocked_requests: 12847,
            load_time_savings: 3.2,
            doh_enabled: true,
            fingerprinting_protection: true,
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
        storage: Option<&fortrust_storage::StorageDatabase>,
    ) -> Option<String> {
        let mut search_navigation: Option<String> = None;
        let mut tile_navigation: Option<String> = None;

        self.animation_frame += dt;
        let rect = ui.available_rect_before_wrap();

        // Dark background
        ui.painter().rect_filled(rect, CornerRadius::ZERO, theme.surface_deepest);

        // Paint animated background (starfield + hills)
        self.paint_background(ui, rect);

        // Dot grid overlay
        self.render_dot_grid(ui, rect);

        // Foreground content
        ui.vertical_centered(|ui| {
            ui.add_space(8.0);

            // Clock
            self.render_clock(ui, theme);

            ui.add_space(8.0);

            // Privacy badge
            self.render_privacy_badge(ui, theme);

            ui.add_space(20.0);

            // Search bar
            search_navigation = self.render_search_bar(ui, theme);

            ui.add_space(30.0);

            // Speed dial grid
            tile_navigation = self.render_tile_grid(ui, theme, dt);

            ui.add_space(60.0);
        });

        // Stat pills at bottom
        self.render_stat_pills(ui, theme, rect);

        ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));

        // Add site dialog
        if self.show_add_dialog {
            egui::Area::new("speed_dial_add".into())
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    egui::Frame {
                        fill: Color32::from_rgb(28, 34, 44),
                        corner_radius: CornerRadius::same(12),
                        stroke: Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 25)),
                        inner_margin: egui::Margin::symmetric(20, 16),
                        ..Default::default()
                    }.show(ui, |ui| {
                        ui.set_min_width(280.0);
                        ui.label(egui::RichText::new("Add site").size(14.0).color(Color32::from_rgb(226, 230, 238)));
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Title").size(11.0).color(Color32::from_rgba_unmultiplied(160, 175, 200, 150)));
                        let title_resp = ui.add(
                            egui::TextEdit::singleline(&mut self.add_dialog_title)
                                .hint_text("Site name")
                                .desired_width(260.0)
                                .font(egui::FontId::proportional(13.0))
                                .text_color(Color32::from_rgb(226, 230, 238)),
                        );
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new("URL").size(11.0).color(Color32::from_rgba_unmultiplied(160, 175, 200, 150)));
                        let url_resp = ui.add(
                            egui::TextEdit::singleline(&mut self.add_dialog_url)
                                .hint_text("https://example.com")
                                .desired_width(260.0)
                                .font(egui::FontId::proportional(13.0))
                                .text_color(Color32::from_rgb(226, 230, 238)),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                self.show_add_dialog = false;
                            }
                            let enter_pressed = (title_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                || (url_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                            if ui.button("Add").clicked() || enter_pressed {
                                let url = self.add_dialog_url.trim().to_string();
                                if !url.is_empty() {
                                    let title = if self.add_dialog_title.trim().is_empty() {
                                        url.clone()
                                    } else {
                                        self.add_dialog_title.trim().to_string()
                                    };
                                    self.tiles.push(SpeedDialTile::new(&title, &url, None));
                                    self.show_add_dialog = false;
                                    self.add_dialog_title.clear();
                                    self.add_dialog_url.clear();
                                    if let Some(s) = storage { self.persist_tiles(s); }
                                }
                            }
                        });
                        if url_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            self.show_add_dialog = false;
                        }
                    });
                });
        }

        if *needs_new_tab {
            self.search_query.clear();
            *needs_new_tab = false;
        }

        search_navigation.or(tile_navigation)
    }

    fn render_clock(&self, ui: &mut egui::Ui, _theme: &FortrustTheme) {
        let (h, m, s) = get_time_components();
        let time_str = format!("{:02}:{:02}", h, m);
        let sec_str = format!("{:02}", s);

        let cx = ui.max_rect().center().x;

        ui.painter().text(
            Pos2::new(cx, ui.cursor().min.y + 4.0),
            Align2::CENTER_CENTER,
            &time_str,
            egui::FontId::proportional(64.0),
            Color32::from_rgba_unmultiplied(220, 230, 245, 55),
        );

        ui.painter().text(
            Pos2::new(cx + 170.0, ui.cursor().min.y + 12.0),
            Align2::LEFT_CENTER,
            &sec_str,
            egui::FontId::monospace(18.0),
            Color32::from_rgba_unmultiplied(220, 230, 245, 35),
        );

        ui.allocate_space(Vec2::new(0.0, 74.0));
    }

    fn render_dot_grid(&self, ui: &mut egui::Ui, rect: Rect) {
        let spacing = 42.0;
        let dot_radius = 0.5;
        let dot_color = Color32::from_rgba_unmultiplied(100, 140, 200, 10);

        let cols = (rect.width() / spacing).ceil() as i32;
        let rows = ((rect.height() * 0.72) / spacing).ceil() as i32;

        for r in 0..rows {
            for c in 0..cols {
                let x = rect.min.x + (c as f32 + 0.5) * spacing;
                let y = rect.min.y + (r as f32 + 0.5) * spacing;
                let offset = (r as f32 * 1.7 + c as f32 * 3.1 + self.animation_frame * 0.05).sin() * 0.3;
                let alpha = (dot_color.a() as f32 * (0.5 + offset * 0.5)) as u8;
                ui.painter().circle_filled(
                    Pos2::new(x, y),
                    dot_radius,
                    Color32::from_rgba_unmultiplied(dot_color.r(), dot_color.g(), dot_color.b(), alpha),
                );
            }
        }
    }

    fn paint_background(&self, ui: &mut egui::Ui, rect: Rect) {
        let w = rect.width();
        let h = rect.height();

        // Sky gradient
        let sky = egui::epaint::Color32::from_rgb(5, 8, 16);
        let sky_mid = Color32::from_rgb(8, 12, 24);
        let sky_bot = Color32::from_rgb(12, 18, 32);
        let sky_h = h * 0.72;

        // Draw sky in bands
        let steps = 20usize;
        for i in 0..steps {
            let t = i as f32 / (steps - 1) as f32;
            let c = if t < 0.3 {
                lerp_color(sky, sky_mid, t / 0.3)
            } else {
                lerp_color(sky_mid, sky_bot, (t - 0.3) / 0.7)
            };
            let band = Rect::from_min_size(
                Pos2::new(rect.min.x, rect.min.y + i as f32 * sky_h / steps as f32),
                Vec2::new(w, sky_h / steps as f32 + 1.0),
            );
            ui.painter().rect_filled(band, CornerRadius::ZERO, c);
        }

        // Stars
        let phase = self.animation_frame;
        for s in &self.star_positions {
            let alpha = (s.a + (phase * s.speed + s.twinkle).sin() * 0.15).max(0.0);
            let star_pos = Pos2::new(rect.min.x + s.x * w, rect.min.y + s.y * sky_h);
            let size = s.r;
            ui.painter().circle_filled(
                star_pos,
                size,
                Color32::from_rgba_unmultiplied(200, 215, 255, (alpha * 255.0) as u8),
            );
        }

        // Nebula blobs
        let neb1_cx = rect.min.x + w * 0.25;
        let neb1_cy = rect.min.y + h * 0.18;
        let neb1_r = w * 0.22;
        for i in 0..12 {
            let t = i as f32 / 12.0;
            let r = neb1_r * (1.0 - t * 0.5);
            let a = ((8.0 * (1.0 - t)) as u8).max(1);
            ui.painter().circle_filled(
                Pos2::new(neb1_cx, neb1_cy),
                r,
                Color32::from_rgba_unmultiplied(40, 60, 120, a),
            );
        }

        let neb2_cx = rect.min.x + w * 0.75;
        let neb2_cy = rect.min.y + h * 0.28;
        let neb2_r = w * 0.18;
        for i in 0..10 {
            let t = i as f32 / 10.0;
            let r = neb2_r * (1.0 - t * 0.5);
            let a = ((6.0 * (1.0 - t)) as u8).max(1);
            ui.painter().circle_filled(
                Pos2::new(neb2_cx, neb2_cy),
                r,
                Color32::from_rgba_unmultiplied(20, 50, 80, a),
            );
        }

        // Fog transition band
        let fog_rect = Rect::from_min_size(
            Pos2::new(rect.min.x, rect.min.y + h * 0.56),
            Vec2::new(w, h * 0.2),
        );
        ui.painter().rect_filled(fog_rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(8, 18, 20, 35));

        // Hills
        let hill1: &[(f32, f32)] = &[(0.0,0.72),(0.08,0.65),(0.18,0.61),(0.32,0.57),(0.46,0.60),(0.58,0.56),(0.70,0.59),(0.84,0.55),(0.93,0.59),(1.0,0.63)];
        let hill2: &[(f32, f32)] = &[(0.0,0.80),(0.06,0.70),(0.15,0.66),(0.28,0.63),(0.40,0.68),(0.52,0.63),(0.63,0.66),(0.75,0.61),(0.87,0.65),(1.0,0.70)];
        let hill3: &[(f32, f32)] = &[(0.0,0.88),(0.05,0.76),(0.14,0.72),(0.24,0.69),(0.36,0.74),(0.50,0.70),(0.62,0.73),(0.73,0.68),(0.85,0.72),(0.95,0.75),(1.0,0.79)];
        let hill4: &[(f32, f32)] = &[(0.0,1.0),(0.04,0.84),(0.12,0.79),(0.22,0.76),(0.33,0.81),(0.44,0.77),(0.56,0.80),(0.68,0.75),(0.80,0.79),(0.92,0.76),(1.0,0.82)];
        let hill_configs = [
            (hill1, Color32::from_rgb(12, 24, 20), Color32::from_rgb(8, 18, 14)),
            (hill2, Color32::from_rgb(15, 34, 22), Color32::from_rgb(10, 22, 16)),
            (hill3, Color32::from_rgb(18, 44, 26), Color32::from_rgb(12, 28, 18)),
            (hill4, Color32::from_rgb(14, 36, 20), Color32::from_rgb(8, 18, 12)),
        ];

        for (points, top, _bot) in &hill_configs {
              let path = egui::Shape::Path(egui::epaint::PathShape {
                points: {
                    let mut pts = Vec::new();
                    pts.push(Pos2::new(rect.min.x, rect.max.y));
                    for (px, py) in *points {
                        pts.push(Pos2::new(rect.min.x + px * w, rect.min.y + py * h));
                    }
                    pts.push(Pos2::new(rect.max.x, rect.max.y));
                    pts
                },
                closed: true,
                fill: *top,
                stroke: Default::default(),
            });
            ui.painter().add(path);
        }

        // Ground fill
        let ground_rect = Rect::from_min_size(
            Pos2::new(rect.min.x, rect.min.y + h * 0.84),
            Vec2::new(w, h * 0.16),
        );
        ui.painter().rect_filled(ground_rect, CornerRadius::ZERO, Color32::from_rgb(8, 19, 13));

        // Haze overlay
        let haze_rect = Rect::from_min_size(
            Pos2::new(rect.min.x, rect.min.y + h * 0.64),
            Vec2::new(w, h * 0.15),
        );
        ui.painter().rect_filled(haze_rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(10, 20, 16, 35));
    }

    fn render_privacy_badge(&self, ui: &mut egui::Ui, _theme: &FortrustTheme) {
        let total_blocked = self.ads_blocked + self.trackers_blocked + self.fingerprint_attempts;
        let text = if self.https_upgrades > 0 {
            format!("{total_blocked} blocked · {} HTTPS · {} FP stopped", self.https_upgrades, self.fingerprint_attempts)
        } else {
            format!("{total_blocked} blocked · {} fingerprint attempts", self.fingerprint_attempts)
        };
        let font = egui::FontId::proportional(11.5);
        let galley = ui.painter().layout_no_wrap(text.clone(), font, Color32::from_rgba_unmultiplied(200, 210, 230, 128));

        let padding = Vec2::new(12.0, 5.0);
        let text_size = galley.size();
        let badge_size = text_size + padding * 2.0;

        let pos = Pos2::new(
            ui.max_rect().center().x - badge_size.x / 2.0,
            ui.max_rect().top() + 20.0,
        );
        let badge_rect = Rect::from_min_size(pos, badge_size);

        // Background with blur effect
        ui.painter().rect_filled(badge_rect, CornerRadius::same(20), Color32::from_rgba_unmultiplied(20, 24, 32, 102));
        ui.painter().rect_stroke(badge_rect, CornerRadius::same(20), Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 15)), egui::StrokeKind::Inside);

        // Shield icon + text
        let shield_rect = Rect::from_min_size(
            Pos2::new(badge_rect.min.x + padding.x + 2.0, badge_rect.center().y - 6.0),
            Vec2::new(14.0, 14.0),
        );
        icons::paint_shield_icon_rect(ui.painter(), shield_rect, Color32::WHITE);
        ui.painter().text(
            Pos2::new(badge_rect.min.x + padding.x + 22.0, badge_rect.center().y),
            egui::Align2::LEFT_CENTER,
            &text,
            egui::FontId::proportional(11.5),
            Color32::from_rgba_unmultiplied(200, 210, 230, 128),
        );
    }

    fn render_search_bar(&mut self, ui: &mut egui::Ui, _theme: &FortrustTheme) -> Option<String> {
        let mut navigate: Option<String> = None;

        let search_width = 560.0;
        let search_height = 46.0;
        let x = ui.max_rect().center().x - search_width / 2.0;
        let y = ui.cursor().min.y;

        let search_rect = Rect::from_min_size(
            Pos2::new(x, y),
            Vec2::new(search_width, search_height),
        );

        // Background
        ui.painter().rect_filled(search_rect, CornerRadius::same(25), Color32::from_rgba_unmultiplied(22, 28, 40, 210));
        ui.painter().rect_stroke(search_rect, CornerRadius::same(25), Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 25)), egui::StrokeKind::Inside);

        // Fortrust search glyph
        let g_rect = Rect::from_min_size(
            Pos2::new(search_rect.min.x + 8.0, search_rect.center().y - 11.0),
            Vec2::new(22.0, 22.0),
        );
        ui.painter().circle_filled(g_rect.center(), 10.5, Color32::from_rgba_unmultiplied(80, 155, 255, 35));
        icons::paint_search_icon(ui.painter(), g_rect.center(), 17.0, Color32::from_rgb(185, 215, 255));

        // Input field
        let input_rect = Rect::from_min_size(
            Pos2::new(search_rect.min.x + 34.0, search_rect.min.y),
            Vec2::new(search_width - 80.0, search_height),
        );

        let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(input_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let resp = child_ui.add(
            egui::TextEdit::singleline(&mut self.search_query)
                .hint_text("Search privately with Fortrust")
                .frame(false)
                .desired_width(input_rect.width() - 10.0)
                .font(egui::FontId::proportional(14.0))
                .text_color(Color32::from_rgb(226, 230, 238)),
        );

        // Use the allocated rect from child_ui interaction
        let _ = ui.allocate_rect(search_rect, egui::Sense::hover());

        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let query = self.search_query.trim().to_string();
            if !query.is_empty() {
                navigate = Some(normalize_input(&query));
            }
        }

        // "+" button
        let add_rect = Rect::from_min_size(
            Pos2::new(search_rect.max.x - search_height, search_rect.min.y),
            Vec2::new(search_height, search_height),
        );
        ui.painter().rect_filled(add_rect, CornerRadius::same(25), Color32::from_rgba_unmultiplied(22, 28, 40, 210));
        ui.painter().rect_stroke(add_rect, CornerRadius::same(25), Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 25)), egui::StrokeKind::Inside);
        icons::paint_plus_icon(ui.painter(), add_rect, Color32::from_rgb(226, 230, 238));

        let add_resp = ui.allocate_rect(add_rect, egui::Sense::click());
        if add_resp.clicked() {
            self.show_add_dialog = true;
            self.add_dialog_url.clear();
            self.add_dialog_title.clear();
        }

        ui.allocate_space(Vec2::new(0.0, search_height + 8.0));

        navigate
    }

    fn render_tile_grid(&mut self, ui: &mut egui::Ui, _theme: &FortrustTheme, dt: f32) -> Option<String> {
        let mut navigate: Option<String> = None;

        // Grid layout: 6 columns
        let cols = 6usize;
        let gap = 14.0;
        let tile_w = 72.0;
        let tile_h = 54.0;
        let label_h = 16.0;
        let item_h = tile_h + label_h + 6.0;
        let grid_w = cols as f32 * tile_w + (cols - 1) as f32 * gap;

        let start_x = ui.max_rect().center().x - grid_w / 2.0;
        let start_y = ui.cursor().min.y;

        for (i, tile) in self.tiles.iter_mut().enumerate() {
            let col = i % cols;
            let row = i / cols;

            let x = start_x + col as f32 * (tile_w + gap);
            let y = start_y + row as f32 * (item_h + 2.0);

            let tile_rect = Rect::from_min_size(
                Pos2::new(x, y),
                Vec2::new(tile_w, tile_h),
            );

            let hovered = ui.rect_contains_pointer(tile_rect);
            tile.hover_scale += ((if hovered { 1.05 } else { 1.0 }) - tile.hover_scale)
                * (1.0 - (-12.0 * dt).exp());
            if (tile.hover_scale - 1.0).abs() > 0.001 {
                ui.ctx().request_repaint();
            }

            // Apply hover scale to tile rect
            let s = tile.hover_scale;
            let scaled_rect = Rect::from_center_size(tile_rect.center(), tile_rect.size() * s);

            // Draw tile
            let bg = Color32::from_rgba_unmultiplied(28, 34, 44, 191);
            let border = if hovered {
                Color32::from_rgba_unmultiplied(255, 255, 255, 38)
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 20)
            };

            ui.painter().rect_filled(scaled_rect, CornerRadius::same(8), bg);
            ui.painter().rect_stroke(scaled_rect, CornerRadius::same(8), Stroke::new(1.0, border), egui::StrokeKind::Inside);

            // Initials
            ui.painter().text(
                scaled_rect.center(),
                egui::Align2::CENTER_CENTER,
                &tile.initials,
                egui::FontId::monospace(12.0),
                Color32::from_rgba_unmultiplied(180, 195, 220, 128),
            );

            // Label
            ui.painter().text(
                Pos2::new(scaled_rect.center().x, tile_rect.max.y + 4.0),
                egui::Align2::CENTER_CENTER,
                &tile.title,
                egui::FontId::proportional(11.0),
                Color32::from_rgba_unmultiplied(180, 195, 220, 115),
            );

            let resp = ui.allocate_rect(tile_rect, egui::Sense::click());
            if resp.clicked() {
                navigate = Some(tile.url.clone());
            }
        }

        // Add site tile
        let add_row = (self.tiles.len() / cols) as f32;
        let add_col = (self.tiles.len() % cols) as f32;
        let x = start_x + add_col * (tile_w + gap);
        let y = start_y + add_row * (item_h + 2.0);

        let add_rect = Rect::from_min_size(
            Pos2::new(x, y),
            Vec2::new(tile_w, tile_h),
        );

        ui.painter().rect_stroke(add_rect, CornerRadius::same(8), Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 255, 255, 25)), egui::StrokeKind::Inside);
        icons::paint_plus_icon(ui.painter(), add_rect, Color32::from_rgba_unmultiplied(180, 195, 220, 77));
        ui.painter().text(
            Pos2::new(add_rect.center().x, add_rect.max.y + 4.0),
            egui::Align2::CENTER_CENTER,
            "Add site",
            egui::FontId::proportional(11.0),
            Color32::from_rgba_unmultiplied(180, 195, 220, 115),
        );

        if ui.allocate_rect(add_rect, egui::Sense::click()).clicked() {
            self.show_add_dialog = true;
            self.add_dialog_url.clear();
            self.add_dialog_title.clear();
        }

        navigate
    }

    fn render_stat_pills(&self, ui: &mut egui::Ui, _theme: &FortrustTheme, rect: Rect) {
        let total_blocked = self.ads_blocked + self.trackers_blocked + self.fingerprint_attempts;
        let blocked_str = format!("{}", total_blocked);
        let https_str = format!("{}", self.https_upgrades);
        let fp_str = format!("{}", self.fingerprint_attempts);
        let doh_str = if self.doh_enabled { "Active".to_owned() } else { "Off".to_owned() };
        let pills = [
            ("Blocked", &blocked_str),
            ("HTTPS", &https_str),
            ("FP", &fp_str),
            ("DoH", &doh_str),
        ];

        let pill_h = 28.0;
        let spacing = 12.0;
        let total_w: f32 = pills.iter().map(|(name, val)| {
            8.0 + ui.painter().layout_no_wrap(name.to_string(), egui::FontId::proportional(11.0), Color32::WHITE).size().x
                + 4.0 + ui.painter().layout_no_wrap(val.to_string(), egui::FontId::proportional(11.5), Color32::WHITE).size().x + 8.0 + spacing
        }).sum();
        let total_w = total_w - spacing; // remove trailing spacing

        let pill_y = rect.max.y - 50.0;
        let start_x = rect.center().x - total_w / 2.0;

        let mut cx = start_x;
        for (name, value) in &pills {
            let name_w = ui.painter().layout_no_wrap(name.to_string(), egui::FontId::proportional(11.0), Color32::WHITE).size().x;
            let val_w = ui.painter().layout_no_wrap(value.to_string(), egui::FontId::proportional(11.5), Color32::WHITE).size().x;
            let pw = 8.0 + name_w + 4.0 + val_w + 8.0;

            let pill_rect = Rect::from_min_size(Pos2::new(cx, pill_y), Vec2::new(pw, pill_h));

            ui.painter().rect_filled(pill_rect, CornerRadius::same(14), Color32::from_rgba_unmultiplied(16, 20, 28, 140));
            ui.painter().rect_stroke(pill_rect, CornerRadius::same(14), Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 12)), egui::StrokeKind::Inside);

            // Name
            ui.painter().text(
                Pos2::new(pill_rect.min.x + 8.0, pill_rect.center().y),
                Align2::LEFT_CENTER,
                *name,
                egui::FontId::proportional(11.0),
                Color32::from_rgba_unmultiplied(160, 175, 200, 150),
            );

            // Value
            ui.painter().text(
                Pos2::new(pill_rect.max.x - 8.0, pill_rect.center().y),
                Align2::RIGHT_CENTER,
                *value,
                egui::FontId::proportional(11.5),
                Color32::from_rgba_unmultiplied(200, 215, 240, 180),
            );

            let _ = ui.allocate_rect(pill_rect, egui::Sense::hover());

            cx += pw + spacing;
        }
    }
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
        format!("fortrust://search?q={}", urlencoding::encode(trimmed))
    }
}
