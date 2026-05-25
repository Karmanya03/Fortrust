use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use eframe::egui::{
    self, Align, Align2, Button, CentralPanel, Color32, Context, FontId, Frame, Layout, Margin,
    Pos2, Rect, RichText, Sense, SidePanel, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Visuals,
};
use chrono::Utc;
use fortrust_core::{
    BlockReason, BrowserConfig, PrivacyConfig, PrivacyEngine, PrivacyNote, RequestContext,
    ResourceType, TabId, TabManager, TabStatus,
};
use fortrust_storage::{
    HistoryEntry, StorageDatabase,
};
use trust_engine::{
    Color, DisplayCommand, EnginePage, EngineRect, PageSource, TRUST_ENGINE_NAME, TrustEngine,
    Viewport,
};
use url::form_urlencoded;

const BG: Color32 = Color32::from_rgb(8, 10, 10);
const PANEL: Color32 = Color32::from_rgb(16, 18, 18);
const TEXT: Color32 = Color32::from_rgb(232, 238, 233);
const INK: Color32 = Color32::from_rgb(35, 43, 39);
const MUTED: Color32 = Color32::from_rgb(154, 166, 157);
const ACCENT: Color32 = Color32::from_rgb(120, 207, 150);
const ACCENT_2: Color32 = Color32::from_rgb(188, 220, 111);
const WARN: Color32 = Color32::from_rgb(236, 183, 92);
const DANGER: Color32 = Color32::from_rgb(235, 104, 119);

fn glass() -> Color32 {
    Color32::from_rgba_unmultiplied(225, 234, 224, 178)
}

fn glass_strong() -> Color32 {
    Color32::from_rgba_unmultiplied(239, 244, 236, 215)
}

fn glass_dark() -> Color32 {
    Color32::from_rgba_unmultiplied(20, 25, 23, 126)
}

