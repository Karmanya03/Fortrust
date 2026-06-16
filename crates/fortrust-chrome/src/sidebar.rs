use crate::{animation::SidebarAnimation, icons, theme::FortrustTheme};
use egui::{self, Color32, CornerRadius, Pos2, Rect, Stroke, Vec2};
use fortrust_core::{BrowserConfig, WorkspaceId, WorkspaceManager};
use fortrust_storage::{SettingValue, StorageDatabase};
use serde::{Deserialize, Serialize};

/// AI quick actions that can be triggered from the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub enum AIAction {
    Summarize,
    ExplainSelection,
    Translate,
    RephraseSelection,
    ExtractActionItems,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DownloadAction {
    Pause,
    Resume,
    Remove,
}

#[derive(Default, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub enum SidebarSection {
    #[default]
    Setup,
    Feeds,
    Search,
    AI,
    Downloads,
    Bookmarks,
    History,
    More,
}

impl SidebarSection {
    pub fn title(&self) -> &str {
        match self {
            Self::Setup => "Sidebar Setup",
            Self::Feeds => "Feeds",
            Self::Search => "Search Pages",
            Self::AI => "AI Assistant",
            Self::Downloads => "Downloads",
            Self::Bookmarks => "Bookmarks",
            Self::History => "History",
            Self::More => "More",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SidebarState {
    #[serde(skip)]
    pub visible: bool,
    #[serde(skip)]
    pub section: SidebarSection,
    #[serde(skip)]
    pub pending_download_cmd: Option<(u64, DownloadAction)>,
    #[serde(skip)]
    pub pending_ai_action: Option<AIAction>,
    pub workspaces_enabled: bool,
    pub boosts_enabled: bool,
    pub break_reminder_enabled: bool,
    pub chatgpt_enabled: bool,
    pub messenger_enabled: bool,
    pub whatsapp_enabled: bool,
    pub discord_enabled: bool,
    pub telegram_enabled: bool,
    pub signal_enabled: bool,
    pub instagram_enabled: bool,
    pub x_enabled: bool,
    pub bluesky_enabled: bool,
    pub player_enabled: bool,
    pub speed_dial_enabled: bool,
    pub bookmarks_enabled: bool,
    pub history_enabled: bool,
    pub downloads_enabled: bool,
    pub extensions_enabled: bool,
    pub settings_enabled: bool,
    pub show_sidebar: bool,
    pub auto_hide: bool,
    pub notifications_enabled: bool,
    #[serde(skip)]
    pub messengers_expanded: bool,
    #[serde(skip)]
    pub add_ext_hover: bool,
    #[serde(skip)]
    pub trackers_blocked: u32,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            visible: true,
            section: SidebarSection::Setup,
            pending_download_cmd: None,
            pending_ai_action: None,
            workspaces_enabled: false,
            boosts_enabled: true,
            break_reminder_enabled: true,
            chatgpt_enabled: true,
            messenger_enabled: false,
            whatsapp_enabled: false,
            discord_enabled: false,
            telegram_enabled: false,
            signal_enabled: false,
            instagram_enabled: false,
            x_enabled: false,
            bluesky_enabled: false,
            player_enabled: false,
            speed_dial_enabled: true,
            bookmarks_enabled: true,
            history_enabled: true,
            downloads_enabled: true,
            extensions_enabled: false,
            settings_enabled: true,
            show_sidebar: true,
            auto_hide: false,
            notifications_enabled: true,
            messengers_expanded: false,
            add_ext_hover: false,
            trackers_blocked: 0,
        }
    }
}

impl SidebarState {
    const SETTINGS_KEY: &str = "chrome.sidebar.state";

    pub fn load_from_storage(storage: &StorageDatabase) -> Self {
        if let Some(SettingValue::Json(json)) = storage.settings.load(Self::SETTINGS_KEY)
            && let Ok(state) = serde_json::from_value(json)
        {
            return state;
        }
        Self::default()
    }

    pub fn save_to_storage(&self, storage: &StorageDatabase) {
        if let Ok(json) = serde_json::to_value(self) {
            let _ = storage.settings.store(Self::SETTINGS_KEY, &SettingValue::Json(json));
        }
    }

    pub fn render_icon_rail(&mut self, ui: &mut egui::Ui, theme: &FortrustTheme, anim: &mut SidebarAnimation) {
        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, CornerRadius::ZERO, theme.surface_rail);

        // Auto-hide: when the rail is closed and the user hovers over it,
        // automatically open the sidebar. This is the classic browser-sidebar
        // "peek on hover" behavior.
        if self.auto_hide && !anim.is_open() && ui.rect_contains_pointer(rect) {
            anim.open();
        }

        let rail_left = rect.min.x;
        let mut y = rect.min.y + 8.0;

        let icon_color = theme.text_secondary;
        let active_color = theme.accent_primary;

        let is_open = anim.is_open();

        if Self::rail_btn(ui, theme, rail_left, &mut y, is_open && self.section == SidebarSection::Setup) {
            self.handle_click(SidebarSection::Setup, anim);
        }
        let icon_rect = Rect::from_min_size(Pos2::new(rail_left + 3.0, y - 24.0), Vec2::new(24.0, 24.0));
        icons::paint_setup_icon(ui.painter(), icon_rect, if is_open && self.section == SidebarSection::Setup { active_color } else { icon_color });

        y += 2.0;
        if Self::rail_btn(ui, theme, rail_left, &mut y, is_open && self.section == SidebarSection::Feeds) {
            self.handle_click(SidebarSection::Feeds, anim);
        }
        let icon_rect = Rect::from_min_size(Pos2::new(rail_left + 3.0, y - 24.0), Vec2::new(24.0, 24.0));
        icons::paint_feeds_icon(ui.painter(), icon_rect, if is_open && self.section == SidebarSection::Feeds { active_color } else { icon_color });

        y += 2.0;
        if Self::rail_btn(ui, theme, rail_left, &mut y, is_open && self.section == SidebarSection::Search) {
            self.handle_click(SidebarSection::Search, anim);
        }
        let icon_rect = Rect::from_min_size(Pos2::new(rail_left + 3.0, y - 24.0), Vec2::new(24.0, 24.0));
        icons::paint_search_pages_icon(ui.painter(), icon_rect, if is_open && self.section == SidebarSection::Search { active_color } else { icon_color });

        y += 2.0;
        if Self::rail_btn(ui, theme, rail_left, &mut y, is_open && self.section == SidebarSection::AI) {
            self.handle_click(SidebarSection::AI, anim);
        }
        let icon_rect = Rect::from_min_size(Pos2::new(rail_left + 3.0, y - 24.0), Vec2::new(24.0, 24.0));
        icons::paint_ai_icon(ui.painter(), icon_rect, if is_open && self.section == SidebarSection::AI { active_color } else { icon_color });

