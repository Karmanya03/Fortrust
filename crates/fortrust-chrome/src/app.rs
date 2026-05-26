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
use fortrust_storage::{Bookmark, HistoryEntry, StorageDatabase, SettingValue};
use trust_engine::{Color, DisplayCommand, EnginePage, EngineRect, TrustEngine, Viewport};

use crate::{
    animation::SidebarAnimation,
    omnibox::OmniboxState,
    shield::ShieldState,
    sidebar::{self, SidebarPage},
    speed_dial::SpeedDialState,
    backgrounds,
    theme::FortrustTheme,
};

pub struct FortrustApp {
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
    pub history_filter: String,
    pub bookmark_filter: String,
    // show a transient startup overlay for a short time
    pub startup_deadline: Option<std::time::Instant>,
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

const SETTINGS_UI_THEME: &str = "chrome.ui.theme";
const SETTINGS_UI_COMPACT: &str = "chrome.ui.compact_density";
const SETTINGS_UI_PRIVACY_PANEL: &str = "chrome.ui.show_privacy_panel";
const SETTINGS_UI_MEMORY_METER: &str = "chrome.ui.show_memory_meter";
const SETTINGS_UI_GLASS: &str = "chrome.ui.glass_strength";
const SETTINGS_UI_MOTION: &str = "chrome.ui.motion_strength";
const SETTINGS_UI_WALLPAPER: &str = "chrome.ui.wallpaper";
const SETTINGS_UI_WALLPAPER_STRENGTH: &str = "chrome.ui.wallpaper_strength";
const SETTINGS_PRIVACY_BLOCK_ADS: &str = "chrome.privacy.block_ads";
const SETTINGS_PRIVACY_BLOCK_TRACKERS: &str = "chrome.privacy.block_trackers";
const SETTINGS_PRIVACY_THIRD_PARTY: &str = "chrome.privacy.block_third_party_cookies";
const SETTINGS_PRIVACY_STRIP_PARAMS: &str = "chrome.privacy.strip_tracking_query_params";
const SETTINGS_PRIVACY_HTTPS_ONLY: &str = "chrome.privacy.https_only_mode";
const SETTINGS_PRIVACY_GPC: &str = "chrome.privacy.global_privacy_control";
const SETTINGS_PRIVACY_DNT: &str = "chrome.privacy.do_not_track";
const SETTINGS_PRIVACY_FINGERPRINT: &str = "chrome.privacy.fingerprint_noise";

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
        let storage = Self::open_storage();
        let config = Self::load_browser_config(storage.as_ref());
        let theme = Self::theme_for_mode(&config.ui.theme, config.ui.glass_strength);
        apply_egui_style(&creation_context.egui_ctx, &theme, &config.ui);

        let mut tabs = TabManager::new(config.performance.clone());
        let start_id = tabs.open_tab("fortrust://start", "Speed Dial", false);
        let mut tab_pages = HashMap::new();
        tab_pages.insert(start_id, TabPageState::new("fortrust://start"));