pub struct FortrustApp {
    config: BrowserConfig,
    tabs: TabManager,
    privacy: PrivacyEngine,
    internal_engine: TrustEngine,
    engine_worker: EngineWorker,
    storage: Option<StorageDatabase>,
    tab_pages: HashMap<TabId, TabPageState>,
    request_owner: HashMap<u64, TabId>,
    omnibox: String,
    start_search: String,
    last_decision: String,
    blocked_this_session: u32,
    show_privacy_panel: bool,
    animation_phase: f32,
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
                            Ok(page) => EngineEvent::Loaded { request_id, page: Box::new(page) },
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
        apply_theme(&creation_context.egui_ctx);

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
            omnibox: "fortrust://start".to_owned(),
            start_search: String::new(),
            last_decision: "Ready".to_owned(),
            blocked_this_session: 0,
            show_privacy_panel: false,
            animation_phase: 0.0,
        }
    }

    fn open_storage() -> Option<StorageDatabase> {
        let base = if cfg!(target_os = "windows") {
            std::env::var("APPDATA").ok().map(|p| format!("{}\\Fortrust", p))
        } else {
            std::env::var("HOME").ok().map(|p| format!("{}/.local/share/fortrust", p))
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

    fn top_chrome(&mut self, ctx: &Context) {
        TopBottomPanel::top("fortrust_top")
            .exact_height(64.0)
            .frame(Frame::new().fill(Color32::from_rgba_unmultiplied(232, 238, 228, 112)))
            .show(ctx, |ui| {
                self.tab_strip(ui);
                self.address_bar(ui);
            });
    }

    fn tab_strip(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let active = self.tabs.active_id();
            let mut clicked = None;
            let mut closed = None;
            for tab in self.tabs.tabs() {
                let selected = Some(tab.id) == active;
                let busy = self
                    .tab_pages
                    .get(&tab.id)
                    .is_some_and(|state| state.loading_url.is_some());
                let label = match tab.status {
                    TabStatus::Suspended { .. } => format!("{} [sleep]", tab.title),
                    TabStatus::Discarded => format!("{} [off]", tab.title),
                    _ if busy => format!("{} ...", tab.title),
                    _ => tab.title.to_string(),
                };
                let fill = if selected { glass_strong() } else { glass() };
                let stroke = if selected {
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 140))
                } else {
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 65))
                };

                if ui
                    .add_sized(
                        [150.0, 25.0],
                        Button::new(RichText::new(label).color(INK).size(12.0))
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(16),
                    )
                    .clicked()
                {
                    clicked = Some(tab.id);
                }
                if ui
                    .add_sized(
                        [22.0, 25.0],
                        Button::new(RichText::new("x").color(INK).size(11.0))
                            .fill(Color32::from_rgba_unmultiplied(238, 244, 236, 92))
                            .stroke(Stroke::NONE)
                            .corner_radius(13),
                    )
                    .clicked()
                {
                    closed = Some(tab.id);
                }
            }

            if ui
                .add_sized(
                    [28.0, 25.0],
                    Button::new(RichText::new("+").color(INK).size(14.0))
                        .fill(glass_strong())
                        .stroke(Stroke::NONE)
                        .corner_radius(14),
                )
                .on_hover_text("New tab")
                .clicked()
            {
                self.open_new_tab();
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("x").color(INK).strong());
                ui.label(RichText::new("[]").color(INK).strong());
                ui.label(RichText::new("-").color(INK).strong());
            });

            if let Some(id) = clicked {
                self.tabs.activate(id);
                if let Some(tab) = self.tabs.active_tab() {
                    self.omnibox = tab.url.to_string();
                }
            }
            if let Some(id) = closed {
                self.close_tab(id);
            }
        });
    }

    fn address_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add_space(34.0);
            if nav_button(ui, "<", "Back", self.can_go_back()).clicked() {
                self.go_back();
            }
            if nav_button(ui, ">", "Forward", self.can_go_forward()).clicked() {
                self.go_forward();
            }
            if nav_button(ui, "R", "Reload", true).clicked() {
                self.reload();
            }

            let width = (ui.available_width() - 212.0).max(240.0);
            let frame = Frame::new()
                .fill(Color32::from_rgba_unmultiplied(236, 242, 232, 155))
                .stroke(Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(255, 255, 255, 78),
                ))
                .corner_radius(17)
                .inner_margin(Margin::symmetric(10, 3));
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Q").color(INK).strong());
                    let response = ui.add_sized(
                        [ui.available_width(), 22.0],
                        TextEdit::singleline(&mut self.omnibox)
                            .hint_text("Enter search or web address")
                            .text_color(INK)
                            .frame(false)
                            .desired_width(f32::INFINITY),
                    );
                    let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
                    if response.lost_focus() && enter_pressed {
                        self.navigate_from_omnibox();
                    }
                });
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                tool_chip(ui, "U", "User");
                tool_chip(ui, "S", "Shield");
                tool_chip(ui, "C", "Controls");
            });
        });
    }

    fn open_new_tab(&mut self) {
        let id = self.tabs.open_tab("fortrust://start", "Speed Dial", false);
        self.tab_pages
            .insert(id, TabPageState::new("fortrust://start"));
        self.omnibox = "fortrust://start".to_owned();
        self.start_search.clear();
    }

    fn close_tab(&mut self, id: TabId) {
        self.tabs.close_tab(id);
        self.tab_pages.remove(&id);
        self.request_owner.retain(|_, owner| *owner != id);
        if self.tabs.tabs().is_empty() {
            self.open_new_tab();
        }
        if let Some(tab) = self.tabs.active_tab() {
            self.omnibox = tab.url.to_string();
        }
    }

    fn navigate_from_omnibox(&mut self) {
        let input = self.omnibox.clone();
        self.navigate_input(input, HistoryMode::Push);
    }

    fn navigate_from_start_search(&mut self) {
        let input = if self.start_search.trim().is_empty() {
            "fortrust://start".to_owned()
        } else {
            self.start_search.clone()
        };
        self.navigate_input(input, HistoryMode::Push);
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
                self.last_decision =
                    summarize_decision(&decision.notes, decision.stripped_query_pairs);
                self.commit_navigation(effective, history_mode);
            }
        }
    }

    fn show_blocked_page(
        &mut self,
        original_url: String,
        reason: BlockReason,
        history_mode: HistoryMode,
    ) {
        let Some(tab_id) = self.active_tab_id() else {
            return;
        };
        self.blocked_this_session = self.blocked_this_session.saturating_add(1);
        self.tabs.record_privacy_block(tab_id);
        self.last_decision = format!("Blocked: {}", block_reason_label(reason));
        self.tabs
            .navigate_tab(tab_id, "fortrust://blocked", "Blocked");
        self.omnibox = original_url;

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
        self.omnibox = url.clone();

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
            self.omnibox = url.clone();
            self.navigate_input(url, HistoryMode::Replace);
        }
    }

    fn go_forward(&mut self) {
        let Some(tab_id) = self.active_tab_id() else {
            return;
        };
        if let Some(url) = self.tab_state_mut(tab_id).go_forward() {
            self.omnibox = url.clone();
            self.navigate_input(url, HistoryMode::Replace);
        }
    }

    fn reload(&mut self) {
        let url = self
            .active_state()
            .and_then(TabPageState::current_url)
            .map(str::to_owned)
            .unwrap_or_else(|| self.omnibox.clone());
        self.navigate_input(url, HistoryMode::Replace);
    }

    fn can_go_back(&self) -> bool {
        self.active_state().is_some_and(TabPageState::can_go_back)
    }

    fn can_go_forward(&self) -> bool {
        self.active_state()
            .is_some_and(TabPageState::can_go_forward)
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
                            self.omnibox = url.clone();
                        }
                        self.last_decision = "Loaded through Trust Engine".to_owned();
                        if !url.starts_with("fortrust://") && !url.starts_with("about:")
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
                    let accepted = self.tab_state_mut(tab_id).fail_load(request_id, error);
                    if accepted {
                        self.last_decision = format!("Load failed for {}", title_from_url(&url));
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

    fn left_rail(&mut self, ctx: &Context) {
        SidePanel::left("fortrust_air_rail")
            .resizable(false)
            .exact_width(42.0)
            .frame(Frame::new().fill(Color32::from_rgba_unmultiplied(232, 238, 228, 112)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    if rail_button(ui, "*", "Speed Dial").clicked() {
                        self.commit_navigation("fortrust://start".to_owned(), HistoryMode::Push);
                    }
                    if rail_button(ui, "S", "Shield").clicked() {
                        self.show_privacy_panel = !self.show_privacy_panel;
                    }
                    ui.add_space(180.0);
                    rail_button(ui, "O", "Focus");
                    rail_button(ui, "=", "Controls");
                    rail_button(ui, "^", "Workspaces");
                    rail_button(ui, "~", "Flow");
                    rail_button(ui, "?", "Help");
                    rail_button(ui, "...", "More");
                });
            });
    }

    fn privacy_panel(&mut self, ctx: &Context) {
        SidePanel::right("privacy_panel")
            .resizable(false)
            .exact_width(300.0)
            .frame(Frame::new().fill(Color32::from_rgba_unmultiplied(10, 13, 12, 215)))
            .show(ctx, |ui| {
                ui.add_space(18.0);
                ui.heading(RichText::new("Shield").color(TEXT).size(22.0));
                ui.label(RichText::new("Strict by default").color(MUTED));
                ui.add_space(16.0);

                metric(ui, "Blocked", self.blocked_this_session.to_string(), DANGER);
                metric(
                    ui,
                    "HTTPS only",
                    enabled_label(self.config.privacy.https_only_mode),
                    ACCENT,
                );
                metric(
                    ui,
                    "Tracker filter",
                    enabled_label(self.config.privacy.block_trackers),
                    ACCENT,
                );
                metric(
                    ui,
                    "3P cookies",
                    if self.config.privacy.block_third_party_cookies {
                        "Blocked"
                    } else {
                        "Allowed"
                    },
                    WARN,
                );

                ui.separator();
                ui.add_space(10.0);
                ui.label(RichText::new("Last decision").color(MUTED));
                ui.label(RichText::new(&self.last_decision).color(TEXT));

                if let Some(page) = self.active_state().and_then(|state| state.page.as_ref()) {
                    ui.add_space(16.0);
                    ui.label(RichText::new(TRUST_ENGINE_NAME).color(MUTED));
                    metric(ui, "Source", source_label(page.security.source), ACCENT_2);
                    metric(
                        ui,
                        "Body",
                        format!("{} KB", page.security.body_bytes.div_ceil(1024)),
                        ACCENT,
                    );
                    metric(
                        ui,
                        "Display list",
                        page.security.display_commands.to_string(),
                        ACCENT,
                    );
                    metric(
                        ui,
                        "JavaScript",
                        if page.security.javascript_enabled {
                            "Enabled"
                        } else {
                            "Disabled"
                        },
                        ACCENT,
                    );
                    metric(
                        ui,
                        "Subresources",
                        if page.security.external_subresources_enabled {
                            "Allowed"
                        } else {
                            "Gated off"
                        },
                        ACCENT,
                    );
                    metric(
                        ui,
                        "Stylesheets",
                        format!(
                            "{} loaded / {} blocked",
                            page.security.external_stylesheets_loaded,
                            page.security.external_stylesheets_blocked
                        ),
                        ACCENT,
                    );
                    metric(
                        ui,
                        "Images",
                        format!(
                            "{} loaded / {} blocked",
                            page.security.external_images_loaded,
                            page.security.external_images_blocked
                        ),
                        ACCENT,
                    );
                }

                ui.add_space(16.0);
                ui.label(RichText::new("Tab memory").color(MUTED));
                let report = self.tabs.memory_report();
                ui.label(
                    RichText::new(format!(
                        "{} active / {} warm / {} sleeping",
                        report.active_tabs, report.warm_tabs, report.suspended_tabs
                    ))
                    .color(TEXT),
                );
                ui.add(
                    egui::ProgressBar::new(
                        report.total_estimated_mb as f32 / report.budget_mb.max(1) as f32,
                    )
                    .fill(ACCENT)
                    .corner_radius(8)
                    .text(format!("{} MB", report.total_estimated_mb)),
                );
            });
    }

    fn page(&mut self, ctx: &Context) {
        CentralPanel::default()
            .frame(Frame::new().fill(BG))
            .show(ctx, |ui| {
                let active_url = self
                    .active_state()
                    .and_then(TabPageState::current_url)
                    .map(str::to_owned)
                    .or_else(|| self.tabs.active_tab().map(|tab| tab.url.to_string()))
                    .unwrap_or_else(|| "fortrust://start".to_owned());

                if active_url == "fortrust://blocked" {
                    paint_forest_background(ui, self.animation_phase);
                    blocked_page(ui);
                } else if active_url.starts_with("fortrust://") {
                    self.start_page(ui);
                } else {
                    paint_document_background(ui, self.animation_phase);
                    self.document_page(ui, &active_url);
                }
            });
    }

    fn start_page(&mut self, ui: &mut Ui) {
        paint_forest_background(ui, self.animation_phase);
        let rect = ui.max_rect();
        let center = Pos2::new(rect.center().x, rect.top() + rect.height() * 0.43);
        let search_rect = Rect::from_center_size(center, Vec2::new(318.0, 48.0));

        ui.painter().rect_filled(
            search_rect,
            24.0,
            Color32::from_rgba_unmultiplied(230, 238, 226, 198),
        );
        ui.painter().text(
            Pos2::new(search_rect.left() + 22.0, search_rect.center().y),
            Align2::CENTER_CENTER,
            "G",
            FontId::proportional(22.0),
            INK,
        );
        let edit_rect = Rect::from_min_max(
            Pos2::new(search_rect.left() + 50.0, search_rect.top() + 8.0),
            Pos2::new(search_rect.right() - 18.0, search_rect.bottom() - 8.0),
        );
        let response = ui.put(
            edit_rect,
            TextEdit::singleline(&mut self.start_search)
                .hint_text("Search the web")
                .text_color(INK)
                .frame(false),
        );
        let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
        if response.lost_focus() && enter_pressed {
            self.navigate_from_start_search();
        }

        let dial_y = search_rect.bottom() + 56.0;
        let dial_x = rect.center().x - 120.0;
        let dials = [
            (
                "O",
                "https://www.opera.com/air",
                Color32::from_rgb(255, 124, 31),
            ),
            (
                "C",
                "https://www.canva.com",
                Color32::from_rgb(111, 135, 220),
            ),
            (
                "B",
                "https://www.behance.net",
                Color32::from_rgb(38, 90, 255),
            ),
            (
                "P",
                "https://www.pinterest.com",
                Color32::from_rgb(230, 55, 62),
            ),
            ("+", "fortrust://start", Color32::from_rgb(172, 184, 166)),
        ];

        for (index, (label, url, color)) in dials.into_iter().enumerate() {
            let center = Pos2::new(dial_x + index as f32 * 72.0, dial_y);
            if speed_dial(ui, center, label, color).clicked() {
                if url == "fortrust://start" {
                    self.open_new_tab();
                } else {
                    self.navigate_input(url.to_owned(), HistoryMode::Push);
                }
            }
        }

        let top_dials = [
            (
                "AI",
                "https://chat.openai.com",
                Color32::from_rgb(22, 22, 22),
            ),
            ("M", "https://mistral.ai", Color32::from_rgb(35, 178, 214)),
        ];
        for (index, (label, url, color)) in top_dials.into_iter().enumerate() {
            let center = Pos2::new(search_rect.right() + 50.0 + index as f32 * 72.0, center.y);
            if speed_dial(ui, center, label, color).clicked() {
                self.navigate_input(url.to_owned(), HistoryMode::Push);
            }
        }

        ui.painter().text(
            Pos2::new(rect.center().x, rect.bottom() - 28.0),
            Align2::CENTER_CENTER,
            "You can't stop the waves, but you can learn to surf",
            FontId::proportional(15.0),
            Color32::from_rgba_unmultiplied(245, 248, 242, 230),
        );

        let feedback = Rect::from_min_size(
            Pos2::new(rect.right() - 132.0, rect.bottom() - 48.0),
            Vec2::new(112.0, 28.0),
        );
        ui.painter().rect_filled(
            feedback,
            14.0,
            Color32::from_rgba_unmultiplied(237, 243, 231, 176),
        );
        ui.painter().text(
            feedback.center(),
            Align2::CENTER_CENTER,
            "Got feedback?",
            FontId::proportional(12.0),
            INK,
        );
    }

    fn document_page(&mut self, ui: &mut Ui, url: &str) {
        let Some(tab_id) = self.active_tab_id() else {
            return;
        };
        let state = self.tab_state_mut(tab_id);

        if state.loading_url.as_deref() == Some(url) {
            loading_page(ui, url, self.animation_phase);
            return;
        }

        if let Some(error) = state.load_error.as_deref() {
            document_error(ui, url, error);
            return;
        }

        if let Some(page) = &state.page
            && page.url == url
        {
            paint_engine_page(ui, page);
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
            paint_engine_page(ui, &page);
        } else {
            document_error(ui, url, "no rendered page is available for this tab");
        }
    }
}