        y += 2.0;
        if Self::rail_btn(ui, theme, rail_left, &mut y, is_open && self.section == SidebarSection::Downloads) {
            self.handle_click(SidebarSection::Downloads, anim);
        }
        let icon_rect = Rect::from_min_size(Pos2::new(rail_left + 3.0, y - 24.0), Vec2::new(24.0, 24.0));
        icons::paint_downloads_icon(ui.painter(), icon_rect, if is_open && self.section == SidebarSection::Downloads { active_color } else { icon_color });

        y += 2.0;
        if Self::rail_btn(ui, theme, rail_left, &mut y, is_open && self.section == SidebarSection::Bookmarks) {
            self.handle_click(SidebarSection::Bookmarks, anim);
        }
        let icon_rect = Rect::from_min_size(Pos2::new(rail_left + 3.0, y - 24.0), Vec2::new(24.0, 24.0));
        icons::paint_bookmark_icon(ui.painter(), icon_rect, if is_open && self.section == SidebarSection::Bookmarks { active_color } else { icon_color });

        y += 2.0;
        if Self::rail_btn(ui, theme, rail_left, &mut y, is_open && self.section == SidebarSection::History) {
            self.handle_click(SidebarSection::History, anim);
        }
        let icon_rect = Rect::from_min_size(Pos2::new(rail_left + 3.0, y - 24.0), Vec2::new(24.0, 24.0));
        icons::paint_history_icon(ui.painter(), icon_rect, if is_open && self.section == SidebarSection::History { active_color } else { icon_color });

        y += 80.0;
        ui.painter().line_segment(
            [Pos2::new(rail_left + 7.0, y), Pos2::new(rail_left + 17.0, y)],
            Stroke::new(1.0, theme.glass_border),
        );
        y += 8.0;

        if Self::rail_btn(ui, theme, rail_left, &mut y, is_open && self.section == SidebarSection::More) {
            self.handle_click(SidebarSection::More, anim);
        }
        let icon_rect = Rect::from_min_size(Pos2::new(rail_left + 3.0, y - 24.0), Vec2::new(24.0, 24.0));
        icons::paint_more_icon(ui.painter(), icon_rect, if is_open && self.section == SidebarSection::More { active_color } else { icon_color });
    }

    fn handle_click(&mut self, section: SidebarSection, anim: &mut SidebarAnimation) {
        let is_open = anim.is_open();
        if is_open && self.section == section {
            anim.close();
        } else {
            self.section = section;
            anim.open();
        }
    }

    fn rail_btn(ui: &mut egui::Ui, theme: &FortrustTheme, rl: f32, y: &mut f32, active: bool) -> bool {
        let rect = Rect::from_min_size(Pos2::new(rl + 3.0, *y), Vec2::new(24.0, 24.0));
        let bg = if active { Color32::from_rgba_unmultiplied(79, 158, 255, 33) }
                 else if ui.rect_contains_pointer(rect) { theme.glass_hover }
                 else { Color32::TRANSPARENT };
        ui.painter().rect_filled(rect, CornerRadius::same(5), bg);
        let resp = ui.allocate_rect(rect, egui::Sense::click());
        if active {
            ui.painter().circle_filled(Pos2::new(rect.right() - 3.0, rect.top() + 3.0), 2.5, theme.accent_primary);
        }
        *y += 24.0;
        resp.clicked()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_overlay(&mut self, ui: &mut egui::Ui, theme: &FortrustTheme, anim: &mut SidebarAnimation, config: &mut BrowserConfig, storage: Option<&StorageDatabase>, downloads: &[crate::download::DownloadEntry], workspaces: &mut WorkspaceManager, local_search_query: &mut String, local_search_results: &[fortrust_search::local_index::IndexedDocument]) -> Option<String> {
        let offset = anim.current_offset();
        if offset < 1.0 { return None; }

        let area = ui.max_rect();
        let sbr = Rect::from_min_size(Pos2::new(area.min.x, area.min.y), Vec2::new(offset, area.height()));

        // Click-outside-to-close: a transparent scrim over the area to the
        // right of the sidebar. Clicking it dismisses the sidebar (and the
        // caller should consume the click so it doesn't navigate).
        let scrim = Rect::from_min_size(
            Pos2::new(sbr.max.x, sbr.min.y),
            Vec2::new((area.max.x - sbr.max.x).max(0.0), area.height()),
        );
        if scrim.width() > 0.0 {
            let alpha = (anim.scrim_alpha() * 110.0).round().clamp(0.0, 255.0) as u8;
            if alpha > 0 {
                ui.painter().rect_filled(scrim, CornerRadius::ZERO, Color32::from_black_alpha(alpha));
            }
            let resp = ui.allocate_rect(scrim, egui::Sense::click());
            if resp.clicked() {
                anim.close();
            }
        }

        ui.painter().rect_filled(sbr, CornerRadius::ZERO, theme.surface_sidebar);
        ui.painter().rect_stroke(sbr, CornerRadius::ZERO, Stroke::new(1.0, theme.border_subtle), egui::StrokeKind::Inside);

        // Gradient shadow on sidebar right edge
        let shadow_right = sbr.right();
        for i in 0..8 {
            let alpha = (60 - i * 7).max(0) as u8;
            let x = shadow_right + i as f32 * 1.5;
            ui.painter().rect_filled(
                Rect::from_min_size(Pos2::new(x, sbr.top()), Vec2::new(1.5, sbr.height())),
                CornerRadius::ZERO,
                Color32::from_black_alpha(alpha),
            );
        }

        let sx = sbr.min.x + 18.0;

        // Header
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(sbr.min.x, sbr.min.y), Vec2::new(sbr.width(), 44.0)), CornerRadius::ZERO, theme.surface_sidebar);
        ui.painter().line_segment([Pos2::new(sbr.min.x, sbr.min.y + 44.0), Pos2::new(sbr.max.x, sbr.min.y + 44.0)], Stroke::new(1.0, theme.border_subtle));

        ui.painter().text(Pos2::new(sx, sbr.min.y + 16.0), egui::Align2::LEFT_TOP, self.section.title(), egui::FontId::proportional(13.5), theme.text_primary);

        let cr = Rect::from_min_size(Pos2::new(sbr.max.x - 30.0, sbr.min.y + 10.0), Vec2::new(22.0, 22.0));
        if ui.rect_contains_pointer(cr) { ui.painter().rect_filled(cr, CornerRadius::same(4), theme.glass_hover); }
        if ui.allocate_rect(cr, egui::Sense::click()).clicked() { anim.close(); }
        icons::paint_close_icon(ui.painter(), cr, theme.text_muted);

