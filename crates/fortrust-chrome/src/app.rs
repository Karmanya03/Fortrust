use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use eframe::egui::{self, Color32, Context, CornerRadius, Frame, Margin, Stroke, Vec2};
use fortrust_core::{
    BlockReason, BrowserConfig, PrivacyConfig, PrivacyEngine, RequestContext, ResourceType, TabId,
    TabManager,
};
use fortrust_storage::{HistoryEntry, StorageDatabase};
use trust_engine::{Color, DisplayCommand, EnginePage, EngineRect, TrustEngine, Viewport};

use crate::{
    animation::SidebarAnimation,
    omnibox::OmniboxState,
    shield::ShieldState,
    sidebar::{self, SidebarPage},
    speed_dial::SpeedDialState,
    theme::FortrustTheme,
};

pub struct FortrustApp {
    #[allow(dead_code)]
    config: BrowserConfig,
    tabs: TabManager,
    privacy: PrivacyEngine,
    internal_engine: TrustEngine,
    engine_worker: EngineWorker,
    storage: Option<StorageDatabase>,
    tab_pages: HashMap<TabId, TabPageState>,
    request_owner: HashMap<u64, TabId>,

    // Opera Air UI state
    pub omnibox: OmniboxState,
    pub sidebar_anim: SidebarAnimation,
    pub sidebar_page: SidebarPage,
    pub speed_dial: SpeedDialState,
    pub shield: ShieldState,
    pub theme: FortrustTheme,
    pub theme_mode: ThemeMode,

    // Performance
    pub total_memory_mb: f32,
    pub memory_anim: f32,
    pub last_frame: std::time::Instant,
    pub animation_phase: f32,
    pub needs_new_tab: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ThemeMode {
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryMode {
    Push,
    Replace,
}

#[derive(Debug, Default)]
struct TabPageState {
    page: Option<EnginePage>,
    load_error: Option<String>,
    loading_url: Option<String>,
    request_id: Option<u64>,
    history: Vec<String>,
    history_index: usize,
}

impl TabPageState {
    fn new(url: impl Into<String>) -> Self {
        Self {
            history: vec![url.into()],
            ..Self::default()
        }
    }

    fn current_url(&self) -> Option<&str> {
        self.history.get(self.history_index).map(String::as_str)
    }

    fn push_history(&mut self, url: String) {
        if self.current_url() == Some(url.as_str()) {
            return;
        }
        self.history.truncate(self.history_index.saturating_add(1));
        self.history.push(url);
        self.history_index = self.history.len().saturating_sub(1);
    }

    fn replace_history(&mut self, url: String) {
        if self.history.is_empty() {
            self.history.push(url);
            self.history_index = 0;
        } else if let Some(current) = self.history.get_mut(self.history_index) {
            *current = url;
        }
    }

    fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    fn can_go_forward(&self) -> bool {
        self.history_index + 1 < self.history.len()
    }

    fn go_back(&mut self) -> Option<String> {
        if !self.can_go_back() {
            return None;
        }
        self.history_index -= 1;
        self.current_url().map(str::to_owned)
    }

    fn go_forward(&mut self) -> Option<String> {
        if !self.can_go_forward() {
            return None;
        }
        self.history_index += 1;
        self.current_url().map(str::to_owned)
    }

    fn begin_load(&mut self, url: String, request_id: u64) {
        self.page = None;
        self.load_error = None;
        self.loading_url = Some(url);
        self.request_id = Some(request_id);
    }

    fn finish_load(&mut self, request_id: u64, page: EnginePage) -> bool {
        if self.request_id != Some(request_id) {
            return false;
        }
        self.replace_history(page.url.clone());
        self.page = Some(page);
        self.load_error = None;
        self.loading_url = None;
        self.request_id = None;
        true
    }

    fn fail_load(&mut self, request_id: u64, error: String) -> bool {
        if self.request_id != Some(request_id) {
            return false;
        }
        self.page = None;
        self.load_error = Some(error);
        self.loading_url = None;
        self.request_id = None;
        true
    }

    fn clear_document(&mut self) {
        self.page = None;
        self.load_error = None;
        self.loading_url = None;
        self.request_id = None;
    }
}

struct EngineWorker {
    sender: Sender<EngineCommand>,
    receiver: Receiver<EngineEvent>,
    next_request_id: u64,
}

enum EngineCommand {
    Load {
        request_id: u64,
        url: String,
        viewport: Viewport,
    },
}

enum EngineEvent {
    Loaded {
        request_id: u64,
        page: Box<EnginePage>,
    },
    Failed {
        request_id: u64,
        url: String,
        error: String,
    },
}

impl EngineWorker {
    fn spawn(privacy: PrivacyConfig) -> Self {
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();

        thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    drain_failed_worker(command_receiver, event_sender, error.to_string());
                    return;
                }
            };

            let mut engine = match TrustEngine::secure_networked(privacy) {
                Ok(engine) => engine,
                Err(error) => {
                    drain_failed_worker(command_receiver, event_sender, format!("{error:?}"));
                    return;
                }
            };

            while let Ok(command) = command_receiver.recv() {
                match command {
                    EngineCommand::Load {
                        request_id,
                        url,
                        viewport,
                    } => {
                        let result = runtime.block_on(engine.load_url(url.clone(), viewport));
                        let event = match result {
                            Ok(page) => EngineEvent::Loaded {
                                request_id,
                                page: Box::new(page),
                            },
                            Err(error) => EngineEvent::Failed {
                                request_id,
                                url,
                                error: format!("{error:?}"),
                            },
                        };
                        let _ = event_sender.send(event);
                    }
                }
            }
        });

        Self {
            sender: command_sender,
            receiver: event_receiver,
            next_request_id: 1,
        }
    }

    fn load(&mut self, url: String, viewport: Viewport) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        let _ = self.sender.send(EngineCommand::Load {
            request_id,
            url,
            viewport,
        });
        request_id
    }
}