impl eframe::App for FortrustApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.animation_phase = ctx.input(|input| input.time as f32 * 1.4);
        self.poll_loading(ctx);
        self.top_chrome(ctx);
        self.left_rail(ctx);
        if self.show_privacy_panel {
            self.privacy_panel(ctx);
        }
        self.page(ctx);
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

fn apply_theme(ctx: &Context) {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.inactive.bg_fill = Color32::from_rgba_unmultiplied(235, 241, 231, 160);
    visuals.widgets.hovered.bg_fill = glass_strong();
    visuals.widgets.active.bg_fill = glass_strong();
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = Color32::from_rgb(77, 129, 91);
    ctx.set_visuals(visuals);
}

fn nav_button(ui: &mut Ui, text: &str, tip: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        Button::new(RichText::new(text).color(INK).size(13.0))
            .fill(Color32::from_rgba_unmultiplied(232, 238, 228, 115))
            .stroke(Stroke::NONE)
            .corner_radius(14)
            .min_size(Vec2::new(25.0, 25.0)),
    )
    .on_hover_text(tip)
}

fn tool_chip(ui: &mut Ui, text: &str, tip: &str) {
    let _ = ui
        .add_sized(
            [27.0, 25.0],
            Button::new(RichText::new(text).color(INK).size(12.0))
                .fill(Color32::from_rgba_unmultiplied(232, 238, 228, 105))
                .stroke(Stroke::NONE)
                .corner_radius(13),
        )
        .on_hover_text(tip);
}