        // Only render scrollable content when sidebar is wide enough to avoid negative-size panic
        if offset < 36.0 { return None; }

        // Scrollable content
        let content_rect = Rect::from_min_size(
            Pos2::new(sbr.min.x, sbr.min.y + 56.0),
            Vec2::new(sbr.width(), (sbr.height() - 56.0).max(0.0)),
        );
        let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect).layout(egui::Layout::top_down(egui::Align::LEFT)));

        let scroll_result = egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(&mut content_ui, |ui| {
                ui.add_space(0.0);
                let sx = sbr.min.x + 18.0;
                let sw = (sbr.width() - 36.0).max(0.0);

                match self.section {
                    SidebarSection::Setup => {
                        section_label_ui(ui, theme, "Workspaces", sx, sw);
                        self.workspaces_enabled = toggle_row_ui(ui, theme, "", self.workspaces_enabled, sx, sw);
                        if self.workspaces_enabled {
                            render_workspace_list_ui(ui, theme, workspaces, sx, sw);
                        }

                        section_label_ui(ui, theme, "Mindfulness Features", sx, sw);
                        self.boosts_enabled = icon_check_row_ui(ui, theme, "Boosts", self.boosts_enabled, sx, sw, icons::paint_sun_icon);
                        self.break_reminder_enabled = icon_check_row_ui(ui, theme, "Take a Break", self.break_reminder_enabled, sx, sw, icons::paint_timer_icon);

                        section_label_ui(ui, theme, "AI Services", sx, sw);
                        self.chatgpt_enabled = icon_check_row_ui(ui, theme, "ChatGPT", self.chatgpt_enabled, sx, sw, icons::paint_face_icon);

                        section_label_ui(ui, theme, "Messengers", sx, sw);
                        self.messenger_enabled = icon_check_row_ui(ui, theme, "Facebook Messenger", self.messenger_enabled, sx, sw, icons::paint_chat_icon);
                        self.whatsapp_enabled = icon_check_row_ui(ui, theme, "WhatsApp", self.whatsapp_enabled, sx, sw, icons::paint_globe_icon);
                        self.discord_enabled = icon_check_row_ui(ui, theme, "Discord", self.discord_enabled, sx, sw, icons::paint_at_icon);

                        let more_txt = if self.messengers_expanded { "Show less" } else { "Show more" };
                        if show_more_btn_ui(ui, theme, more_txt, sx, sw) {
                            self.messengers_expanded ^= true;
                        }
                        if self.messengers_expanded {
                            self.telegram_enabled = icon_check_row_ui(ui, theme, "Telegram", self.telegram_enabled, sx, sw, icons::paint_at_icon);
                            self.signal_enabled = icon_check_row_ui(ui, theme, "Signal", self.signal_enabled, sx, sw, icons::paint_signal_icon);
                        }

                        section_label_ui(ui, theme, "Social Media", sx, sw);
                        self.instagram_enabled = icon_check_row_ui(ui, theme, "Instagram", self.instagram_enabled, sx, sw, icons::paint_camera_icon);
                        self.x_enabled = icon_check_row_ui(ui, theme, "X", self.x_enabled, sx, sw, icons::paint_x_icon);
                        self.bluesky_enabled = icon_check_row_ui(ui, theme, "Bluesky", self.bluesky_enabled, sx, sw, icons::paint_heart_icon);

                        section_label_ui(ui, theme, "Special Features", sx, sw);
                        self.player_enabled = icon_check_row_ui(ui, theme, "Player", self.player_enabled, sx, sw, icons::paint_compass_icon);

                        section_label_ui(ui, theme, "Browser Tools", sx, sw);
                        self.speed_dial_enabled = icon_check_row_ui(ui, theme, "Speed Dial", self.speed_dial_enabled, sx, sw, icons::paint_grid_icon);
                        self.bookmarks_enabled = icon_check_row_ui(ui, theme, "Bookmarks", self.bookmarks_enabled, sx, sw, icons::paint_bookmark_icon);
                        self.history_enabled = icon_check_row_ui(ui, theme, "History", self.history_enabled, sx, sw, icons::paint_history_icon);
                        self.downloads_enabled = icon_check_row_ui(ui, theme, "Downloads", self.downloads_enabled, sx, sw, icons::paint_downloads_icon);
                        self.extensions_enabled = icon_check_row_ui(ui, theme, "Extensions", self.extensions_enabled, sx, sw, icons::paint_puzzle_icon);
                        self.settings_enabled = icon_check_row_ui(ui, theme, "Settings", self.settings_enabled, sx, sw, icons::paint_gear_icon);
                        let settings_nav_rect = Rect::from_min_size(Pos2::new(sx, ui.cursor().min.y - 4.0), Vec2::new(sw, 20.0));
                        if ui.rect_contains_pointer(settings_nav_rect) {
                            ui.painter().rect_filled(settings_nav_rect, CornerRadius::same(4), Color32::from_white_alpha(4));
                        }
                        ui.painter().text(Pos2::new(sx + 22.0, settings_nav_rect.center().y), egui::Align2::LEFT_CENTER, "Open full settings page", egui::FontId::proportional(11.0), theme.accent_primary);
                        if ui.allocate_rect(settings_nav_rect, egui::Sense::click()).clicked() {
                            ui.memory_mut(|mem| mem.data.insert_temp::<String>(egui::Id::new("sidebar_navigate"), "fortrust://settings".to_owned()));
                        }
                        ui.allocate_space(Vec2::new(sw, 24.0));

                        section_label_ui(ui, theme, "Sidebar Extensions", sx, sw);
                        add_ext_btn_ui(ui, theme, sx, sw);

                        section_label_ui(ui, theme, "Settings", sx, sw);
                        self.show_sidebar = toggle_row_ui(ui, theme, "Show sidebar", self.show_sidebar, sx, sw);
                        self.auto_hide = toggle_row_ui(ui, theme, "Automatically hide sidebar", self.auto_hide, sx, sw);
                        self.notifications_enabled = toggle_row_ui(ui, theme, "Enable notifications for messengers", self.notifications_enabled, sx, sw);
                        ui.memory_mut(|mem| {
                            let val = mem.data.get_temp::<String>(egui::Id::new("sidebar_navigate"));
                            if val.is_some() {
                                mem.data.remove::<String>(egui::Id::new("sidebar_navigate"));
                            }
                            val
                        })
                    }
                    SidebarSection::Feeds => {
                        section_label_ui(ui, theme, "Feed Sources", sx, sw);
                        self.speed_dial_enabled = icon_check_row_ui(ui, theme, "RSS Feeds", self.speed_dial_enabled, sx, sw, icons::paint_feeds_icon);
                        self.bookmarks_enabled = icon_check_row_ui(ui, theme, "Newsletters", self.bookmarks_enabled, sx, sw, icons::paint_bookmark_icon);
                        section_label_ui(ui, theme, "Updates", sx, sw);
                        ui.painter().text(Pos2::new(sx, ui.cursor().min.y + 4.0), egui::Align2::LEFT_TOP, "No feeds configured", egui::FontId::proportional(12.0), theme.text_muted);
                        ui.allocate_space(Vec2::new(sw, 24.0));
                        None
                    }
                    SidebarSection::Search => {
                        let mut clicked_url: Option<String> = None;
                        let input_rect = Rect::from_min_size(Pos2::new(sx, ui.cursor().min.y), Vec2::new(sw, 26.0));
                        ui.painter().rect_filled(input_rect, CornerRadius::same(6), Color32::from_rgba_unmultiplied(255, 255, 255, 10));
                        let _ = ui.new_child(egui::UiBuilder::new().max_rect(input_rect).layout(egui::Layout::left_to_right(egui::Align::Center)))
                            .add(egui::TextEdit::singleline(local_search_query)
                                .hint_text("Search indexed pages...")
                                .frame(false)
                                .desired_width(sw - 12.0)
                                .font(egui::FontId::proportional(11.0))
                                .text_color(theme.text_secondary));
                        ui.allocate_space(Vec2::new(sw, 30.0));

                        for doc in local_search_results {
                            let y = ui.cursor().min.y;
                            let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 52.0));
                            let hovered = ui.rect_contains_pointer(rect);
                            if hovered {
                                ui.painter().rect_filled(rect, CornerRadius::same(4), Color32::from_white_alpha(6));
                            }
                            let icon_rect = Rect::from_min_size(Pos2::new(sx + 2.0, y + 5.0), Vec2::new(14.0, 14.0));
                            icons::paint_search_pages_icon(ui.painter(), icon_rect, if hovered { theme.text_primary } else { theme.text_secondary });
                            let display_t = if doc.title.len() > 30 { format!("{}...", &doc.title[..27]) } else { doc.title.clone() };
                            ui.painter().text(Pos2::new(sx + 22.0, y + 2.0), egui::Align2::LEFT_TOP, &display_t, egui::FontId::proportional(12.0), theme.text_secondary);
                            let url_display = if doc.url.len() > 36 { format!("{}...", &doc.url[..33]) } else { doc.url.clone() };
                            ui.painter().text(Pos2::new(sx + 22.0, y + 16.0), egui::Align2::LEFT_TOP, &url_display, egui::FontId::proportional(9.5), theme.text_muted);
                            let snippet = if doc.content_snippet.len() > 60 { format!("{}...", &doc.content_snippet[..57]) } else { doc.content_snippet.clone() };
                            ui.painter().text(Pos2::new(sx + 22.0, y + 30.0), egui::Align2::LEFT_TOP, &snippet, egui::FontId::proportional(9.5), Color32::from_rgba_unmultiplied(150, 150, 150, 255));
                            if ui.allocate_rect(rect, egui::Sense::click()).clicked() {
                                clicked_url = Some(doc.url.clone());
                            }
                            ui.allocate_space(Vec2::new(sw, 56.0));
                        }
                        if local_search_results.is_empty() && !local_search_query.is_empty() {
                            let cy = ui.cursor().min.y;
                            ui.painter().text(Pos2::new(sx, cy + 4.0), egui::Align2::LEFT_TOP, "No matching pages found in the local index.", egui::FontId::proportional(12.0), theme.text_muted);
                            ui.allocate_space(Vec2::new(sw, 30.0));
                        } else if local_search_results.is_empty() {
                            let cy = ui.cursor().min.y;
                            ui.painter().text(Pos2::new(sx, cy + 4.0), egui::Align2::LEFT_TOP, "Type a query above to search your visited pages.", egui::FontId::proportional(12.0), theme.text_muted);
                            ui.allocate_space(Vec2::new(sw, 30.0));
                        }
                        clicked_url
                    }
                    SidebarSection::AI => {
                        section_label_ui(ui, theme, "AI Providers", sx, sw);
                        self.chatgpt_enabled = icon_check_row_ui(ui, theme, "ChatGPT", self.chatgpt_enabled, sx, sw, icons::paint_face_icon);
                        {
                            use egui::Align2;
                            let y = ui.cursor().min.y;
                            let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 28.0));
                            let hovered = ui.rect_contains_pointer(rect);
                            if hovered { ui.painter().rect_filled(rect.shrink2(Vec2::new(0.0, 4.0)), CornerRadius::ZERO, Color32::from_white_alpha(4)); }
                            ui.painter().text(Pos2::new(sx + 26.0, rect.center().y), Align2::LEFT_CENTER, "Claude", egui::FontId::proportional(12.5), if hovered { theme.text_primary } else { theme.text_secondary });
                            let tr = Rect::from_min_size(Pos2::new(rect.max.x - 48.0, rect.center().y - 11.0), Vec2::new(38.0, 22.0));
                            let bg = theme.border_strong;
                            ui.painter().rect_filled(tr, CornerRadius::same(11), bg);
                            ui.painter().circle_filled(Pos2::new(tr.min.x + 11.0, tr.center().y), 8.0, Color32::WHITE);
                            ui.painter().text(Pos2::new(sx + 2.0, rect.center().y), Align2::LEFT_CENTER, "C", egui::FontId::monospace(10.0), theme.text_muted);
                            if ui.allocate_rect(rect, egui::Sense::click()).clicked() { tracing::info!("Claude toggled"); }
                            ui.allocate_space(Vec2::new(sw, 32.0));
                        }
                        {
                            use egui::Align2;
                            let y = ui.cursor().min.y;
                            let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 28.0));
                            let hovered = ui.rect_contains_pointer(rect);
                            if hovered { ui.painter().rect_filled(rect.shrink2(Vec2::new(0.0, 4.0)), CornerRadius::ZERO, Color32::from_white_alpha(4)); }
                            ui.painter().text(Pos2::new(sx + 26.0, rect.center().y), Align2::LEFT_CENTER, "Gemini", egui::FontId::proportional(12.5), if hovered { theme.text_primary } else { theme.text_secondary });
                            let tr = Rect::from_min_size(Pos2::new(rect.max.x - 48.0, rect.center().y - 11.0), Vec2::new(38.0, 22.0));
                            let bg = theme.border_strong;
                            ui.painter().rect_filled(tr, CornerRadius::same(11), bg);
                            ui.painter().circle_filled(Pos2::new(tr.min.x + 11.0, tr.center().y), 8.0, Color32::WHITE);
                            ui.painter().text(Pos2::new(sx + 2.0, rect.center().y), Align2::LEFT_CENTER, "G", egui::FontId::monospace(10.0), theme.text_muted);
                            if ui.allocate_rect(rect, egui::Sense::click()).clicked() { tracing::info!("Gemini toggled"); }
                            ui.allocate_space(Vec2::new(sw, 32.0));
                        }
                        {
                            use egui::Align2;
                            let y = ui.cursor().min.y;
                            let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 28.0));
                            let hovered = ui.rect_contains_pointer(rect);
                            if hovered { ui.painter().rect_filled(rect.shrink2(Vec2::new(0.0, 4.0)), CornerRadius::ZERO, Color32::from_white_alpha(4)); }
                            ui.painter().text(Pos2::new(sx + 26.0, rect.center().y), Align2::LEFT_CENTER, "Local LLM", egui::FontId::proportional(12.5), if hovered { theme.text_primary } else { theme.text_secondary });
                            let tr = Rect::from_min_size(Pos2::new(rect.max.x - 48.0, rect.center().y - 11.0), Vec2::new(38.0, 22.0));
                            let bg = theme.border_strong;
                            ui.painter().rect_filled(tr, CornerRadius::same(11), bg);
                            ui.painter().circle_filled(Pos2::new(tr.min.x + 11.0, tr.center().y), 8.0, Color32::WHITE);
                            ui.painter().text(Pos2::new(sx + 2.0, rect.center().y), Align2::LEFT_CENTER, "L", egui::FontId::monospace(10.0), theme.text_muted);
                            if ui.allocate_rect(rect, egui::Sense::click()).clicked() { tracing::info!("Local LLM toggled"); }
                            ui.allocate_space(Vec2::new(sw, 32.0));
                        }
                        section_label_ui(ui, theme, "Quick Actions", sx, sw);
                        type ActionEntry = (&'static str, AIAction, fn(&egui::Painter, Rect, Color32));
                        let actions: &[ActionEntry] = &[
                            ("Summarize page", AIAction::Summarize, icons::paint_feeds_icon as fn(&egui::Painter, Rect, Color32)),
                            ("Explain selection", AIAction::ExplainSelection, icons::paint_bookmark_icon),
                            ("Translate page", AIAction::Translate, icons::paint_globe_icon),
                            ("Rephrase selection", AIAction::RephraseSelection, icons::paint_gear_icon),
                            ("Extract action items", AIAction::ExtractActionItems, icons::paint_grid_icon),
                        ];
                        for (label, action, icon_fn) in actions {
                            let cy = ui.cursor().min.y;
                            let rect = Rect::from_min_size(Pos2::new(sx, cy), Vec2::new(sw, 28.0));
                            let hovered = ui.rect_contains_pointer(rect);
                            if hovered { ui.painter().rect_filled(rect.shrink2(Vec2::new(0.0, 4.0)), CornerRadius::ZERO, Color32::from_white_alpha(4)); }
                            let icon_rect = Rect::from_min_size(Pos2::new(sx + 2.0, rect.center().y - 7.0), Vec2::new(14.0, 14.0));
                            icon_fn(ui.painter(), icon_rect, if hovered { theme.text_primary } else { theme.text_secondary });
                            ui.painter().text(Pos2::new(sx + 22.0, rect.center().y), egui::Align2::LEFT_CENTER, *label, egui::FontId::proportional(12.0), if hovered { theme.text_primary } else { theme.text_secondary });
                            if ui.allocate_rect(rect, egui::Sense::click()).clicked() {
                                self.pending_ai_action = Some(action.clone());
                                tracing::info!(target: "fortrust.ai", "AI quick action triggered: {}", label);
                            }
                            ui.allocate_space(Vec2::new(sw, 32.0));
                        }
                        None
                    }
                    SidebarSection::Downloads => {
                        use crate::download::DownloadStatus;
                        section_label_ui(ui, theme, "Downloads", sx, sw);
                        if downloads.is_empty() {
                            let cy = ui.cursor().min.y;
                            ui.painter().text(Pos2::new(sx, cy + 4.0), egui::Align2::LEFT_TOP, "No downloads yet.", egui::FontId::proportional(12.0), theme.text_muted);
                            ui.allocate_space(Vec2::new(sw, 30.0));
                        } else {
                            for dl in downloads {
                                let y = ui.cursor().min.y;
                                let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 48.0));
                                let hovered = ui.rect_contains_pointer(rect);
                                if hovered {
                                    ui.painter().rect_filled(rect, CornerRadius::same(4), Color32::from_white_alpha(6));
                                }

                                // Download icon
                                let icon_rect = Rect::from_min_size(Pos2::new(sx + 2.0, y + 4.0), Vec2::new(14.0, 14.0));
                                icons::paint_downloads_icon(ui.painter(), icon_rect, if hovered { theme.text_primary } else { theme.text_secondary });

                                // Filename
                                let fname = if dl.filename.len() > 24 { format!("{}...", &dl.filename[..21]) } else { dl.filename.clone() };
                                ui.painter().text(Pos2::new(sx + 22.0, y + 2.0), egui::Align2::LEFT_TOP, &fname, egui::FontId::proportional(12.0), theme.text_secondary);

                                // Status/progress
                                let status_text = match dl.status {
                                    DownloadStatus::Downloading => {
                                        let pct = if dl.total_bytes > 0 { dl.downloaded_bytes as f64 / dl.total_bytes as f64 * 100.0 } else { 0.0 };
                                        let speed_mb = dl.speed_bytes_per_sec / 1024.0 / 1024.0;
                                        format!("{:.0}% ({:.1} MB/s)", pct, speed_mb)
                                    }
                                    DownloadStatus::Paused => format!("Paused ({}/{})", dl.downloaded_bytes / 1024, (dl.total_bytes.max(dl.downloaded_bytes)) / 1024),
                                    DownloadStatus::Completed => "Completed".into(),
                                    DownloadStatus::Failed(ref e) => format!("Failed: {}", if e.len() > 20 { &e[..20] } else { e }),
                                    DownloadStatus::Queued => "Queued".into(),
                                };
                                ui.painter().text(Pos2::new(sx + 22.0, y + 18.0), egui::Align2::LEFT_TOP, &status_text, egui::FontId::proportional(9.5), theme.text_muted);

                                // Progress bar for active downloads
                                if matches!(dl.status, DownloadStatus::Downloading) && dl.total_bytes > 0 {
                                    let bar_rect = Rect::from_min_size(Pos2::new(sx + 22.0, y + 32.0), Vec2::new(sw - 60.0, 4.0));
                                    ui.painter().rect_filled(bar_rect, CornerRadius::same(2), Color32::from_rgba_unmultiplied(255, 255, 255, 16));
                                    let fill_w = (dl.downloaded_bytes as f32 / dl.total_bytes as f32) * bar_rect.width();
                                    if fill_w > 0.0 {
                                        ui.painter().rect_filled(Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_rect.height())), CornerRadius::same(2), theme.accent_primary);
                                    }
                                }

                                // Action buttons (pause/resume/remove) on hover
                                if hovered {
                                    let can_pause = matches!(dl.status, DownloadStatus::Downloading);
                                    let can_resume = matches!(dl.status, DownloadStatus::Paused | DownloadStatus::Failed(_));
                                    let btn_rect = Rect::from_min_size(Pos2::new(rect.max.x - 58.0, y + 6.0), Vec2::new(50.0, 20.0));
                                    if can_pause {
                                        if ui.allocate_rect(btn_rect, egui::Sense::click()).clicked() {
                                            self.pending_download_cmd = Some((dl.id, DownloadAction::Pause));
                                        }
                                        ui.painter().rect_filled(btn_rect, CornerRadius::same(3), Color32::from_rgba_unmultiplied(255, 180, 50, 30));
                                        ui.painter().text(btn_rect.center(), egui::Align2::CENTER_CENTER, "Pause", egui::FontId::proportional(10.0), Color32::from_rgb(255, 180, 50));
                                    } else if can_resume {
                                        if ui.allocate_rect(btn_rect, egui::Sense::click()).clicked() {
                                            self.pending_download_cmd = Some((dl.id, DownloadAction::Resume));
                                        }
                                        ui.painter().rect_filled(btn_rect, CornerRadius::same(3), Color32::from_rgba_unmultiplied(50, 200, 80, 30));
                                        ui.painter().text(btn_rect.center(), egui::Align2::CENTER_CENTER, "Resume", egui::FontId::proportional(10.0), Color32::from_rgb(50, 200, 80));
                                    }
                                    let del_rect = Rect::from_min_size(Pos2::new(rect.max.x - 24.0, y + 28.0), Vec2::new(20.0, 18.0));
                                    if ui.allocate_rect(del_rect, egui::Sense::click()).clicked() {
                                        self.pending_download_cmd = Some((dl.id, DownloadAction::Remove));
                                    }
                                    icons::paint_close_icon(ui.painter(), del_rect, theme.text_muted);
                                }

                                ui.allocate_space(Vec2::new(sw, 52.0));
                            }
                        }
                        None
                    }
                    SidebarSection::Bookmarks => {
                        let bookmarks: Vec<fortrust_storage::Bookmark> = storage.map(|s| s.bookmarks.all()).unwrap_or_default();
                        let mut deleted: Option<String> = None;
                        for bm in &bookmarks {
                            let y = ui.cursor().min.y;
                            let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 32.0));
                            let hovered = ui.rect_contains_pointer(rect);
                            if hovered {
                                ui.painter().rect_filled(rect, CornerRadius::same(4), Color32::from_white_alpha(6));
                            }
                            // Bookmark icon
                            let icon_rect = Rect::from_min_size(Pos2::new(sx + 2.0, y + 9.0), Vec2::new(14.0, 14.0));
                            icons::paint_bookmark_icon(ui.painter(), icon_rect, if hovered { theme.text_primary } else { theme.text_secondary });
                            // Title
                            let display = if bm.title.len() > 28 { format!("{}...", &bm.title[..25]) } else { bm.title.clone() };
                            ui.painter().text(Pos2::new(sx + 22.0, y + 6.0), egui::Align2::LEFT_TOP, &display, egui::FontId::proportional(12.0), theme.text_secondary);
                            // URL
                            let url_display = if bm.url.len() > 34 { format!("{}...", &bm.url[..31]) } else { bm.url.clone() };
                            ui.painter().text(Pos2::new(sx + 22.0, y + 20.0), egui::Align2::LEFT_TOP, &url_display, egui::FontId::proportional(9.5), theme.text_muted);
                            // Delete button
                            let del_rect = Rect::from_min_size(Pos2::new(rect.max.x - 26.0, y + 6.0), Vec2::new(20.0, 20.0));
                            let del_hovered = ui.rect_contains_pointer(del_rect);
                            if del_hovered { ui.painter().rect_filled(del_rect, CornerRadius::same(3), Color32::from_rgba_unmultiplied(255, 60, 60, 30)); }
                            icons::paint_close_icon(ui.painter(), del_rect, if del_hovered { theme.accent_shield_off } else { theme.text_muted });
                            if ui.allocate_rect(del_rect, egui::Sense::click()).clicked() {
                                deleted = Some(bm.id.clone());
                            }
                            ui.allocate_space(Vec2::new(sw, 36.0));
                        }
                        if bookmarks.is_empty() {
                            let cy = ui.cursor().min.y;
                            ui.painter().text(Pos2::new(sx, cy + 4.0), egui::Align2::LEFT_TOP, "No bookmarks yet. Click the star in the address bar to add one.", egui::FontId::proportional(12.0), theme.text_muted);
                            ui.allocate_space(Vec2::new(sw, 30.0));
                        }
                        if let Some(id) = deleted
                            && let Some(s) = storage {
                            let _ = s.bookmarks.delete(&id);
                        }
                        None
                    }
                    SidebarSection::History => {
                        let search_query = ui.memory_mut(|mem| {
                            mem.data.get_temp::<String>(egui::Id::new("sidebar_history_search"))
                                .unwrap_or_default()
                        });
                        let mut new_query = search_query.clone();
                        let input_rect = Rect::from_min_size(Pos2::new(sx, ui.cursor().min.y), Vec2::new(sw, 26.0));
                        ui.painter().rect_filled(input_rect, CornerRadius::same(6), Color32::from_rgba_unmultiplied(255, 255, 255, 10));
                        let _ = ui.new_child(egui::UiBuilder::new().max_rect(input_rect).layout(egui::Layout::left_to_right(egui::Align::Center)))
                            .add(egui::TextEdit::singleline(&mut new_query)
                                .hint_text("Search history...")
                                .frame(false)
                                .desired_width(sw - 12.0)
                                .font(egui::FontId::proportional(11.0))
                                .text_color(theme.text_secondary));
                        ui.allocate_space(Vec2::new(sw, 30.0));
                        ui.memory_mut(|mem| {
                            mem.data.insert_temp::<String>(egui::Id::new("sidebar_history_search"), new_query.clone());
                        });

                        let hist_query = fortrust_storage::HistoryQuery::new(new_query.trim()).with_limit(100);
                        let history: Vec<fortrust_storage::HistoryEntry> = storage.and_then(|s| s.history.search(&hist_query).ok()).unwrap_or_default();
                        let mut clicked_url: Option<String> = None;
                        for entry in &history {
                            let y = ui.cursor().min.y;
                            let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 32.0));
                            let hovered = ui.rect_contains_pointer(rect);
                            if hovered {
                                ui.painter().rect_filled(rect, CornerRadius::same(4), Color32::from_white_alpha(6));
                            }
                            // History icon
                            let icon_rect = Rect::from_min_size(Pos2::new(sx + 2.0, y + 9.0), Vec2::new(14.0, 14.0));
                            icons::paint_history_icon(ui.painter(), icon_rect, if hovered { theme.text_primary } else { theme.text_secondary });
                            // Title
                            let display = if entry.title.len() > 28 { format!("{}...", &entry.title[..25]) } else { entry.title.clone() };
                            ui.painter().text(Pos2::new(sx + 22.0, y + 4.0), egui::Align2::LEFT_TOP, &display, egui::FontId::proportional(12.0), theme.text_secondary);
                            // URL
                            let url_display = if entry.url.len() > 34 { format!("{}...", &entry.url[..31]) } else { entry.url.clone() };
                            ui.painter().text(Pos2::new(sx + 22.0, y + 18.0), egui::Align2::LEFT_TOP, &url_display, egui::FontId::proportional(9.5), theme.text_muted);
                            // Click to navigate
                            if ui.allocate_rect(rect, egui::Sense::click()).clicked() {
                                clicked_url = Some(entry.url.clone());
                            }
                            ui.allocate_space(Vec2::new(sw, 36.0));
                        }
                        if history.is_empty() {
                            let cy = ui.cursor().min.y;
                            ui.painter().text(Pos2::new(sx, cy + 4.0), egui::Align2::LEFT_TOP, "No history yet. Browsing the web will record pages here.", egui::FontId::proportional(12.0), theme.text_muted);
                            ui.allocate_space(Vec2::new(sw, 30.0));
                        }
                        clicked_url
                    }
                    SidebarSection::More => {
                        section_label_ui(ui, theme, "Appearance", sx, sw);
                        let current_theme = config.ui.theme.clone();
                        let is_dark = current_theme == "dark";
                        let new_dark = toggle_row_ui(ui, theme, "Dark theme", is_dark, sx, sw);
                        if new_dark != is_dark {
                            config.ui.theme = if new_dark { "dark".into() } else { "light".into() };
                            if let Some(s) = storage { let _ = s.settings.store("chrome.ui.theme", &fortrust_storage::SettingValue::from(config.ui.theme.as_str())); }
                        }
                        let current_compact = config.ui.compact_density;
                        let new_compact = toggle_row_ui(ui, theme, "Compact density", current_compact, sx, sw);
                        if new_compact != current_compact {
                            config.ui.compact_density = new_compact;
                            if let Some(s) = storage { let _ = s.settings.store("chrome.ui.compact_density", &fortrust_storage::SettingValue::from(if new_compact { "1" } else { "0" })); }
                        }

                        section_label_ui(ui, theme, "Privacy", sx, sw);
                        let cur_block_ads = config.privacy.block_ads;
                        let new_block_ads = toggle_row_ui(ui, theme, "Block ads", cur_block_ads, sx, sw);
                        if new_block_ads != cur_block_ads {
                            config.privacy.block_ads = new_block_ads;
                            if let Some(s) = storage { let _ = s.settings.store("chrome.privacy.block_ads", &fortrust_storage::SettingValue::from(if new_block_ads { "1" } else { "0" })); }
                        }
                        let cur_block_trk = config.privacy.block_trackers;
                        let new_block_trk = toggle_row_ui(ui, theme, "Block trackers", cur_block_trk, sx, sw);
                        if new_block_trk != cur_block_trk {
                            config.privacy.block_trackers = new_block_trk;
                            if let Some(s) = storage { let _ = s.settings.store("chrome.privacy.block_trackers", &fortrust_storage::SettingValue::from(if new_block_trk { "1" } else { "0" })); }
                        }
                        let cur_https = config.privacy.https_only_mode;
                        let new_https = toggle_row_ui(ui, theme, "HTTPS-only mode", cur_https, sx, sw);
                        if new_https != cur_https {
                            config.privacy.https_only_mode = new_https;
                            if let Some(s) = storage { let _ = s.settings.store("chrome.privacy.https_only_mode", &fortrust_storage::SettingValue::from(if new_https { "1" } else { "0" })); }
                        }
                        let cur_fp = config.privacy.fingerprint_noise;
                        let new_fp = toggle_row_ui(ui, theme, "Fingerprint protection", cur_fp, sx, sw);
                        if new_fp != cur_fp {
                            config.privacy.fingerprint_noise = new_fp;
                            if let Some(s) = storage { let _ = s.settings.store("chrome.privacy.fingerprint_noise", &fortrust_storage::SettingValue::from(if new_fp { "1" } else { "0" })); }
                        }

                        section_label_ui(ui, theme, "Sidebar", sx, sw);
                        self.show_sidebar = toggle_row_ui(ui, theme, "Show sidebar", self.show_sidebar, sx, sw);
                        self.auto_hide = toggle_row_ui(ui, theme, "Auto-hide sidebar", self.auto_hide, sx, sw);
                        None
                    }
                }
            });
        let result = scroll_result.inner;
        // Persist sidebar state after any toggle changes
        if let Some(s) = storage {
            self.save_to_storage(s);
        }
        result
    }
}