fn drain_failed_worker(
    command_receiver: Receiver<EngineCommand>,
    event_sender: Sender<EngineEvent>,
    error: String,
) {
    while let Ok(command) = command_receiver.recv() {
        let EngineCommand::Load {
            request_id, url, ..
        } = command;
        let _ = event_sender.send(EngineEvent::Failed {
            request_id,
            url,
            error: error.clone(),
        });
    }
}

impl FortrustApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        let theme = FortrustTheme::dark();
        apply_egui_style(&creation_context.egui_ctx, &theme);

        let config = BrowserConfig::default();
        let mut tabs = TabManager::new(config.performance.clone());
        let start_id = tabs.open_tab("fortrust://start", "Speed Dial", false);
        let mut tab_pages = HashMap::new();
        tab_pages.insert(start_id, TabPageState::new("fortrust://start"));

        let storage = Self::open_storage();

        Self {
            privacy: PrivacyEngine::new(config.privacy.clone()),
            internal_engine: TrustEngine::offline(),
            engine_worker: EngineWorker::spawn(config.privacy.clone()),
            storage,
            config,
            tabs,
            tab_pages,
            request_owner: HashMap::new(),

            omnibox: OmniboxState::default(),
            sidebar_anim: SidebarAnimation::new(),
            sidebar_page: SidebarPage::Tabs,
            speed_dial: SpeedDialState::default(),
            shield: ShieldState::default(),
            theme,
            theme_mode: ThemeMode::Dark,

            total_memory_mb: 0.0,
            memory_anim: 0.0,
            last_frame: std::time::Instant::now(),
            animation_phase: 0.0,
            needs_new_tab: false,
        }
    }

    fn open_storage() -> Option<StorageDatabase> {
        let base = if cfg!(target_os = "windows") {
            std::env::var("APPDATA")
                .ok()
                .map(|p| format!("{}\\Fortrust", p))
        } else {
            std::env::var("HOME")
                .ok()
                .map(|p| format!("{}/.local/share/fortrust", p))
        };
        match base {
            Some(dir) => {
                let path = format!("{}\\storage.redb", dir);
                let _ = std::fs::create_dir_all(&dir);
                Some(StorageDatabase::open_or_default(path))
            }
            None => {
                tracing::warn!("No data directory found, running without persistent storage");
                None
            }
        }
    }

    fn active_tab_id(&self) -> Option<TabId> {
        self.tabs.active_id()
    }

    fn active_state(&self) -> Option<&TabPageState> {
        self.active_tab_id().and_then(|id| self.tab_pages.get(&id))
    }

    fn tab_state_mut(&mut self, id: TabId) -> &mut TabPageState {
        self.tab_pages
            .entry(id)
            .or_insert_with(|| TabPageState::new("fortrust://start"))
    }

    fn navigate_input(&mut self, input: String, history_mode: HistoryMode) {
        let target = normalize_input(&input);
        let top_level = self.tabs.active_tab().map(|tab| tab.url.to_string());
        let decision = self.privacy.inspect(&RequestContext {
            url: target,
            top_level_url: top_level,
            resource_type: ResourceType::Document,
        });

        match decision.blocked {
            Some(reason) => self.show_blocked_page(decision.original_url, reason, history_mode),
            None => {
                let effective = decision
                    .effective_url
                    .unwrap_or_else(|| decision.original_url.clone());
                self.commit_navigation(effective, history_mode);
            }
        }
    }

    fn show_blocked_page(
        &mut self,
        original_url: String,
        _reason: BlockReason,
        history_mode: HistoryMode,
    ) {
        let Some(tab_id) = self.active_tab_id() else {
            return;
        };
        self.shield.ads_blocked = self.shield.ads_blocked.saturating_add(1);
        self.tabs.record_privacy_block(tab_id);
        self.tabs
            .navigate_tab(tab_id, "fortrust://blocked", "Blocked");
        self.omnibox.text = original_url;

        let state = self.tab_state_mut(tab_id);
        match history_mode {
            HistoryMode::Push => state.push_history("fortrust://blocked".to_owned()),
            HistoryMode::Replace => state.replace_history("fortrust://blocked".to_owned()),
        }
        state.clear_document();
    }

    fn commit_navigation(&mut self, url: String, history_mode: HistoryMode) {
        let Some(tab_id) = self.active_tab_id() else {
            return;
        };
        let title = title_from_url(&url);
        self.tabs.navigate_tab(tab_id, url.clone(), title);
        self.omnibox.text = url.clone();

        let state = self.tab_state_mut(tab_id);
        match history_mode {
            HistoryMode::Push => state.push_history(url.clone()),
            HistoryMode::Replace => state.replace_history(url.clone()),
        }

        if url.starts_with("fortrust://") || url.starts_with("about:") {
            state.clear_document();
            return;
        }

        let request_id = self.engine_worker.load(
            url.clone(),
            Viewport {
                width: 1180.0,
                height: 760.0,
            },
        );
        self.request_owner.insert(request_id, tab_id);
        self.tab_state_mut(tab_id).begin_load(url, request_id);
    }

    fn go_back(&mut self) {
        let Some(tab_id) = self.active_tab_id() else {
            return;
        };
        if let Some(url) = self.tab_state_mut(tab_id).go_back() {
            self.omnibox.text = url.clone();
            self.navigate_input(url, HistoryMode::Replace);
        }
    }

    fn go_forward(&mut self) {
        let Some(tab_id) = self.active_tab_id() else {
            return;
        };
        if let Some(url) = self.tab_state_mut(tab_id).go_forward() {
            self.omnibox.text = url.clone();
            self.navigate_input(url, HistoryMode::Replace);
        }
    }

    fn reload(&mut self) {
        let url = self
            .active_state()
            .and_then(TabPageState::current_url)
            .map(str::to_owned)
            .unwrap_or_else(|| self.omnibox.text.clone());
        self.navigate_input(url, HistoryMode::Replace);
    }

    fn can_go_back(&self) -> bool {
        self.active_state().is_some_and(TabPageState::can_go_back)
    }

    fn can_go_forward(&self) -> bool {
        self.active_state()
            .is_some_and(TabPageState::can_go_forward)
    }

    fn open_new_tab(&mut self) {
        let id = self.tabs.open_tab("fortrust://start", "Speed Dial", false);
        self.tab_pages
            .insert(id, TabPageState::new("fortrust://start"));
        self.omnibox.text = "fortrust://start".to_owned();
        self.needs_new_tab = true;
    }

    fn close_tab(&mut self, id: TabId) {
        self.tabs.close_tab(id);
        self.tab_pages.remove(&id);
        self.request_owner.retain(|_, owner| *owner != id);
        if self.tabs.tabs().is_empty() {
            self.open_new_tab();
        }
        if let Some(tab) = self.tabs.active_tab() {
            self.omnibox.text = tab.url.to_string();
        }
    }

    fn poll_loading(&mut self, ctx: &Context) {
        while let Ok(event) = self.engine_worker.receiver.try_recv() {
            match event {
                EngineEvent::Loaded { request_id, page } => {
                    let Some(tab_id) = self.request_owner.remove(&request_id) else {
                        continue;
                    };
                    let page = *page;
                    let title = page.title.clone();
                    let url = page.url.clone();
                    let accepted = self.tab_state_mut(tab_id).finish_load(request_id, page);
                    if accepted {
                        self.tabs.navigate_tab(tab_id, url.clone(), title.clone());
                        if self.active_tab_id() == Some(tab_id) {
                            self.omnibox.text = url.clone();
                        }
                        if !url.starts_with("fortrust://")
                            && !url.starts_with("about:")
                            && let Some(ref storage) = self.storage
                        {
                            let entry = HistoryEntry {
                                url: url.clone(),
                                title,
                                visit_time: Utc::now(),
                                visit_count: 1,
                                typed_count: 0,
                                is_bookmarked: false,
                            };
                            let _ = storage.history.store(&entry);
                        }
                    }
                }
                EngineEvent::Failed {
                    request_id,
                    url,
                    error,
                } => {
                    let Some(tab_id) = self.request_owner.remove(&request_id) else {
                        continue;
                    };
                    let err_msg = error.clone();
                    let accepted = self.tab_state_mut(tab_id).fail_load(request_id, error);
                    if accepted {
                        tracing::warn!("Load failed for {}: {}", url, err_msg);
                    }
                }
            }
        }

        if self
            .tab_pages
            .values()
            .any(|state| state.loading_url.is_some())
        {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn render_toolbar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("fortrust_toolbar")
            .exact_height(44.0)
            .frame(Frame {
                fill: self.theme.glass_bg,
                inner_margin: Margin::symmetric(8, 6),
                stroke: Stroke::new(0.5, self.theme.glass_border),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Back/Forward/Reload
                    let back = self.can_go_back();
                    let fwd = self.can_go_forward();
                    if nav_button(ui, "◀", back, &self.theme).clicked() {
                        self.go_back();
                    }
                    if nav_button(ui, "▶", fwd, &self.theme).clicked() {
                        self.go_forward();
                    }
                    if nav_button(ui, "⟳", true, &self.theme).clicked() {
                        self.reload();
                    }

                    // Omnibox
                    ui.add_space(8.0);
                    if let Some(url) = self.omnibox.render(ui, &self.theme) {
                        self.navigate_input(url, HistoryMode::Push);
                    }

                    // Shield button
                    ui.add_space(8.0);
                    self.shield.render_button(ui, &self.theme);
                });
            });
    }

    fn render_tab_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("fortrust_tab_bar")
            .exact_height(34.0)
            .frame(Frame {
                fill: self.theme.glass_bg,
                inner_margin: Margin::symmetric(4, 2),
                stroke: Stroke::new(0.5, self.theme.glass_border),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let active = self.tabs.active_id();
                    let mut clicked = None;
                    let mut closed = None;

                    for tab in self.tabs.tabs() {
                        let selected = Some(tab.id) == active;
                        let busy = self
                            .tab_pages
                            .get(&tab.id)
                            .is_some_and(|state| state.loading_url.is_some());

                        let label = if busy {
                            format!("{} ...", tab.title)
                        } else {
                            tab.title.to_string()
                        };

                        let fill = if selected {
                            self.theme.accent_primary
                        } else {
                            Color32::TRANSPARENT
                        };

                        let response = ui.add(
                            egui::Button::new(egui::RichText::new(&label).size(11.0).color(
                                if selected {
                                    self.theme.text_on_accent
                                } else {
                                    self.theme.text_secondary
                                },
                            ))
                            .fill(fill)
                            .stroke(Stroke::NONE)
                            .corner_radius(6)
                            .min_size(Vec2::new(80.0, 26.0)),
                        );

                        if response.clicked() {
                            clicked = Some(tab.id);
                        }

                        let close_resp = ui.add(
                            egui::Button::new(
                                egui::RichText::new("✕")
                                    .size(9.0)
                                    .color(self.theme.text_secondary),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(4)
                            .min_size(Vec2::new(18.0, 18.0)),
                        );

                        if close_resp.clicked() {
                            closed = Some(tab.id);
                        }
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("+")
                                    .size(13.0)
                                    .color(self.theme.text_primary),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(6)
                            .min_size(Vec2::new(26.0, 26.0)),
                        )
                        .clicked()
                    {
                        self.open_new_tab();
                    }

                    if let Some(id) = clicked {
                        self.tabs.activate(id);
                        if let Some(tab) = self.tabs.active_tab() {
                            self.omnibox.text = tab.url.to_string();
                        }
                    }
                    if let Some(id) = closed {
                        self.close_tab(id);
                    }
                });
            });
    }

    fn render_central_panel(&mut self, ctx: &Context) {
        let mut navigate: Option<String> = None;

        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show(ctx, |ui| {
                let active_url = self
                    .active_state()
                    .and_then(TabPageState::current_url)
                    .map(str::to_owned)
                    .or_else(|| self.tabs.active_tab().map(|tab| tab.url.to_string()))
                    .unwrap_or_else(|| "fortrust://start".to_owned());

                let dt = self.last_frame.elapsed().as_secs_f32().min(0.05);

                if active_url == "fortrust://start" {
                    if let Some(url) =
                        self.speed_dial
                            .render(ui, &self.theme, dt, &mut self.needs_new_tab)
                    {
                        navigate = Some(url);
                    }
                } else if active_url == "fortrust://blocked" {
                    self.render_blocked_page(ui);
                } else {
                    self.render_web_content(ui, &active_url);
                }
            });

        if let Some(url) = navigate {
            self.navigate_input(url, HistoryMode::Push);
        }
    }

    fn render_blocked_page(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(96.0);
            ui.label(
                egui::RichText::new("🛡 Request Blocked")
                    .size(30.0)
                    .strong()
                    .color(self.theme.accent_shield_off),
            );
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Fortrust stopped this navigation before it left the browser.")
                    .color(self.theme.text_secondary),
            );
        });
    }

    fn render_web_content(&mut self, ui: &mut egui::Ui, url: &str) {
        let Some(tab_id) = self.active_tab_id() else {
            return;
        };

        let is_loading = self
            .tab_pages
            .get(&tab_id)
            .map(|s| s.loading_url.as_deref() == Some(url))
            .unwrap_or(false);
        if is_loading {
            self.render_loading(ui, url);
            return;
        }

        let load_error = self
            .tab_pages
            .get(&tab_id)
            .and_then(|s| s.load_error.as_deref().map(|e| e.to_owned()));
        if let Some(error) = load_error {
            self.render_error(ui, url, &error);
            return;
        }

        let page_url = self
            .tab_pages
            .get(&tab_id)
            .and_then(|s| s.page.as_ref().map(|p| p.url.clone()));
        if let Some(ref page_url_val) = page_url
            && page_url_val == url
            && let Some(page) = &self.tab_pages.get(&tab_id).and_then(|s| s.page.as_ref())
        {
            self.paint_engine_page(ui, page);
            return;
        }

        let viewport = Viewport {
            width: ui.available_width().max(320.0),
            height: ui.available_height().max(240.0),
        };
        if let Ok(page) = self
            .internal_engine
            .internal_page("fortrust://empty", viewport)
        {
            self.paint_engine_page(ui, &page);
        } else {
            self.render_error(ui, url, "no rendered page is available for this tab");
        }
    }

    fn render_loading(&mut self, ui: &mut egui::Ui, url: &str) {
        let rect = ui.max_rect();
        let center = egui::Pos2::new(rect.center().x, rect.top() + rect.height() * 0.42);
        let phase = self.animation_phase;
        let radius = 26.0 + (phase.sin() * 0.5 + 0.5) * 10.0;
        ui.painter().circle_filled(
            center,
            radius + 14.0,
            Color32::from_rgba_unmultiplied(130, 100, 255, 32),
        );
        ui.painter()
            .circle_filled(center, radius, self.theme.accent_primary);
        ui.painter().text(
            egui::Pos2::new(center.x, center.y + 64.0),
            egui::Align2::CENTER_CENTER,
            "Fortrust is loading",
            egui::FontId::proportional(28.0),
            self.theme.text_primary,
        );
        ui.painter().text(
            egui::Pos2::new(center.x, center.y + 94.0),
            egui::Align2::CENTER_CENTER,
            url,
            egui::FontId::proportional(13.0),
            self.theme.text_secondary,
        );
    }

    fn render_error(&mut self, ui: &mut egui::Ui, url: &str, error: &str) {
        ui.vertical_centered(|ui| {
            ui.add_space(72.0);
            ui.label(
                egui::RichText::new("⛔ Load Error")
                    .size(28.0)
                    .strong()
                    .color(self.theme.accent_shield_off),
            );
            ui.add_space(8.0);
            ui.label(egui::RichText::new(url).color(self.theme.accent_primary));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(error).color(self.theme.text_secondary));
        });
    }

    fn paint_engine_page(&self, ui: &mut egui::Ui, page: &EnginePage) {
        let outer = ui.available_size();
        let (surface, _) = ui.allocate_exact_size(outer, egui::Sense::hover());
        let painter = ui.painter_at(surface);
        painter.rect_filled(surface.shrink(18.0), 22.0, self.theme.glass_bg);

        let content = surface.shrink2(Vec2::new(34.0, 32.0));
        painter.rect_filled(content, 14.0, Color32::WHITE);
        for command in page.rendered.display_list.commands() {
            match command {
                DisplayCommand::FillRect { rect, color } => {
                    painter.rect_filled(
                        to_egui_rect(content.min, *rect),
                        0.0,
                        to_egui_color(*color),
                    );
                }
                DisplayCommand::DrawText {
                    rect,
                    text,
                    color,
                    font_size_px,
                    ..
                } => {
                    painter.text(
                        egui::Pos2::new(content.min.x + rect.x, content.min.y + rect.y),
                        egui::Align2::LEFT_TOP,
                        text,
                        egui::FontId::proportional(*font_size_px),
                        to_egui_color(*color),
                    );
                }
                DisplayCommand::ClipPush(_) | DisplayCommand::ClipPop => {}
            }
        }
    }
}