fn rail_button(ui: &mut Ui, label: &str, tip: &str) -> egui::Response {
    let response = ui
        .add_sized(
            [28.0, 28.0],
            Button::new(RichText::new(label).color(INK).strong().size(12.0))
                .fill(Color32::from_rgba_unmultiplied(236, 242, 232, 168))
                .stroke(Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(255, 255, 255, 90),
                ))
                .corner_radius(14),
        )
        .on_hover_text(tip);
    ui.add_space(6.0);
    response
}

fn speed_dial(ui: &mut Ui, center: Pos2, label: &str, color: Color32) -> egui::Response {
    let rect = Rect::from_center_size(center, Vec2::splat(50.0));
    let response = ui.allocate_rect(rect, Sense::click());
    let painter = ui.painter();
    let lift = if response.hovered() { -3.0 } else { 0.0 };
    let center = Pos2::new(center.x, center.y + lift);
    painter.circle_filled(
        center,
        24.0,
        Color32::from_rgba_unmultiplied(245, 249, 241, 205),
    );
    painter.circle_filled(center, 17.0, color);
    painter.text(
        center,
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(15.0),
        Color32::WHITE,
    );
    response
}

fn paint_forest_background(ui: &mut Ui, phase: f32) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    for index in 0..32 {
        let t = index as f32 / 31.0;
        let color = lerp_color(
            Color32::from_rgb(139, 166, 125),
            Color32::from_rgb(19, 39, 28),
            t,
        );
        let band = Rect::from_min_max(
            Pos2::new(rect.left(), rect.top() + rect.height() * t),
            Pos2::new(
                rect.right(),
                rect.top() + rect.height() * ((index + 1) as f32 / 31.0),
            ),
        );
        painter.rect_filled(band, 0.0, color);
    }

    painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(8, 18, 11, 62));

    for index in 0..16 {
        let x = rect.left() + rect.width() * (index as f32 / 15.0);
        let drift = (phase + index as f32 * 0.7).sin() * 4.0;
        let width = 18.0 + (index % 4) as f32 * 10.0;
        let trunk = Rect::from_min_max(
            Pos2::new(x + drift, rect.top() - 40.0),
            Pos2::new(x + drift + width, rect.bottom() * 0.78),
        );
        painter.rect_filled(
            trunk,
            10.0,
            Color32::from_rgba_unmultiplied(20, 37, 28, 125),
        );
    }

    for index in 0..18 {
        let x = rect.left() + rect.width() * ((index as f32 * 0.17) % 1.0);
        let y = rect.top() + rect.height() * (0.32 + ((index % 5) as f32 * 0.08));
        let radius = 78.0 + (index % 6) as f32 * 24.0;
        let green = if index % 2 == 0 {
            Color32::from_rgba_unmultiplied(109, 156, 65, 70)
        } else {
            Color32::from_rgba_unmultiplied(52, 111, 62, 78)
        };
        painter.circle_filled(Pos2::new(x, y), radius, green);
    }

    let stone = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.bottom() - 142.0),
        Vec2::new(rect.width() * 0.43, 102.0),
    );
    painter.rect_filled(
        stone,
        34.0,
        Color32::from_rgba_unmultiplied(64, 77, 70, 238),
    );
    painter.rect_filled(
        stone.shrink2(Vec2::new(8.0, 14.0)),
        30.0,
        Color32::from_rgba_unmultiplied(95, 109, 101, 190),
    );
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(rect.left(), rect.bottom() - 120.0),
            Pos2::new(rect.right(), rect.bottom()),
        ),
        0.0,
        Color32::from_rgba_unmultiplied(19, 30, 20, 155),
    );
    for index in 0..20 {
        let x = rect.left() + rect.width() * ((index as f32 * 0.071) % 1.0);
        let y = rect.bottom() - 74.0 + (index % 6) as f32 * 13.0;
        painter.circle_filled(
            Pos2::new(x, y),
            58.0 + (index % 5) as f32 * 10.0,
            Color32::from_rgba_unmultiplied(65, 103, 32, 140),
        );
    }
}