        Self {
            privacy: PrivacyEngine::new(config.privacy.clone()),
            internal_engine: TrustEngine::offline(),
            engine_worker: EngineWorker::spawn(config.privacy.clone()),
            storage,
            config: config.clone(),
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
            history_filter: String::new(),
            bookmark_filter: String::new(),
            startup_deadline: Some(std::time::Instant::now() + std::time::Duration::from_millis(1500)),
        }
    }

    fn load_browser_config(storage: Option<&StorageDatabase>) -> BrowserConfig {
        let mut config = BrowserConfig::default();
        let Some(storage) = storage else {
            return config;
        };

        if let Some(theme) = storage
            .settings
            .load(SETTINGS_UI_THEME)
            .and_then(|value| value.as_string().map(str::to_owned))
        {
            config.ui.theme = theme;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_UI_COMPACT)
            .and_then(|value| value.as_bool())
        {
            config.ui.compact_density = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_UI_WALLPAPER)
            .and_then(|value| value.as_string().map(str::to_owned))
        {
            config.ui.wallpaper = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_UI_WALLPAPER_STRENGTH)
            .and_then(|value| value.as_int().map(|v| v as u8))
        {
            config.ui.wallpaper_strength = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_UI_PRIVACY_PANEL)
            .and_then(|value| value.as_bool())
        {
            config.ui.show_privacy_panel = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_UI_MEMORY_METER)
            .and_then(|value| value.as_bool())
        {
            config.ui.show_memory_meter = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_UI_GLASS)
            .and_then(|value| value.as_int())
        {
            config.ui.glass_strength = value.clamp(0, 100) as u8;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_UI_MOTION)
            .and_then(|value| value.as_int())
        {
            config.ui.motion_strength = value.clamp(0, 100) as u8;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_PRIVACY_BLOCK_ADS)
            .and_then(|value| value.as_bool())
        {
            config.privacy.block_ads = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_PRIVACY_BLOCK_TRACKERS)
            .and_then(|value| value.as_bool())
        {
            config.privacy.block_trackers = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_PRIVACY_THIRD_PARTY)
            .and_then(|value| value.as_bool())
        {
            config.privacy.block_third_party_cookies = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_PRIVACY_STRIP_PARAMS)
            .and_then(|value| value.as_bool())
        {
            config.privacy.strip_tracking_query_params = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_PRIVACY_HTTPS_ONLY)
            .and_then(|value| value.as_bool())
        {
            config.privacy.https_only_mode = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_PRIVACY_GPC)
            .and_then(|value| value.as_bool())
        {
            config.privacy.global_privacy_control = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_PRIVACY_DNT)
            .and_then(|value| value.as_bool())
        {
            config.privacy.do_not_track = value;
        }
        if let Some(value) = storage
            .settings
            .load(SETTINGS_PRIVACY_FINGERPRINT)
            .and_then(|value| value.as_bool())
        {
            config.privacy.fingerprint_noise = value;
        }

        config
    }

    fn persist_browser_config(&self) {
        let Some(storage) = &self.storage else {
            return;
        };

        let _ = storage
            .settings
            .store(SETTINGS_UI_THEME, &SettingValue::from(self.config.ui.theme.clone()));
        let _ = storage.settings.store(
            SETTINGS_UI_COMPACT,
            &SettingValue::from(self.config.ui.compact_density),
        );
        let _ = storage.settings.store(
            SETTINGS_UI_PRIVACY_PANEL,
            &SettingValue::from(self.config.ui.show_privacy_panel),
        );
        let _ = storage.settings.store(
            SETTINGS_UI_MEMORY_METER,
            &SettingValue::from(self.config.ui.show_memory_meter),
        );
        let _ = storage.settings.store(
            SETTINGS_UI_GLASS,
            &SettingValue::from(i64::from(self.config.ui.glass_strength)),
        );
        let _ = storage
            .settings
            .store(SETTINGS_UI_WALLPAPER, &SettingValue::from(self.config.ui.wallpaper.clone()));
        let _ = storage.settings.store(
            SETTINGS_UI_WALLPAPER_STRENGTH,
            &SettingValue::from(i64::from(self.config.ui.wallpaper_strength)),
        );
        let _ = storage.settings.store(
            SETTINGS_UI_MOTION,
            &SettingValue::from(i64::from(self.config.ui.motion_strength)),
        );
        let _ = storage
            .settings
            .store(SETTINGS_PRIVACY_BLOCK_ADS, &SettingValue::from(self.config.privacy.block_ads));
        let _ = storage.settings.store(
            SETTINGS_PRIVACY_BLOCK_TRACKERS,
            &SettingValue::from(self.config.privacy.block_trackers),
        );
        let _ = storage.settings.store(
            SETTINGS_PRIVACY_THIRD_PARTY,
            &SettingValue::from(self.config.privacy.block_third_party_cookies),
        );
        let _ = storage.settings.store(
            SETTINGS_PRIVACY_STRIP_PARAMS,
            &SettingValue::from(self.config.privacy.strip_tracking_query_params),
        );
        let _ = storage.settings.store(
            SETTINGS_PRIVACY_HTTPS_ONLY,
            &SettingValue::from(self.config.privacy.https_only_mode),
        );
        let _ = storage.settings.store(
            SETTINGS_PRIVACY_GPC,
            &SettingValue::from(self.config.privacy.global_privacy_control),
        );
        let _ = storage
            .settings
            .store(SETTINGS_PRIVACY_DNT, &SettingValue::from(self.config.privacy.do_not_track));
        let _ = storage.settings.store(
            SETTINGS_PRIVACY_FINGERPRINT,
            &SettingValue::from(self.config.privacy.fingerprint_noise),
        );
    }

    fn theme_for_mode(theme: &str, glass_strength: u8) -> FortrustTheme {
        match theme.to_ascii_lowercase().as_str() {
            "light" => FortrustTheme::light_with_glass_strength(glass_strength),
            "system" => FortrustTheme::dark_with_glass_strength(glass_strength),
            _ => FortrustTheme::dark_with_glass_strength(glass_strength),
        }
    }

    fn refresh_theme(&mut self, ctx: &Context) {
        self.theme = Self::theme_for_mode(&self.config.ui.theme, self.config.ui.glass_strength);
        apply_egui_style(ctx, &self.theme, &self.config.ui);
    }

    fn motion_scale(&self) -> f32 {
        0.65 + (self.config.ui.motion_strength.min(100) as f32 / 100.0) * 0.85
    }

    fn rebuild_privacy_pipeline(&mut self) {
        self.privacy = PrivacyEngine::new(self.config.privacy.clone());
        self.engine_worker = EngineWorker::spawn(self.config.privacy.clone());
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
                    let memory_estimate = (page.security.body_bytes as f32 / 1024.0 / 1024.0)
                        .max(0.25)
                        + (page.security.display_commands as f32 * 0.01);
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
                        self.total_memory_mb = memory_estimate;
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
        let glass_frame = Frame {
            fill: self.theme.glass_bg,
            inner_margin: Margin::symmetric(12, 8),
            outer_margin: Margin::symmetric(6, 0),
            corner_radius: CornerRadius::same(16),
            stroke: Stroke::new(1.0, self.theme.glass_border),
            shadow: egui::epaint::Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(40),
            },
            ..Default::default()
        };

        egui::TopBottomPanel::top("fortrust_toolbar")
            .exact_height(50.0)
            .frame(glass_frame)
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

                    if self.config.ui.show_memory_meter {
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new(format!("🧠 {:.1} MB", self.total_memory_mb))
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                    }
                });
            });
    }

    fn render_tab_bar(&mut self, ctx: &Context) {
        let glass_frame = Frame {
            fill: self.theme.glass_bg,
            inner_margin: Margin::symmetric(8, 4),
            outer_margin: Margin::symmetric(6, 6),
            corner_radius: CornerRadius::same(12),
            stroke: Stroke::new(1.0, self.theme.glass_border),
            shadow: egui::epaint::Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(40),
            },
            ..Default::default()
        };

        egui::TopBottomPanel::top("fortrust_tab_bar")
            .exact_height(40.0)
            .frame(glass_frame)
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
        let selected_page = self.sidebar_page;
        let selected_page_str = match selected_page {
            SidebarPage::Tabs => "Tabs",
            SidebarPage::Bookmarks => "Bookmarks",
            SidebarPage::History => "History",
            SidebarPage::Downloads => "Downloads",
            SidebarPage::Notes => "Notes",
            SidebarPage::Settings => "Settings",
        };
        let active_url_hint = self
            .active_state()
            .and_then(TabPageState::current_url)
            .map(str::to_owned)
            .or_else(|| self.tabs.active_tab().map(|tab| tab.url.to_string()))
            .unwrap_or_else(|| "fortrust://start".to_owned());
        println!("Fortrust:render_central_panel selected_page={} active_url={}", selected_page_str, active_url_hint);

        let glass_frame = Frame {
            fill: self.theme.glass_bg,
            inner_margin: Margin::symmetric(0, 0),
            outer_margin: Margin::symmetric(6, 6),
            corner_radius: CornerRadius::same(16),
            stroke: Stroke::new(1.0, self.theme.glass_border),
            shadow: egui::epaint::Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(40),
            },
            ..Default::default()
        };

        egui::CentralPanel::default()
            .frame(glass_frame)
            .show(ctx, |ui| {
                    // DEBUG: geometry + visual confirmation
                    let central_rect = ui.max_rect();
                    println!("Fortrust:central_rect={:?}", central_rect);
                    ui.painter().rect_stroke(
                        central_rect,
                        CornerRadius::same(12),
                        egui::Stroke::new(2.0, Color32::from_rgb(200, 40, 40)),
                        egui::StrokeKind::Outside,
                    );
                    ui.painter().text(
                        central_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "CENTRAL PANEL ACTIVE",
                        egui::FontId::proportional(28.0),
                        Color32::from_rgb(255, 0, 0),
                    );
                if selected_page == SidebarPage::Tabs {
                    let active_url = self
                        .active_state()
                        .and_then(TabPageState::current_url)
                        .map(str::to_owned)
                        .or_else(|| self.tabs.active_tab().map(|tab| tab.url.to_string()))
                        .unwrap_or_else(|| "fortrust://start".to_owned());

                    let dt = self.last_frame.elapsed().as_secs_f32().min(0.05) * self.motion_scale();

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
                } else {
                    self.render_sidebar_page(ui, selected_page);
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

    fn render_sidebar_page(&mut self, ui: &mut egui::Ui, page: SidebarPage) {
        let theme = self.theme;
        let title = match page {
            SidebarPage::Tabs => "Tabs",
            SidebarPage::Bookmarks => "Bookmarks",
            SidebarPage::History => "History",
            SidebarPage::Downloads => "Downloads",
            SidebarPage::Notes => "Notes",
            SidebarPage::Settings => "Settings",
        };
        let subtitle = match page {
            SidebarPage::Tabs => "",
            SidebarPage::Bookmarks => "Saved destinations and quick actions.",
            SidebarPage::History => "Recent visits captured by the storage layer.",
            SidebarPage::Downloads => "Download manager surface and future queue controls.",
            SidebarPage::Notes => "Local scratchpad space for pinned notes.",
            SidebarPage::Settings => "Visual, motion, and privacy controls for Fortrust.",
        };

        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(26.0)
                    .strong()
                    .color(theme.text_primary),
            );
            if !subtitle.is_empty() {
                ui.label(
                    egui::RichText::new(subtitle)
                        .size(13.0)
                        .color(theme.text_secondary),
                );
            }
        });
        ui.add_space(16.0);

        match page {
            SidebarPage::Bookmarks => self.render_bookmarks_page(ui),
            SidebarPage::History => self.render_history_page(ui),
            SidebarPage::Downloads => self.render_downloads_page(ui),
            SidebarPage::Notes => self.render_notes_page(ui),
            SidebarPage::Settings => self.render_settings_page(ui),
            SidebarPage::Tabs => {}
        }
    }

    fn render_bookmarks_page(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme;
        let bookmarks = self
            .storage
            .as_ref()
            .map(|storage| storage.bookmarks.all())
            .unwrap_or_default();
        let query = self.bookmark_filter.to_ascii_lowercase();
        let mut add_current = false;
        let mut open_target: Option<String> = None;
        let mut delete_target: Option<String> = None;

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Search").color(theme.text_secondary));
            ui.add_sized(
                [320.0, 30.0],
                egui::TextEdit::singleline(&mut self.bookmark_filter)
                    .hint_text("Filter bookmarks")
                    .frame(false),
            );
            if ui.button("Bookmark current page").clicked() {
                add_current = true;
            }
        });

        ui.add_space(12.0);

        let mut any_visible = false;
        for bookmark in bookmarks {
            if !query.is_empty()
                && !bookmark.url.to_ascii_lowercase().contains(&query)
                && !bookmark.title.to_ascii_lowercase().contains(&query)
            {
                continue;
            }
            any_visible = true;
            render_metric_card(ui, theme, &bookmark.title, &bookmark.url, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        open_target = Some(bookmark.url.clone());
                    }
                    if ui.button("Delete").clicked() {
                        delete_target = Some(bookmark.url.clone());
                    }
                });
            });
            ui.add_space(8.0);
        }

        if !any_visible {
            render_empty_state(
                ui,
                theme,
                "No bookmarks yet",
                "Use the action above to pin the current page or add a quick launch tile.",
            );
        }

        if add_current {
            self.bookmark_current_page();
        }
        if let Some(url) = open_target {
            self.navigate_input(url, HistoryMode::Push);
        }
        if let Some(url) = delete_target
            && let Some(storage) = self.storage.as_ref()
        {
            let _ = storage.bookmarks.delete(&url);
        }
    }

    fn render_history_page(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme;
        let history = self
            .storage
            .as_ref()
            .and_then(|storage| storage.history.recently_visited(100).ok())
            .unwrap_or_default();
        let query = self.history_filter.to_ascii_lowercase();
        let mut open_target: Option<String> = None;
        let mut bookmark_target: Option<HistoryEntry> = None;
        let mut delete_target: Option<String> = None;
        let mut clear_history = false;

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Search").color(theme.text_secondary));
            ui.add_sized(
                [320.0, 30.0],
                egui::TextEdit::singleline(&mut self.history_filter)
                    .hint_text("Filter history")
                    .frame(false),
            );
            if ui.button("Clear history").clicked() {
                clear_history = true;
            }
        });

        ui.add_space(12.0);

        let mut any_visible = false;
        for entry in history {
            if !query.is_empty()
                && !entry.url.to_ascii_lowercase().contains(&query)
                && !entry.title.to_ascii_lowercase().contains(&query)
            {
                continue;
            }
            any_visible = true;
            render_metric_card(ui, theme, &entry.title, &entry.url, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        open_target = Some(entry.url.clone());
                    }
                    if ui.button("Bookmark").clicked() {
                        bookmark_target = Some(entry.clone());
                    }
                    if ui.button("Remove").clicked() {
                        delete_target = Some(entry.url.clone());
                    }
                });
                ui.add_space(6.0);
                metric_row(ui, theme, "Visits", &entry.visit_count.to_string());
                metric_row(ui, theme, "Last seen", &entry.visit_time.to_rfc2822());
            });
            ui.add_space(8.0);
        }

        if !any_visible {
            render_empty_state(
                ui,
                theme,
                "No history found",
                "Visited pages will appear here once they finish loading.",
            );
        }

        if clear_history
            && let Some(storage) = self.storage.as_ref()
        {
            let _ = storage.history.clear();
        }
        if let Some(url) = open_target {
            self.navigate_input(url, HistoryMode::Push);
        }
        if let Some(entry) = bookmark_target {
            self.save_bookmark(entry.url, entry.title);
        }
        if let Some(url) = delete_target
            && let Some(storage) = self.storage.as_ref()
        {
            let _ = storage.history.delete_entry(&url);
        }
    }

    fn render_downloads_page(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme;
        let stats = self.storage.as_ref().map(|storage| storage.stats());

        render_metric_card(
            ui,
            theme,
            "Storage",
            "Backed by redb with an in-memory fallback",
            |ui| {
                let history_count = stats.as_ref().map(|s| s.history_count).unwrap_or(0);
                let bookmark_count = stats.as_ref().map(|s| s.bookmark_count).unwrap_or(0);
                let cookie_count = stats.as_ref().map(|s| s.cookie_count).unwrap_or(0);
                metric_row(ui, theme, "History entries", &history_count.to_string());
                metric_row(ui, theme, "Bookmarks", &bookmark_count.to_string());
                metric_row(ui, theme, "Cookies", &cookie_count.to_string());
            },
        );

        ui.add_space(12.0);
        render_empty_state(
            ui,
            theme,
            "Download manager not wired yet",
            "The page is active, but queueing and transfer tracking still need a dedicated backend.",
        );
    }

    fn render_notes_page(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme;
        render_empty_state(
            ui,
            theme,
            "Notes surface is reserved",
            "This page is ready for local scratch notes, but the editing backend is not wired yet.",
        );
    }

    fn render_settings_page(&mut self, ui: &mut egui::Ui) {
        let theme = self.theme;
        let mut theme_changed = false;
        let mut visual_changed = false;
        let mut privacy_changed = false;
        let mut reset_requested = false;
        let active_security = self
            .active_state()
            .and_then(|state| state.page.as_ref())
            .map(|page| page.security.clone());
        let storage_stats = self.storage.as_ref().map(|storage| storage.stats());

        render_metric_card(ui, theme, "Appearance", "Glass surfaces and motion controls", |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Theme").color(theme.text_secondary));
                theme_changed |= ui
                    .selectable_value(&mut self.config.ui.theme, "dark".to_owned(), "Dark")
                    .changed();
                theme_changed |= ui
                    .selectable_value(&mut self.config.ui.theme, "light".to_owned(), "Light")
                    .changed();
                theme_changed |= ui
                    .selectable_value(&mut self.config.ui.theme, "system".to_owned(), "System")
                    .changed();
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Wallpaper").color(theme.text_secondary));
                ui.selectable_value(&mut self.config.ui.wallpaper, "watercolor".to_owned(), "Watercolor");
                ui.selectable_value(&mut self.config.ui.wallpaper, "forest".to_owned(), "Forest");
                ui.selectable_value(&mut self.config.ui.wallpaper, "none".to_owned(), "None");
            });

            visual_changed |= ui
                .add(
                    egui::Slider::new(&mut self.config.ui.wallpaper_strength, 0..=100)
                        .text("Wallpaper strength")
                        .show_value(true),
                )
                .changed();

            visual_changed |= ui
                .add(
                    egui::Slider::new(&mut self.config.ui.glass_strength, 30..=100)
                        .text("Glass strength")
                        .show_value(true),
                )
                .changed();
            visual_changed |= ui
                .add(
                    egui::Slider::new(&mut self.config.ui.motion_strength, 20..=100)
                        .text("Motion strength")
                        .show_value(true),
                )
                .changed();

            visual_changed |= ui
                .checkbox(&mut self.config.ui.compact_density, "Compact density")
                .changed();
            visual_changed |= ui
                .checkbox(&mut self.config.ui.show_privacy_panel, "Show privacy panel")
                .changed();
            visual_changed |= ui
                .checkbox(&mut self.config.ui.show_memory_meter, "Show memory meter")
                .changed();
        });

        ui.add_space(12.0);

        render_metric_card(ui, theme, "Privacy", "Request filtering that drives the secure navigation pipeline", |ui| {
            privacy_changed |= ui
                .checkbox(&mut self.config.privacy.block_ads, "Block ads")
                .changed();
            privacy_changed |= ui
                .checkbox(&mut self.config.privacy.block_trackers, "Block trackers")
                .changed();
            privacy_changed |= ui
                .checkbox(
                    &mut self.config.privacy.block_third_party_cookies,
                    "Block third-party cookies",
                )
                .changed();
            privacy_changed |= ui
                .checkbox(
                    &mut self.config.privacy.strip_tracking_query_params,
                    "Strip tracking query params",
                )
                .changed();
            privacy_changed |= ui
                .checkbox(&mut self.config.privacy.https_only_mode, "Upgrade HTTP to HTTPS")
                .changed();
            privacy_changed |= ui
                .checkbox(
                    &mut self.config.privacy.global_privacy_control,
                    "Send Global Privacy Control",
                )
                .changed();
            privacy_changed |= ui
                .checkbox(&mut self.config.privacy.do_not_track, "Send Do Not Track")
                .changed();
            privacy_changed |= ui
                .checkbox(&mut self.config.privacy.fingerprint_noise, "Fingerprint noise")
                .changed();
        });

        ui.add_space(12.0);

        render_metric_card(ui, theme, "Diagnostics", "Current storage and engine state", |ui| {
            if let Some(stats) = storage_stats.as_ref() {
                metric_row(ui, theme, "History count", &stats.history_count.to_string());
                metric_row(ui, theme, "Bookmark count", &stats.bookmark_count.to_string());
                metric_row(ui, theme, "Cookie count", &stats.cookie_count.to_string());
            } else {
                metric_row(ui, theme, "Storage", "Fallback mode");
            }

            if let Some(security) = active_security.as_ref() {
                metric_row(
                    ui,
                    theme,
                    "JS enabled",
                    if security.javascript_enabled { "Yes" } else { "No" },
                );
                metric_row(
                    ui,
                    theme,
                    "Privacy pipeline",
                    if security.privacy_pipeline_enforced { "On" } else { "Off" },
                );
                metric_row(
                    ui,
                    theme,
                    "Subresources",
                    if security.external_subresources_enabled {
                        "Loaded"
                    } else {
                        "Blocked"
                    },
                );
                metric_row(ui, theme, "Display commands", &security.display_commands.to_string());
            }
        });

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if ui.button("Reset to defaults").clicked() {
                reset_requested = true;
            }
            if ui.button("Save settings").clicked() {
                theme_changed = true;
                visual_changed = true;
                privacy_changed = true;
            }
        });

        if reset_requested {
            self.config = BrowserConfig::default();
            self.rebuild_privacy_pipeline();
            self.refresh_theme(ui.ctx());
            self.persist_browser_config();
            return;
        }

        if theme_changed || visual_changed {
            self.refresh_theme(ui.ctx());
            self.persist_browser_config();
        }

        if privacy_changed {
            self.rebuild_privacy_pipeline();
            self.persist_browser_config();
        }
    }

    fn bookmark_current_page(&mut self) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        let Some(tab) = self.tabs.active_tab() else {
            return;
        };
        let current_url = tab.url.to_string();
        if current_url.starts_with("fortrust://") || current_url.starts_with("about:") {
            return;
        }

        let title = self
            .active_state()
            .and_then(|state| state.page.as_ref().map(|page| page.title.clone()))
            .unwrap_or_else(|| tab.title.to_string());

        let bookmark = Bookmark {
            id: current_url.clone(),
            url: current_url,
            title,
            folder_id: None,
            added_at: Utc::now(),
            last_visited: None,
            visit_count: 1,
            icon_data: None,
            description: None,
        };
        let _ = storage.bookmarks.store(&bookmark);
    }

    fn save_bookmark(&mut self, url: String, title: String) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        let bookmark = Bookmark {
            id: url.clone(),
            url,
            title,
            folder_id: None,
            added_at: Utc::now(),
            last_visited: None,
            visit_count: 1,
            icon_data: None,
            description: None,
        };
        let _ = storage.bookmarks.store(&bookmark);
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

        let content = surface.shrink2(Vec2::new(12.0, 12.0));
        painter.rect_filled(content, CornerRadius::same(8), Color32::WHITE);
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
        let motion_scale = self.motion_scale();
        self.animation_phase = ctx.input(|i| i.time as f32 * (1.15 + motion_scale * 0.45));

        let sidebar_page_str = match self.sidebar_page {
            SidebarPage::Tabs => "Tabs",
            SidebarPage::Bookmarks => "Bookmarks",
            SidebarPage::History => "History",
            SidebarPage::Downloads => "Downloads",
            SidebarPage::Notes => "Notes",
            SidebarPage::Settings => "Settings",
        };

        println!(
            "Fortrust:update page={} tabs={} active_id={:?} needs_new_tab={} glass={}",
            sidebar_page_str,
            self.tabs.tabs().len(),
            self.tabs.active_id(),
            self.needs_new_tab,
            self.config.ui.glass_strength,
        );

        // Tick animations
        self.sidebar_anim.tick(dt * motion_scale);
        if !self.sidebar_anim.width.is_settled() {
            ctx.request_repaint();
        }

        // Poll loading engine
        self.poll_loading(ctx);

        // Clear startup overlay after deadline
        if let Some(dl) = self.startup_deadline {
            if std::time::Instant::now() >= dl {
                self.startup_deadline = None;
            } else {
                // keep repainting until deadline passes
                ctx.request_repaint_after(dl - std::time::Instant::now());
            }
        }

        // Apply egui style
        apply_egui_style(ctx, &self.theme, &self.config.ui);

        // Render shield popup first (floats above everything)
        if self.config.ui.show_privacy_panel {
            self.shield.render_popup(ctx, &self.theme);
        }

        // Background: draw textured wallpaper via backgrounds module
        let screen_rect = ctx.content_rect();
        backgrounds::paint_background(ctx, screen_rect, &self.theme, &self.config.ui);

        // Render sidebar
        let tabs = self.tabs.tabs().to_vec();
        let mut active_idx = 0;
        if let Some(active_id) = self.tabs.active_id() {
            active_idx = tabs.iter().position(|t| t.id == active_id).unwrap_or(0);
        }

        let sidebar_frame = Frame {
            fill: self.theme.glass_bg,
            inner_margin: Margin::symmetric(8, 8),
            outer_margin: Margin::symmetric(6, 6),
            corner_radius: CornerRadius::same(16),
            stroke: Stroke::new(1.0, self.theme.glass_border),
            shadow: egui::epaint::Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(40),
            },
            ..Default::default()
        };

        egui::SidePanel::left("sidebar")
            .exact_width(self.sidebar_anim.current_width() + 16.0) // padding adjustment
            .resizable(false)
            .frame(sidebar_frame)
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
        let frame_delay = (33.0 / motion_scale.max(0.5)).round().clamp(16.0, 40.0) as u64;
        ctx.request_repaint_after(Duration::from_millis(frame_delay));

        if self.startup_deadline.is_some() {
            egui::Area::new("startup_overlay".into())
                .order(egui::Order::Background)
                .fixed_pos(egui::pos2(24.0, 24.0))
                .interactable(false)
                .show(ctx, |ui| {
                    Frame {
                        fill: self.theme.glass_bg,
                        inner_margin: Margin::symmetric(18, 14),
                        outer_margin: Margin::ZERO,
                        corner_radius: CornerRadius::same(16),
                        stroke: Stroke::new(1.0, self.theme.glass_border),
                        shadow: egui::epaint::Shadow {
                            offset: [0, 8],
                            blur: 24,
                            spread: 0,
                            color: Color32::from_black_alpha(50),
                        },
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("Fortrust starting")
                                .size(18.0)
                                .strong()
                                .color(self.theme.text_primary),
                        );
                        ui.label(
                            egui::RichText::new("Private browser shell and Trust Engine are loading the home surface.")
                                .size(12.0)
                                .color(self.theme.text_secondary),
                        );
                    });
                });
        }
    }
}