fn section_label_ui(ui: &mut egui::Ui, theme: &FortrustTheme, label: &str, sx: f32, sw: f32) {
    let y = ui.cursor().min.y;
    ui.painter().text(Pos2::new(sx, y + 4.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(11.0), theme.text_muted);
    ui.allocate_space(Vec2::new(sw, 24.0));
}

fn icon_check_row_ui(ui: &mut egui::Ui, theme: &FortrustTheme, label: &str, value: bool, sx: f32, sw: f32, icon_fn: fn(&egui::Painter, Rect, Color32)) -> bool {
    check_row_icon_ui(ui, theme, label, value, sx, sw, Some(icon_fn)).0
}

fn check_row_icon_ui(
    ui: &mut egui::Ui, theme: &FortrustTheme, label: &str, mut value: bool,
    sx: f32, sw: f32, icon: Option<fn(&egui::Painter, Rect, Color32)>,
) -> (bool, bool) {
    let y = ui.cursor().min.y;
    let icon_offset = if icon.is_some() { 24.0 } else { 0.0 };
    let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 28.0)).expand2(Vec2::new(0.0, 2.0));
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect.shrink2(Vec2::new(0.0, 4.0)), CornerRadius::ZERO, Color32::from_white_alpha(4));
    }

    let hovered = ui.rect_contains_pointer(rect);

    // Draw icon if provided
    if let Some(icon_fn) = icon {
        let icon_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + 2.0, rect.center().y - 7.0),
            Vec2::new(14.0, 14.0),
        );
        icon_fn(ui.painter(), icon_rect, if hovered { theme.text_primary } else { theme.text_secondary });
    }

    ui.painter().text(
        Pos2::new(rect.min.x + 2.0 + icon_offset, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.5),
        theme.text_secondary,
    );

    let clicked = ui.allocate_rect(rect, egui::Sense::click()).clicked();
    if clicked { value = !value; }

    let cr = Rect::from_min_size(Pos2::new(rect.max.x - 28.0, rect.center().y - 9.0), Vec2::new(18.0, 18.0));
    let cb = if value { Color32::from_rgba_unmultiplied(255, 255, 255, 25) } else { Color32::from_rgba_unmultiplied(255, 255, 255, 15) };
    let cbo = if value { Color32::from_rgba_unmultiplied(255, 255, 255, 72) } else { Color32::from_rgba_unmultiplied(255, 255, 255, 36) };
    ui.painter().rect_filled(cr, CornerRadius::same(4), cb);
    ui.painter().rect_stroke(cr, CornerRadius::same(4), Stroke::new(1.5, cbo), egui::StrokeKind::Inside);
    if value {
        let check_rect = Rect::from_center_size(cr.center(), Vec2::new(14.0, 14.0));
        icons::paint_check_icon(ui.painter(), check_rect, theme.text_primary);
    }

    ui.allocate_space(Vec2::new(sw, 34.0));
    (value, clicked)
}