fn paint_document_background(ui: &mut Ui, phase: f32) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, BG);
    painter.circle_filled(
        Pos2::new(rect.left() + 140.0 + phase.sin() * 16.0, rect.top() + 80.0),
        210.0,
        Color32::from_rgba_unmultiplied(32, 74, 52, 48),
    );
    painter.circle_filled(
        Pos2::new(rect.right() - 180.0, rect.bottom() - 120.0),
        240.0,
        Color32::from_rgba_unmultiplied(82, 102, 56, 36),
    );
}

fn loading_page(ui: &mut Ui, url: &str, phase: f32) {
    let rect = ui.max_rect();
    let center = Pos2::new(rect.center().x, rect.top() + rect.height() * 0.42);
    let radius = 26.0 + (phase.sin() * 0.5 + 0.5) * 10.0;
    ui.painter().circle_filled(
        center,
        radius + 14.0,
        Color32::from_rgba_unmultiplied(120, 207, 150, 32),
    );
    ui.painter().circle_filled(center, radius, ACCENT);
    ui.painter().text(
        Pos2::new(center.x, center.y + 64.0),
        Align2::CENTER_CENTER,
        "Trust Engine is loading",
        FontId::proportional(28.0),
        TEXT,
    );
    ui.painter().text(
        Pos2::new(center.x, center.y + 94.0),
        Align2::CENTER_CENTER,
        url,
        FontId::proportional(13.0),
        MUTED,
    );
}