fn apply_egui_style(ctx: &egui::Context, theme: &FortrustTheme, ui_config: &fortrust_core::UiConfig) {
    let mut style = (*ctx.style()).clone();
    style.visuals.panel_fill = Color32::TRANSPARENT; // Background drawn manually
    style.visuals.window_fill = theme.glass_bg;
    style.visuals.window_stroke = egui::Stroke::new(1.0, theme.glass_border);
    let density = if ui_config.compact_density { 0.88 } else { 1.08 };
    style.spacing.item_spacing = Vec2::new(8.0 * density, 4.0 * density);
    style.spacing.button_padding = Vec2::new(8.0 * density, 5.0 * density);
    style.spacing.interact_size = Vec2::new(40.0 * density, 24.0 * density);
    ctx.set_style(style);
}

fn render_metric_card<R>(
    ui: &mut egui::Ui,
    theme: FortrustTheme,
    title: &str,
    subtitle: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    Frame {
        fill: theme.glass_bg,
        inner_margin: Margin::symmetric(16, 14),
        outer_margin: Margin::ZERO,
        corner_radius: CornerRadius::same(18),
        stroke: Stroke::new(1.0, theme.glass_border),
        shadow: egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(26),
        },
        ..Default::default()
    }
    .show(ui, |ui| {
        ui.label(
            egui::RichText::new(title)
                .size(15.0)
                .strong()
                .color(theme.text_primary),
        );
        ui.label(
            egui::RichText::new(subtitle)
                .size(12.0)
                .color(theme.text_secondary),
        );
        ui.add_space(12.0);
        add_contents(ui)
    })
    .inner
}

fn metric_row(ui: &mut egui::Ui, theme: FortrustTheme, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(12.0)
                .color(theme.text_secondary),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(value)
                    .size(12.0)
                    .strong()
                    .color(theme.text_primary),
            );
        });
    });
}

fn render_empty_state(ui: &mut egui::Ui, theme: FortrustTheme, title: &str, message: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.label(
            egui::RichText::new(title)
                .size(20.0)
                .strong()
                .color(theme.text_primary),
        );
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(message)
                .size(13.0)
                .color(theme.text_secondary),
        );
    });
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