fn toggle_row_ui(ui: &mut egui::Ui, theme: &FortrustTheme, label: &str, mut value: bool, sx: f32, sw: f32) -> bool {
    let y = ui.cursor().min.y;
    let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 36.0));
    ui.painter().text(Pos2::new(rect.min.x, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.5), theme.text_secondary);

    let tr = Rect::from_min_size(Pos2::new(rect.max.x - 48.0, rect.center().y - 11.0), Vec2::new(38.0, 22.0));
    let bg = if value { theme.accent_primary } else { theme.border_strong };
    ui.painter().rect_filled(tr, CornerRadius::same(11), bg);
    let kx = if value { tr.max.x - 19.0 } else { tr.min.x + 3.0 };
    ui.painter().circle_filled(Pos2::new(kx + 8.0, tr.center().y), 8.0, Color32::WHITE);

    if ui.allocate_rect(rect, egui::Sense::click()).clicked() { value = !value; }
    ui.allocate_space(Vec2::new(sw, 40.0));
    value
}

fn show_more_btn_ui(ui: &mut egui::Ui, theme: &FortrustTheme, label: &str, sx: f32, sw: f32) -> bool {
    let y = ui.cursor().min.y;
    let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 24.0));
    ui.painter().text(Pos2::new(rect.min.x, rect.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), theme.accent_primary);
    let clicked = ui.allocate_rect(rect, egui::Sense::click()).clicked();
    ui.allocate_space(Vec2::new(sw, 30.0));
    // If clicked, return the inverse of the current visual state
    clicked
}