fn document_error(ui: &mut Ui, url: &str, error: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(72.0);
        ui.label(
            RichText::new("Trust Engine load failed")
                .size(28.0)
                .strong()
                .color(DANGER),
        );
        ui.add_space(8.0);
        ui.label(RichText::new(url).color(ACCENT));
        ui.add_space(8.0);
        ui.label(RichText::new(error).color(MUTED));
    });
}

fn paint_engine_page(ui: &mut Ui, page: &EnginePage) {
    let outer = ui.available_size();
    let (surface, _) = ui.allocate_exact_size(outer, Sense::hover());
    let painter = ui.painter_at(surface);
    painter.rect_filled(surface.shrink(18.0), 22.0, glass_dark());

    let content = surface.shrink2(Vec2::new(34.0, 32.0));
    painter.rect_filled(content, 14.0, Color32::WHITE);
    for command in page.rendered.display_list.commands() {
        match command {
            DisplayCommand::FillRect { rect, color } => {
                painter.rect_filled(to_egui_rect(content.min, *rect), 0.0, to_egui_color(*color));
            }
            DisplayCommand::DrawText {
                rect,
                text,
                color,
                font_size_px,
                ..
            } => {
                painter.text(
                    Pos2::new(content.min.x + rect.x, content.min.y + rect.y),
                    Align2::LEFT_TOP,
                    text,
                    FontId::proportional(*font_size_px),
                    to_egui_color(*color),
                );
            }
            DisplayCommand::ClipPush(_) | DisplayCommand::ClipPop => {}
        }
    }
}