impl eframe::App for FortrustApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.05);
        self.last_frame = std::time::Instant::now();
        self.animation_phase = ctx.input(|i| i.time as f32 * 1.4);

        // Tick animations
        self.sidebar_anim.tick(dt);
        if !self.sidebar_anim.width.is_settled() {
            ctx.request_repaint();
        }

        // Poll loading engine
        self.poll_loading(ctx);

        // Apply egui style
        apply_egui_style(ctx, &self.theme);

        // Render shield popup first (floats above everything)
        self.shield.render_popup(ctx, &self.theme);

        // Render sidebar
        let tabs = self.tabs.tabs().to_vec();
        let mut active_idx = 0;
        if let Some(active_id) = self.tabs.active_id() {
            active_idx = tabs.iter().position(|t| t.id == active_id).unwrap_or(0);
        }
        egui::SidePanel::left("sidebar")
            .exact_width(self.sidebar_anim.current_width())
            .resizable(false)
            .frame(Frame {
                fill: self.theme.glass_bg,
                inner_margin: Margin::symmetric(0, 0),
                corner_radius: CornerRadius::ZERO,
                stroke: Stroke::new(0.5, self.theme.glass_border),
                ..Default::default()
            })
            .show(ctx, |ui| {
                sidebar::render_sidebar(
                    ui,
                    &mut self.sidebar_anim,
                    &mut self.sidebar_page,
                    &self.theme,
                    &tabs,
                    &mut active_idx,
                );
            });

        // Render tab bar
        self.render_tab_bar(ctx);

        // Render toolbar with omnibox
        self.render_toolbar(ctx);

        // Render central panel
        self.render_central_panel(ctx);

        // Keep repainting for animations
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