fn add_ext_btn_ui(ui: &mut egui::Ui, theme: &FortrustTheme, sx: f32, sw: f32) {
    let y = ui.cursor().min.y;
    let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 30.0));
    let hovered = ui.rect_contains_pointer(rect);
    let bg = if hovered { theme.surface_card } else { theme.surface_deepest };
    ui.painter().rect_filled(rect, CornerRadius::same(20), bg);
    ui.painter().rect_stroke(rect, CornerRadius::same(20), Stroke::new(1.0, theme.border_strong), egui::StrokeKind::Inside);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Add extension", egui::FontId::proportional(12.0), theme.text_primary);
    let _ = ui.allocate_rect(rect, egui::Sense::click());
    ui.allocate_space(Vec2::new(sw, 40.0));
}

fn render_workspace_list_ui(ui: &mut egui::Ui, theme: &FortrustTheme, workspaces: &mut WorkspaceManager, sx: f32, sw: f32) {
    let active_id = workspaces.active();
    let ws_list: Vec<_> = workspaces
        .all()
        .iter()
        .map(|ws| (ws.id, ws.name.to_string(), ws.color_hex.to_string(), ws.tab_ids.len()))
        .collect();
    let has_multiple = ws_list.len() > 1;

    for (ws_id, name, color_hex, tab_count) in &ws_list {
        let y = ui.cursor().min.y;
        let is_active = *ws_id == active_id;
        let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 30.0));

        let bg = if is_active {
            theme.accent_primary.gamma_multiply(0.15)
        } else if ui.rect_contains_pointer(rect) {
            theme.surface_card
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(6), bg);

        // Color dot
        let dot_color = parse_hex_color(color_hex).unwrap_or(theme.accent_primary);
        ui.painter().circle_filled(Pos2::new(sx + 8.0, rect.center().y), 4.0, dot_color);

        // Name
        ui.painter().text(Pos2::new(sx + 20.0, rect.center().y), egui::Align2::LEFT_CENTER, name.as_str(), egui::FontId::proportional(12.0), theme.text_primary);

        // Tab count
        let count_label = format!("{tab_count} tabs");
        ui.painter().text(Pos2::new(rect.right() - 4.0, rect.center().y), egui::Align2::RIGHT_CENTER, &count_label, egui::FontId::proportional(10.0), theme.text_muted);

        if ui.allocate_rect(rect, egui::Sense::click()).clicked() && !is_active {
            workspaces.activate(*ws_id);
        }
        ui.allocate_space(Vec2::new(sw, 32.0));
    }

    if has_multiple {
        // Delete workspace button
        let y = ui.cursor().min.y;
        let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 24.0));
        if ui.rect_contains_pointer(rect) {
            ui.painter().rect_filled(rect, CornerRadius::same(4), theme.glass_hover);
        }
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Remove workspace", egui::FontId::proportional(11.0), theme.accent_danger);
        if ui.allocate_rect(rect, egui::Sense::click()).clicked() {
            let ids: Vec<WorkspaceId> = ws_list.iter().filter(|(id, _, _, _)| *id != WorkspaceId(1)).map(|(id, _, _, _)| *id).collect();
            for id in ids {
                workspaces.delete(id);
            }
        }
    }

    // Add workspace button
    let y = ui.cursor().min.y;
    let rect = Rect::from_min_size(Pos2::new(sx, y), Vec2::new(sw, 24.0));
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect, CornerRadius::same(4), theme.glass_hover);
    }
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "+ New workspace", egui::FontId::proportional(11.0), theme.accent_primary);
    if ui.allocate_rect(rect, egui::Sense::click()).clicked() {
        let count = workspaces.all().len();
        workspaces.create(format!("Workspace {}", count + 1), "#4d9fff");
    }
    ui.allocate_space(Vec2::new(sw, 36.0));
}

fn parse_hex_color(hex: &str) -> Option<Color32> {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Color32::from_rgb(r, g, b))
    } else {
        None
    }
}