fn to_egui_rect(origin: Pos2, rect: EngineRect) -> Rect {
    Rect::from_min_size(
        Pos2::new(origin.x + rect.x, origin.y + rect.y),
        Vec2::new(rect.width, rect.height),
    )
}

fn to_egui_color(color: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

fn blocked_page(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(96.0);
        ui.label(
            RichText::new("Request blocked")
                .size(30.0)
                .strong()
                .color(DANGER),
        );
        ui.add_space(10.0);
        ui.label(
            RichText::new("Fortrust stopped this navigation before it left the browser.")
                .color(TEXT),
        );
    });
}

fn metric(ui: &mut Ui, label: &str, value: impl Into<String>, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value.into()).color(color).strong());
        });
    });
    ui.add_space(8.0);
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
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("q", trimmed)
            .finish();
        format!("https://duckduckgo.com/?{query}")
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

fn summarize_decision(notes: &[PrivacyNote], stripped_query_pairs: usize) -> String {
    if notes.is_empty() {
        return "Allowed".to_owned();
    }

    let mut parts = Vec::new();
    if notes.contains(&PrivacyNote::HttpsUpgraded) {
        parts.push("HTTPS upgraded".to_owned());
    }
    if stripped_query_pairs > 0 {
        parts.push(format!("{stripped_query_pairs} tracker params stripped"));
    }
    if notes.contains(&PrivacyNote::ThirdPartyCookieBlocked) {
        parts.push("third-party cookies blocked".to_owned());
    }
    if notes.contains(&PrivacyNote::FingerprintNoiseEnabled) {
        parts.push("fingerprint noise on".to_owned());
    }

    if parts.is_empty() {
        "Allowed with privacy headers".to_owned()
    } else {
        parts.join(", ")
    }
}