fn apply_egui_style(ctx: &egui::Context, theme: &FortrustTheme) {
    let mut style = (*ctx.style()).clone();
    style.visuals.panel_fill = theme.glass_bg;
    style.visuals.window_fill = theme.glass_bg;
    style.visuals.window_stroke = egui::Stroke::new(1.0, theme.glass_border);
    style.spacing.item_spacing = Vec2::new(8.0, 4.0);
    ctx.set_style(style);
}

fn nav_button(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    theme: &FortrustTheme,
) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(text)
                .color(theme.text_secondary)
                .size(13.0),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .corner_radius(6)
        .min_size(Vec2::new(28.0, 28.0)),
    )
}

fn to_egui_rect(origin: egui::Pos2, rect: EngineRect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::Pos2::new(origin.x + rect.x, origin.y + rect.y),
        egui::Vec2::new(rect.width, rect.height),
    )
}

fn to_egui_color(color: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

fn normalize_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("fortrust://")
        || trimmed.starts_with("about:")
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
    {
        trimmed.to_owned()
    } else if looks_like_host_candidate(trimmed) {
        format!("https://{trimmed}")
    } else {
        let query = urlencoding::encode(trimmed);
        format!("https://duckduckgo.com/?q={query}")
    }
}

fn looks_like_host_candidate(input: &str) -> bool {
    if input.is_empty() || input.contains(' ') {
        return false;
    }
    if input.starts_with('[') && input.contains(']') {
        return true;
    }
    if input.eq_ignore_ascii_case("localhost") || input.starts_with("localhost:") {
        return true;
    }
    input.contains('.')
}

fn title_from_url(url: &str) -> String {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .trim_end_matches('/')
        .chars()
        .take(24)
        .collect()
}