fn source_label(source: PageSource) -> &'static str {
    match source {
        PageSource::Internal => "internal",
        PageSource::Offline => "offline",
        PageSource::Network => "network",
        PageSource::Cache => "cache",
        PageSource::RevalidatedCache => "cache revalidated",
    }
}

fn block_reason_label(reason: BlockReason) -> &'static str {
    match reason {
        BlockReason::InvalidUrl => "invalid URL",
        BlockReason::UnsupportedScheme => "unsupported scheme",
        BlockReason::TrackerDomain => "known tracker domain",
        BlockReason::AdDomain => "known ad domain",
        BlockReason::MixedContent => "mixed content",
    }
}

fn enabled_label(enabled: bool) -> &'static str {
    if enabled { "On" } else { "Off" }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let lerp = |left: u8, right: u8| left as f32 + (right as f32 - left as f32) * t;
    Color32::from_rgb(
        lerp(a.r(), b.r()) as u8,
        lerp(a.g(), b.g()) as u8,
        lerp(a.b(), b.b()) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_terms_are_form_encoded() {
        assert_eq!(
            normalize_input("privacy browser + rust"),
            "https://duckduckgo.com/?q=privacy+browser+%2B+rust"
        );
    }

    #[test]
    fn bare_hosts_become_https_urls() {
        assert_eq!(normalize_input("example.com"), "https://example.com");
    }

    #[test]
    fn localhost_becomes_https_url() {
        assert_eq!(normalize_input("localhost:3000"), "https://localhost:3000");
    }

    #[test]
    fn bracketed_ipv6_becomes_https_url() {
        assert_eq!(normalize_input("[::1]:8080"), "https://[::1]:8080");
    }

    #[test]
    fn tab_history_discards_forward_entries_on_new_navigation() {
        let mut state = TabPageState::new("fortrust://start");
        state.push_history("https://a.test".to_owned());
        state.push_history("https://b.test".to_owned());
        assert_eq!(state.go_back().as_deref(), Some("https://a.test"));
        state.push_history("https://c.test".to_owned());

        assert_eq!(state.current_url(), Some("https://c.test"));
        assert!(!state.can_go_forward());
    }
}
