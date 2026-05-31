use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::process::{Command, Stdio};
use std::thread;
use std::io::ErrorKind;
use std::time::Duration;

use chrono::Utc;
use eframe::egui::{self, Color32, Context, CornerRadius, Frame, Margin, Pos2, Rect, Stroke, Vec2};
use fortrust_core::{
    BlockReason, BrowserConfig, PrivacyConfig, PrivacyEngine, RequestContext, ResourceType, TabId,
    TabManager, WorkspaceManager,
};
use fortrust_storage::{HistoryEntry, StorageDatabase, SettingValue};
use trust_engine::{Color, DisplayCommand, EnginePage, EngineRect, TrustEngine, Viewport};

use crate::{
    animation::SidebarAnimation,
    download::{DownloadManager, default_download_dir},
    icons,
    omnibox::{OmniboxState, SuggestionItem, SuggestionKind},
    shield::ShieldState,
    sidebar::{DownloadAction, SidebarState},
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

    // v2 UI state
    pub omnibox: OmniboxState,
    pub sidebar_anim: SidebarAnimation,
    pub sidebar_state: SidebarState,
    pub speed_dial: SpeedDialState,
    pub shield: ShieldState,
    pub theme: FortrustTheme,

    // Performance
    pub total_memory_mb: f32,
    pub last_frame: std::time::Instant,
    pub animation_phase: f32,
    pub needs_new_tab: bool,
    pub history_filter: String,
    pub bookmark_filter: String,
    pub startup_deadline: Option<std::time::Instant>,
    pub window_hovered_close: bool,
    pub window_hovered_min: bool,
    pub window_hovered_max: bool,

    // Tab drag-and-drop
    drag_tab_id: Option<TabId>,
    drag_cursor_x: f32,

    // Context menu state
    context_menu: Option<(egui::Pos2, TabId)>,

    // Page info panel
    page_info_open: bool,

    // Download manager
    download_manager: DownloadManager,
    download_dir: String,

    // Workspace management
    workspaces: WorkspaceManager,
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

#[derive(Default)]
struct TabPageState {
    page: Option<EnginePage>,
    load_error: Option<String>,
    loading_url: Option<String>,
    request_id: Option<u64>,
    history: Vec<String>,
    history_index: usize,
    renderer_frame: Option<egui::TextureHandle>,
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
        if self.current_url() == Some(url.as_str()) { return; }
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

    fn can_go_back(&self) -> bool { self.history_index > 0 }
    fn can_go_forward(&self) -> bool { self.history_index + 1 < self.history.len() }

    fn go_back(&mut self) -> Option<String> {
        if !self.can_go_back() { return None; }
        self.history_index -= 1;
        self.current_url().map(str::to_owned)
    }

    fn go_forward(&mut self) -> Option<String> {
        if !self.can_go_forward() { return None; }
        self.history_index += 1;
        self.current_url().map(str::to_owned)
    }

    fn begin_load(&mut self, url: String, request_id: u64) {
        self.page = None;
        self.load_error = None;
        self.loading_url = Some(url);
        self.request_id = Some(request_id);
        self.renderer_frame = None;
    }

    fn finish_load(&mut self, request_id: u64, page: EnginePage) -> bool {
        if self.request_id != Some(request_id) { return false; }
        self.replace_history(page.url.clone());
        self.page = Some(page);
        self.load_error = None;
        self.loading_url = None;
        self.request_id = None;
        self.renderer_frame = None;
        true
    }

    fn fail_load(&mut self, request_id: u64, error: String) -> bool {
        if self.request_id != Some(request_id) { return false; }
        self.page = None;
        self.load_error = Some(error);
        self.loading_url = None;
        self.request_id = None;
        self.renderer_frame = None;
        true
    }

    fn clear_document(&mut self) {
        self.page = None;
        self.load_error = None;
        self.loading_url = None;
        self.request_id = None;
        self.renderer_frame = None;
    }
}

struct EngineWorker {
    sender: Sender<EngineCommand>,
    receiver: Receiver<EngineEvent>,
    next_request_id: u64,
    ipc_sender: Option<fortrust_ipc::MessageSender>,
}

enum EngineCommand {
    Load { request_id: u64, url: String, viewport: Viewport },
}

enum EngineEvent {
    Loaded { request_id: u64, page: Box<EnginePage> },
    Failed { request_id: u64, url: String, error: String },
    RendererMsg { msg: fortrust_ipc::RendererToBrowser },
}

impl EngineWorker {
    fn spawn(privacy: PrivacyConfig) -> Self {
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();

        let ipc_sender = if let Some((sender, receiver)) = try_connect_external_renderer() {
            spawn_renderer_event_forwarder(receiver, event_sender.clone());
            sender
        } else {
            let (ipc_a, ipc_b) = fortrust_ipc::create_ipc_pair();
            spawn_basic_renderer_loop(ipc_b.receiver().clone(), ipc_b.sender().clone());
            ipc_a.sender().clone()
        };

        thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all().build()
            {
                Ok(r) => r,
                Err(e) => { drain_failed_worker(command_receiver, event_sender, e.to_string()); return; }
            };

            let mut engine = match TrustEngine::secure_networked(privacy) {
                Ok(e) => e,
                Err(e) => { drain_failed_worker(command_receiver, event_sender, format!("{e:?}")); return; }
            };

            while let Ok(command) = command_receiver.recv() {
                match command {
                    EngineCommand::Load { request_id, url, viewport } => {
                        let result = runtime.block_on(engine.load_url(url.clone(), viewport));
                        let event = match result {
                            Ok(page) => EngineEvent::Loaded { request_id, page: Box::new(page) },
                            Err(e) => EngineEvent::Failed { request_id, url, error: format!("{e:?}") },
                        };
                        let _ = event_sender.send(event);
                    }
                }
            }
        });

        Self { sender: command_sender, receiver: event_receiver, next_request_id: 1, ipc_sender: Some(ipc_sender) }
    }

    fn load(&mut self, url: String, viewport: Viewport) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        let url_clone = url.clone();
        let _ = self.sender.send(EngineCommand::Load { request_id, url: url.clone(), viewport });
        // Also send a navigation command to the renderer over IPC if available.
        if let Some(sender) = &self.ipc_sender {
            let sender = sender.clone();
            let _ = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build();
                if let Ok(rt) = rt {
                    rt.block_on(async move {
                        let _ = sender.send_browser_message(&fortrust_ipc::BrowserToRenderer::Navigate { url: url_clone }).await;
                    });
                }
            });
        }
        request_id
    }
}

fn try_connect_external_renderer() -> Option<(fortrust_ipc::MessageSender, fortrust_ipc::MessageReceiver)> {
    // Try several times to spawn/connect to an external renderer process.
    let renderer_path = find_renderer_binary()?;

    for attempt in 0..3 {
        let listener = match std::net::TcpListener::bind(("127.0.0.1", 0)) {
            Ok(l) => l,
            Err(_) => return None,
        };
        if listener.set_nonblocking(true).is_err() { return None; }
        let addr = match listener.local_addr() { Ok(a) => a, Err(_) => return None };

        let addr_arg = format!("{}:{}", addr.ip(), addr.port());

        tracing::info!(target: "fortrust.renderer", "spawning renderer attempt {} -> {}", attempt + 1, renderer_path.display());
        let mut child = match Command::new(&renderer_path)
            .arg("--ipc-connect")
            .arg(&addr_arg)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => { tracing::warn!(target: "fortrust.renderer", "failed spawn: {e}"); continue; }
        };

        // Wait for the renderer to connect, or fail/exit early.
        let deadline = std::time::Instant::now() + Duration::from_secs(6 + attempt * 2);
        let mut accepted = None;
        while std::time::Instant::now() < deadline {
            // If the child exited early, break and retry.
            match child.try_wait() {
                Ok(Some(status)) => {
                    tracing::warn!(target: "fortrust.renderer", "renderer exited early: {:?}", status);
                    break;
                }
                Ok(None) => {}
                Err(e) => { tracing::warn!(target: "fortrust.renderer", "child status check failed: {e}"); break; }
            }

            match listener.accept() {
                Ok((sock, _)) => { accepted = Some(sock); break; }
                Err(error) if error.kind() == ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(40)),
                Err(e) => { tracing::warn!(target: "fortrust.renderer", "accept error: {e}"); break; }
            }
        }

        if accepted.is_none() {
            // Try to kill the child process if it's still running
            let _ = child.kill();
            continue;
        }

        let sock = accepted.unwrap();
        if sock.set_nonblocking(true).is_err() { let _ = child.kill(); continue; }
        if let Ok(stream) = tokio::net::TcpStream::from_std(sock) {
            return Some(fortrust_ipc::create_tcp_endpoint(stream));
        }
    }

    None
}

fn find_renderer_binary() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(path) = std::env::var("CARGO_BIN_EXE_renderer") {
        candidates.push(PathBuf::from(path));
    }

    if let Ok(current_exe) = std::env::current_exe()
        && let Some(dir) = current_exe.parent() {
            candidates.push(dir.join(if cfg!(windows) { "renderer.exe" } else { "renderer" }));
            candidates.push(dir.join("renderer"));
    }

    if let Ok(current_dir) = std::env::current_dir() {
        for profile in ["debug", "release"] {
            candidates.push(current_dir.join("target").join(profile).join(if cfg!(windows) { "renderer.exe" } else { "renderer" }));
        }
    }

    candidates.into_iter().find(|candidate| candidate.exists())
}

fn spawn_basic_renderer_loop(receiver: fortrust_ipc::MessageReceiver, sender: fortrust_ipc::MessageSender) {
    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(_) => return,
        };

        rt.block_on(async move {
            while let Ok(msg) = receiver.recv_browser_message().await {
                match msg {
                    fortrust_ipc::BrowserToRenderer::Navigate { url } => {
                            let reply = fortrust_ipc::RendererToBrowser::TitleChanged { title: url };
                            let _ = sender.send_renderer_message(&reply).await;
                        }
                        fortrust_ipc::BrowserToRenderer::ExecuteScript { js } => {
                            let reply = fortrust_ipc::RendererToBrowser::ConsoleMessage {
                                level: "info".into(),
                                message: format!("executed script: {js}"),
                            };
                            let _ = sender.send_renderer_message(&reply).await;
                        }
                        fortrust_ipc::BrowserToRenderer::Resize { width, height } => {
                            let reply = fortrust_ipc::RendererToBrowser::LoadProgress {
                                percent: 1.0,
                                state: fortrust_ipc::LoadState::Loaded,
                            };
                            let _ = sender.send_renderer_message(&reply).await;
                            let frame = generate_basic_frame(width, height, &format!("{}x{}", width, height));
                            let _ = sender.send_renderer_message(&fortrust_ipc::RendererToBrowser::FrameReady {
                                texture_data: frame,
                                width,
                                height,
                                stride: width.saturating_mul(4),
                            }).await;
                        }
                        fortrust_ipc::BrowserToRenderer::Shutdown => break,
                        _ => {}
                    }
            }
        });
    });
}

fn spawn_renderer_event_forwarder(receiver: fortrust_ipc::MessageReceiver, event_sender: Sender<EngineEvent>) {
    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => runtime,
            Err(_) => return,
        };

        rt.block_on(async move {
            while let Ok(msg) = receiver.recv_renderer_message().await {
                let _ = event_sender.send(EngineEvent::RendererMsg { msg });
            }
        });
    });
}

fn generate_basic_frame(width: u32, height: u32, label: &str) -> Vec<u8> {
    let width = width.max(1) as usize;
    let height = height.max(1) as usize;
    let mut pixels = vec![0u8; width * height * 4];

    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            let t = y as f32 / height as f32;
            pixels[idx] = (14.0 + 18.0 * t) as u8;
            pixels[idx + 1] = (17.0 + 22.0 * t) as u8;
            pixels[idx + 2] = (20.0 + 28.0 * t) as u8;
            pixels[idx + 3] = 255;
        }
    }

    let bar_height = (height as f32 * 0.16).max(24.0) as usize;
    for y in 0..bar_height.min(height) {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            pixels[idx] = 28;
            pixels[idx + 1] = 33;
            pixels[idx + 2] = 40;
            pixels[idx + 3] = 255;
        }
    }

    let text_width = label.len().saturating_mul(7).min(width.saturating_sub(16));
    let text_height = 12usize.min(height.saturating_sub(16));
    let start_x = 8usize.min(width.saturating_sub(1));
    let start_y = (bar_height / 2).saturating_sub(text_height / 2);
    for y in 0..text_height {
        for x in 0..text_width {
            let idx = ((start_y + y).min(height - 1) * width + (start_x + x).min(width - 1)) * 4;
            pixels[idx] = 77;
            pixels[idx + 1] = 159;
            pixels[idx + 2] = 255;
            pixels[idx + 3] = 255;
        }
    }

    pixels
}

fn drain_failed_worker(
    command_receiver: Receiver<EngineCommand>,
    event_sender: Sender<EngineEvent>,
    error: String,
) {
    while let Ok(command) = command_receiver.recv() {
        let EngineCommand::Load { request_id, url, .. } = command;
        let _ = event_sender.send(EngineEvent::Failed { request_id, url, error: error.clone() });
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

        let mut speed_dial = SpeedDialState::default();
        if let Some(ref s) = storage {
            speed_dial.load_persisted_tiles(s);
        }

        let mut app = Self {
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
            sidebar_state: SidebarState::default(),
            speed_dial,
            shield: ShieldState::default(),
            theme,

            total_memory_mb: 0.0,
            last_frame: std::time::Instant::now(),
            animation_phase: 0.0,
            needs_new_tab: false,
            history_filter: String::new(),
            bookmark_filter: String::new(),
            startup_deadline: Some(std::time::Instant::now() + Duration::from_millis(1500)),
            window_hovered_close: false,
            window_hovered_min: false,
            window_hovered_max: false,

            drag_tab_id: None,
            drag_cursor_x: 0.0,
            context_menu: None,
            page_info_open: false,
            download_manager: DownloadManager::new(),
            download_dir: default_download_dir(),

            workspaces: WorkspaceManager::default(),
        };

        // Restore persisted download state and auto-resume pending downloads
        if let Some(ref s) = app.storage {
            app.download_manager.load_state_from_settings(&s.settings);
            app.download_manager.resume_all_paused(&app.download_dir);
        }

        app
    }

    fn save_download_state(&self) {
        if let Some(ref s) = self.storage {
            self.download_manager.save_state_to_settings(&s.settings);
        }
    }

    fn load_browser_config(storage: Option<&StorageDatabase>) -> BrowserConfig {
        let mut config = BrowserConfig::default();
        let Some(storage) = storage else { return config; };

        macro_rules! load_str {
            ($key:expr) => {
                storage.settings.load($key).and_then(|v| v.as_string().map(str::to_owned))
            };
        }
        macro_rules! load_bool {
            ($key:expr) => {
                storage.settings.load($key).and_then(|v| v.as_bool())
            };
        }
        macro_rules! load_int {
            ($key:expr) => {
                storage.settings.load($key).and_then(|v| v.as_int().map(|i| i as u8))
            };
        }

        if let Some(v) = load_str!(SETTINGS_UI_THEME) { config.ui.theme = v; }
        if let Some(v) = load_bool!(SETTINGS_UI_COMPACT) { config.ui.compact_density = v; }
        if let Some(v) = load_str!(SETTINGS_UI_WALLPAPER) { config.ui.wallpaper = v; }
        if let Some(v) = load_int!(SETTINGS_UI_WALLPAPER_STRENGTH) { config.ui.wallpaper_strength = v; }
        if let Some(v) = load_bool!(SETTINGS_UI_PRIVACY_PANEL) { config.ui.show_privacy_panel = v; }
        if let Some(v) = load_bool!(SETTINGS_UI_MEMORY_METER) { config.ui.show_memory_meter = v; }
        if let Some(v) = load_int!(SETTINGS_UI_GLASS) { config.ui.glass_strength = v.clamp(0, 100); }
        if let Some(v) = load_int!(SETTINGS_UI_MOTION) { config.ui.motion_strength = v.clamp(0, 100); }
        if let Some(v) = load_bool!(SETTINGS_PRIVACY_BLOCK_ADS) { config.privacy.block_ads = v; }
        if let Some(v) = load_bool!(SETTINGS_PRIVACY_BLOCK_TRACKERS) { config.privacy.block_trackers = v; }
        if let Some(v) = load_bool!(SETTINGS_PRIVACY_THIRD_PARTY) { config.privacy.block_third_party_cookies = v; }
        if let Some(v) = load_bool!(SETTINGS_PRIVACY_STRIP_PARAMS) { config.privacy.strip_tracking_query_params = v; }
        if let Some(v) = load_bool!(SETTINGS_PRIVACY_HTTPS_ONLY) { config.privacy.https_only_mode = v; }
        if let Some(v) = load_bool!(SETTINGS_PRIVACY_GPC) { config.privacy.global_privacy_control = v; }
        if let Some(v) = load_bool!(SETTINGS_PRIVACY_DNT) { config.privacy.do_not_track = v; }
        if let Some(v) = load_bool!(SETTINGS_PRIVACY_FINGERPRINT) { config.privacy.fingerprint_noise = v; }

        config
    }

    #[allow(dead_code)]
    fn persist_browser_config(&self) {
        let Some(ref storage) = self.storage else { return; };

        macro_rules! save_str {
            ($key:expr, $val:expr) => {
                let _ = storage.settings.store($key, &SettingValue::from($val.clone()));
            };
        }
        macro_rules! save_bool {
            ($key:expr, $val:expr) => {
                let _ = storage.settings.store($key, &SettingValue::from($val));
            };
        }
        macro_rules! save_int {
            ($key:expr, $val:expr) => {
                let _ = storage.settings.store($key, &SettingValue::from(i64::from($val)));
            };
        }

        save_str!(SETTINGS_UI_THEME, self.config.ui.theme);
        save_bool!(SETTINGS_UI_COMPACT, self.config.ui.compact_density);
        save_bool!(SETTINGS_UI_PRIVACY_PANEL, self.config.ui.show_privacy_panel);
        save_bool!(SETTINGS_UI_MEMORY_METER, self.config.ui.show_memory_meter);
        save_int!(SETTINGS_UI_GLASS, self.config.ui.glass_strength);
        save_str!(SETTINGS_UI_WALLPAPER, self.config.ui.wallpaper);
        save_int!(SETTINGS_UI_WALLPAPER_STRENGTH, self.config.ui.wallpaper_strength);
        save_int!(SETTINGS_UI_MOTION, self.config.ui.motion_strength);
        save_bool!(SETTINGS_PRIVACY_BLOCK_ADS, self.config.privacy.block_ads);
        save_bool!(SETTINGS_PRIVACY_BLOCK_TRACKERS, self.config.privacy.block_trackers);
        save_bool!(SETTINGS_PRIVACY_THIRD_PARTY, self.config.privacy.block_third_party_cookies);
        save_bool!(SETTINGS_PRIVACY_STRIP_PARAMS, self.config.privacy.strip_tracking_query_params);
        save_bool!(SETTINGS_PRIVACY_HTTPS_ONLY, self.config.privacy.https_only_mode);
        save_bool!(SETTINGS_PRIVACY_GPC, self.config.privacy.global_privacy_control);
        save_bool!(SETTINGS_PRIVACY_DNT, self.config.privacy.do_not_track);
        save_bool!(SETTINGS_PRIVACY_FINGERPRINT, self.config.privacy.fingerprint_noise);
    }

    fn theme_for_mode(theme: &str, glass_strength: u8) -> FortrustTheme {
        match theme.to_ascii_lowercase().as_str() {
            "light" => FortrustTheme::light_with_glass_strength(glass_strength),
            _ => FortrustTheme::dark_with_glass_strength(glass_strength),
        }
    }

    fn refresh_theme(&mut self, ctx: &Context) {
        self.theme = Self::theme_for_mode(&self.config.ui.theme, self.config.ui.glass_strength);
        apply_egui_style(ctx, &self.theme, &self.config.ui);
    }

    fn apply_renderer_message(&mut self, ctx: &Context, tab_id: TabId, msg: fortrust_ipc::RendererToBrowser) {
        let is_active = self.active_tab_id() == Some(tab_id);

        match msg {
            fortrust_ipc::RendererToBrowser::TitleChanged { title }
            | fortrust_ipc::RendererToBrowser::DocumentTitleChanged { title } => {
                if let Some(tab) = self.tab_pages.get_mut(&tab_id) {
                    if let Some(page) = tab.page.as_mut() {
                        page.title = title.clone();
                    }
                    if let Some(current_url) = tab.current_url().map(str::to_owned) {
                        self.tabs.navigate_tab(tab_id, current_url, title);
                    }
                }
            }
            fortrust_ipc::RendererToBrowser::UrlChanged { url } => {
                if let Some(tab) = self.tab_pages.get_mut(&tab_id) {
                    tab.replace_history(url.clone());
                }
                if is_active {
                    self.omnibox.text = url;
                }
            }
            fortrust_ipc::RendererToBrowser::LoadProgress { percent: _, state } => {
                if matches!(state, fortrust_ipc::LoadState::Loaded)
                    && let Some(tab) = self.tab_pages.get_mut(&tab_id) {
                        tab.loading_url = None;
                }
            }
            fortrust_ipc::RendererToBrowser::LoadComplete => {
                if let Some(tab) = self.tab_pages.get_mut(&tab_id) {
                    tab.loading_url = None;
                }
            }
            fortrust_ipc::RendererToBrowser::FrameReady { texture_data, width, height, stride } => {
                if let Some(rgba) = frame_bytes_to_rgba(&texture_data, width, height, stride) {
                    let image = egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
                    let tex_name = format!("fortrust-renderer-frame-{}", tab_id.0);
                    let texture = ctx.load_texture(&tex_name, image, egui::TextureOptions::LINEAR);
                    if let Some(tab) = self.tab_pages.get_mut(&tab_id) {
                        // Drop previous texture handle (frees GPU resource) before replacing
                        if let Some(prev) = tab.renderer_frame.replace(texture) {
                            drop(prev);
                        }
                    }
                }
            }
            fortrust_ipc::RendererToBrowser::NavigationStart { url } => {
                if let Some(tab) = self.tab_pages.get_mut(&tab_id) {
                    tab.loading_url = Some(url);
                }
            }
            fortrust_ipc::RendererToBrowser::NavigationError { url: _, error } => {
                if let Some(tab) = self.tab_pages.get_mut(&tab_id) {
                    tab.load_error = Some(error);
                    tab.loading_url = None;
                }
            }
            fortrust_ipc::RendererToBrowser::ConsoleMessage { level, message } => {
                tracing::info!(target: "fortrust.renderer", level = %level, message = %message);
            }
            fortrust_ipc::RendererToBrowser::DomEvent { origin: _, name, detail } => {
                // Structured DOM event forwarded from renderer.
                self.handle_js_event(ctx, tab_id, &name, &detail);
            }
            fortrust_ipc::RendererToBrowser::PrivacyEvent { event } => {
                tracing::info!(target: "fortrust.renderer", ?event);
            }
            fortrust_ipc::RendererToBrowser::RendererCrashed { reason } => {
                tracing::error!(target: "fortrust.renderer", %reason);
            }
            fortrust_ipc::RendererToBrowser::ShutdownAck
            | fortrust_ipc::RendererToBrowser::FaviconUpdated { .. }
            | fortrust_ipc::RendererToBrowser::Alert { .. }
            | fortrust_ipc::RendererToBrowser::NewTabRequested { .. }
            | fortrust_ipc::RendererToBrowser::DownloadRequested { .. }
            | fortrust_ipc::RendererToBrowser::ScrollPosition { .. }
            | fortrust_ipc::RendererToBrowser::MemoryUsage { .. } => {}
        }

        ctx.request_repaint();
    }

    fn handle_js_event(&mut self, ctx: &Context, tab_id: TabId, name: &str, detail: &str) {
        tracing::info!(target: "fortrust.renderer", "JS event received: {} -> {}", name, detail);

        // Handle some common event names emitted from JS
        match name.to_ascii_lowercase().as_str() {
            "title" | "settitle" | "document.title" => {
                if let Some(tab) = self.tab_pages.get_mut(&tab_id) {
                    if let Some(page) = tab.page.as_mut() {
                        page.title = detail.to_owned();
                    }
                    // Update tab manager display title if URL known
                    if let Some(current_url) = tab.current_url().map(str::to_owned) {
                        self.tabs.navigate_tab(tab_id, current_url, detail.to_owned());
                    }
                }
                ctx.request_repaint();
            }
            _ => {
                // For now, just log and surface to page console via tracing; later we can forward structured IPC.
                tracing::info!(target: "fortrust.renderer", "Unhandled JS event: {} -> {}", name, detail);
            }
        }
    }

    fn motion_scale(&self) -> f32 {
        0.65 + (self.config.ui.motion_strength.min(100) as f32 / 100.0) * 0.85
    }

    #[allow(dead_code)]
    fn rebuild_privacy_pipeline(&mut self) {
        self.privacy = PrivacyEngine::new(self.config.privacy.clone());
        self.engine_worker = EngineWorker::spawn(self.config.privacy.clone());
    }

    fn open_storage() -> Option<StorageDatabase> {
        let base = if cfg!(target_os = "windows") {
            std::env::var("APPDATA").ok().map(|p| format!("{p}\\Fortrust"))
        } else {
            std::env::var("HOME").ok().map(|p| format!("{p}/.local/share/fortrust"))
        };
        match base {
            Some(dir) => {
                let path = format!("{dir}\\storage.redb");
                let _ = std::fs::create_dir_all(&dir);
                Some(StorageDatabase::open_or_default(path))
            }
            None => { tracing::warn!("No data directory found"); None }
        }
    }

    fn active_tab_id(&self) -> Option<TabId> { self.tabs.active_id() }
    fn active_state(&self) -> Option<&TabPageState> {
        self.active_tab_id().and_then(|id| self.tab_pages.get(&id))
    }
    fn tab_state_mut(&mut self, id: TabId) -> &mut TabPageState {
        self.tab_pages.entry(id).or_insert_with(|| TabPageState::new("fortrust://start"))
    }

    fn current_bookmarked(&self) -> bool {
        let Some(url) = self.active_state().and_then(|s| s.current_url()).or_else(|| self.tabs.active_tab().map(|t| t.url.as_str())) else {
            return false;
        };
        self.storage.as_ref().and_then(|s| s.bookmarks.get_by_url(url)).is_some()
    }

    fn toggle_bookmark(&mut self) {
        let Some(url) = self.active_state().and_then(|s| s.current_url()).or_else(|| self.tabs.active_tab().map(|t| t.url.as_str())).map(str::to_owned) else { return; };
        let url_c = url.clone();
        let title = self.active_state().and_then(|s| s.page.as_ref().map(|p| p.title.clone()))
            .or_else(|| self.tabs.active_tab().map(|t| t.title.to_string()))
            .unwrap_or(url_c);
        let Some(ref storage) = self.storage else { return; };
        if storage.bookmarks.get_by_url(&url).is_some() {
            let _ = storage.bookmarks.delete(&url);
        } else {
            let bm = fortrust_storage::Bookmark {
                id: url.clone(),
                url: url.clone(),
                title,
                folder_id: None,
                added_at: chrono::Utc::now(),
                last_visited: None,
                visit_count: 0,
                icon_data: None,
                description: None,
            };
            let _ = storage.bookmarks.store(&bm);
        }
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
                let effective = decision.effective_url.unwrap_or(decision.original_url);
                self.commit_navigation(effective, history_mode);
            }
        }
    }

    fn show_blocked_page(&mut self, original_url: String, _reason: BlockReason, history_mode: HistoryMode) {
        let Some(tab_id) = self.active_tab_id() else { return; };
        self.shield.ads_blocked = self.shield.ads_blocked.saturating_add(1);
        self.tabs.record_privacy_block(tab_id);
        self.tabs.navigate_tab(tab_id, "fortrust://blocked", "Blocked");
        self.omnibox.text = original_url;
        let state = self.tab_state_mut(tab_id);
        match history_mode {
            HistoryMode::Push => state.push_history("fortrust://blocked".to_owned()),
            HistoryMode::Replace => state.replace_history("fortrust://blocked".to_owned()),
        }
        state.clear_document();
    }

    fn commit_navigation(&mut self, url: String, history_mode: HistoryMode) {
        let Some(tab_id) = self.active_tab_id() else { return; };
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

        let request_id = self.engine_worker.load(url.clone(), Viewport {
            width: 1180.0, height: 760.0,
        });
        self.request_owner.insert(request_id, tab_id);
        self.tab_state_mut(tab_id).begin_load(url, request_id);
    }

    fn go_back(&mut self) {
        let Some(tab_id) = self.active_tab_id() else { return; };
        if let Some(url) = self.tab_state_mut(tab_id).go_back() {
            self.omnibox.text = url.clone();
            self.navigate_input(url, HistoryMode::Replace);
        }
    }

    fn go_forward(&mut self) {
        let Some(tab_id) = self.active_tab_id() else { return; };
        if let Some(url) = self.tab_state_mut(tab_id).go_forward() {
            self.omnibox.text = url.clone();
            self.navigate_input(url, HistoryMode::Replace);
        }
    }

    fn reload(&mut self) {
        let url = self.active_state()
            .and_then(TabPageState::current_url)
            .map(str::to_owned)
            .unwrap_or_else(|| self.omnibox.text.clone());
        self.navigate_input(url, HistoryMode::Replace);
    }

    fn can_go_back(&self) -> bool { self.active_state().is_some_and(TabPageState::can_go_back) }
    fn can_go_forward(&self) -> bool { self.active_state().is_some_and(TabPageState::can_go_forward) }

    fn open_new_tab(&mut self) {
        let id = self.tabs.open_tab("fortrust://start", "Speed Dial", false);
        let ws_id = self.workspaces.active();
        self.workspaces.add_tab(ws_id, id);
        if let Some(tab) = self.tabs.tabs_mut().iter_mut().find(|t| t.id == id) {
            tab.workspace_id = Some(ws_id);
        }
        self.tab_pages.insert(id, TabPageState::new("fortrust://start"));
        self.omnibox.text = "fortrust://start".to_owned();
        self.needs_new_tab = true;
    }

    fn close_tab(&mut self, id: TabId) {
        self.tabs.close_tab(id);
        self.tab_pages.remove(&id);
        self.request_owner.retain(|_, owner| *owner != id);
        if self.tabs.tabs().is_empty() { self.open_new_tab(); }
        if let Some(tab) = self.tabs.active_tab() { self.omnibox.text = tab.url.to_string(); }
    }

    fn cycle_tab(&mut self, forward: bool) {
        let tabs = self.tabs.tabs();
        if tabs.len() < 2 { return; }
        let active = self.active_tab_id();
        let idx = active.and_then(|id| tabs.iter().position(|t| t.id == id)).unwrap_or(0);
        let next = if forward {
            (idx + 1) % tabs.len()
        } else {
            (idx + tabs.len() - 1) % tabs.len()
        };
        self.tabs.activate(tabs[next].id);
        if let Some(tab) = self.tabs.active_tab() { self.omnibox.text = tab.url.to_string(); }
    }

    fn poll_loading(&mut self, ctx: &Context) {
        while let Ok(event) = self.engine_worker.receiver.try_recv() {
            match event {
                EngineEvent::Loaded { request_id, page } => {
                    let Some(tab_id) = self.request_owner.remove(&request_id) else { continue; };
                    let page = *page;
                    let memory_estimate = (page.security.body_bytes as f32 / 1024.0 / 1024.0).max(0.25)
                        + (page.security.display_commands as f32 * 0.01);
                    let title = page.title.clone();
                    let url = page.url.clone();
                    let accepted = self.tab_state_mut(tab_id).finish_load(request_id, page);
                    if accepted {
                        self.tabs.navigate_tab(tab_id, url.clone(), title.clone());
                        if self.active_tab_id() == Some(tab_id) { self.omnibox.text = url.clone(); }
                        if !url.starts_with("fortrust://") && !url.starts_with("about:")
                            && let Some(ref storage) = self.storage
                        {
                            let entry = HistoryEntry {
                                url: url.clone(), title, visit_time: Utc::now(),
                                visit_count: 1, typed_count: 0, is_bookmarked: false,
                            };
                            let _ = storage.history.store(&entry);
                        }
                        self.total_memory_mb = memory_estimate;
                    }
                }
                EngineEvent::Failed { request_id, url, error } => {
                    let Some(tab_id) = self.request_owner.remove(&request_id) else { continue; };
                    let err_msg = error.clone();
                    let accepted = self.tab_state_mut(tab_id).fail_load(request_id, error);
                    if accepted { tracing::warn!("Load failed for {}: {}", url, err_msg); }
                }
                EngineEvent::RendererMsg { msg } => {
                    if let Some(tab_id) = self.active_tab_id() {
                        self.apply_renderer_message(ctx, tab_id, msg);
                    }
                }
            }
        }
        if self.tab_pages.values().any(|state| state.loading_url.is_some()) {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    // ── TAB BAR ─────────────────────────────────────────────

    fn render_tab_bar(&mut self, ctx: &Context) {
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        let pointer_released = ctx.input(|i| i.pointer.any_released());
        let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
        let pointer_x = pointer_pos.map(|p| p.x).unwrap_or(0.0);

        // End drag on release
        if self.drag_tab_id.is_some() && pointer_released {
            self.drag_tab_id = None;
        }

        egui::TopBottomPanel::top("fortrust_tab_bar")
            .exact_height(37.0)
            .frame(Frame {
                fill: self.theme.surface_tab_bar,
                inner_margin: Margin::symmetric(4, 0),
                outer_margin: Margin::ZERO,
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(2.0);

                    let active = self.tabs.active_id();
                    let mut clicked = None;
                    let mut closed = None;
                    // Track cumulative tab x positions for reorder detection
                    let tab_count = self.tabs.tabs().len();
                    let mut tab_x_positions: Vec<(TabId, f32, f32)> = Vec::with_capacity(tab_count); // (id, left, right)
                    let mut reorder_target: Option<(TabId, usize)> = None;

                    for (i, tab) in self.tabs.tabs().iter().enumerate() {
                        let is_dragging = self.drag_tab_id == Some(tab.id);

                        let tab_size = Vec2::new(
                            (tab.title.len() as f32 * 6.5 + 60.0).clamp(60.0, 180.0),
                            27.0,
                        );
                        let tab_rect = ui.allocate_space(tab_size).1;
                        let tab_rect = Rect::from_min_size(tab_rect.min, tab_size);
                        tab_x_positions.push((tab.id, tab_rect.min.x, tab_rect.max.x));

                        // Start drag on pointer down + movement
                        if !is_dragging && self.drag_tab_id.is_none()
                            && pointer_down && ui.rect_contains_pointer(tab_rect)
                        {
                            self.drag_tab_id = Some(tab.id);
                            self.drag_cursor_x = pointer_x;
                        }

                        let selected = Some(tab.id) == active;

                        // Skip painting the original position of the dragged tab
                        if !is_dragging {
                            // Tab hover and active background
                            if selected {
                                ui.painter().rect_filled(tab_rect, CornerRadius::same(6), self.theme.surface_deepest);
                            } else if ui.rect_contains_pointer(tab_rect) {
                                ui.painter().rect_filled(tab_rect, CornerRadius::same(6), self.theme.surface_hover);
                            }

                            let is_speed_dial = tab.url.starts_with("fortrust://start");
                            let is_secure = tab.url.starts_with("https://");

                            // Favicon based on page type
                            if is_speed_dial {
                                let fg = Color32::from_rgba_unmultiplied(79, 158, 255, 180);
                                let fg2 = Color32::from_rgba_unmultiplied(79, 158, 255, 100);
                                let fav_rect = Rect::from_min_size(Pos2::new(tab_rect.min.x + 6.0, tab_rect.center().y - 6.5), Vec2::new(13.0, 13.0));
                                ui.painter().rect_filled(fav_rect, CornerRadius::same(2), Color32::from_rgb(26, 32, 48));
                                ui.painter().rect_filled(Rect::from_min_size(Pos2::new(fav_rect.min.x + 2.0, fav_rect.min.y + 2.0), Vec2::new(4.0, 4.0)), CornerRadius::same(1), fg);
                                ui.painter().rect_filled(Rect::from_min_size(Pos2::new(fav_rect.min.x + 7.0, fav_rect.min.y + 2.0), Vec2::new(4.0, 4.0)), CornerRadius::same(1), fg);
                                ui.painter().rect_filled(Rect::from_min_size(Pos2::new(fav_rect.min.x + 2.0, fav_rect.min.y + 7.0), Vec2::new(4.0, 4.0)), CornerRadius::same(1), fg2);
                                ui.painter().rect_filled(Rect::from_min_size(Pos2::new(fav_rect.min.x + 7.0, fav_rect.min.y + 7.0), Vec2::new(4.0, 4.0)), CornerRadius::same(1), fg2);
                            } else {
                                // Globe icon for web pages
                                let fav_rect = Rect::from_min_size(Pos2::new(tab_rect.min.x + 6.0, tab_rect.center().y - 6.5), Vec2::new(13.0, 13.0));
                                let c = if is_secure { self.theme.accent_shield } else { self.theme.text_secondary };
                                ui.painter().circle_stroke(Pos2::new(fav_rect.center().x, fav_rect.center().y), 5.5, Stroke::new(1.3, c));
                                let cx = fav_rect.center().x;
                                let cy = fav_rect.center().y;
                                ui.painter().line_segment([Pos2::new(cx - 4.5, cy), Pos2::new(cx + 4.5, cy)], Stroke::new(1.3, c));
                                ui.painter().line_segment([Pos2::new(cx, cy - 4.5), Pos2::new(cx, cy + 4.5)], Stroke::new(1.3, c));
                            }

                            // Title text
                            let title_color = if selected { self.theme.text_primary } else { self.theme.text_secondary };
                            let title_x = tab_rect.min.x + 22.0;
                            let display_title: String = if tab.title.len() > 18 { format!("{}...", &tab.title[..15]) } else { tab.title.to_string() };
                            ui.painter().text(
                                Pos2::new(title_x, tab_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                display_title,
                                egui::FontId::proportional(12.0),
                                title_color,
                            );

                            let click_resp = ui.allocate_rect(tab_rect, egui::Sense::click());
                            if click_resp.clicked() { clicked = Some(tab.id); }
                            if click_resp.middle_clicked() { closed = Some(tab.id); }
                            if click_resp.secondary_clicked() {
                                self.context_menu = Some((pointer_pos.unwrap_or(Pos2::ZERO), tab.id));
                            }

                            // Close button on hover
                            if ui.rect_contains_pointer(tab_rect) {
                                let close_rect = Rect::from_min_size(Pos2::new(tab_rect.max.x - 19.0, tab_rect.center().y - 7.5), Vec2::new(15.0, 15.0));
                                let close_hovered = ui.rect_contains_pointer(close_rect);
                                if close_hovered { ui.painter().rect_filled(close_rect, CornerRadius::same(3), self.theme.glass_hover); }
                                icons::paint_close_icon(ui.painter(), close_rect, self.theme.text_muted);
                                if ui.allocate_rect(close_rect, egui::Sense::click()).clicked() { closed = Some(tab.id); }
                            }

                            // Active underline
                            if selected {
                                ui.painter().rect_filled(
                                    Rect::from_min_size(
                                        Pos2::new(tab_rect.min.x, tab_rect.max.y - 1.5),
                                        Vec2::new(tab_rect.width(), 1.5),
                                    ),
                                    CornerRadius::same(1),
                                    self.theme.accent_primary,
                                );
                            }
                        }

                        // Reorder detection: if dragging this tab past another
                        if self.drag_tab_id == Some(tab.id) && pointer_down && i + 1 < tab_x_positions.len() {
                            let next_left = tab_x_positions.get(i + 1).map(|&(_, l, _)| l).unwrap_or(tab_rect.max.x);
                            if pointer_x > next_left {
                                reorder_target = Some((tab.id, (i + 1).min(tab_x_positions.len() - 1)));
                            }
                        }
                    }

                    // Paint drop indicator if dragging
                    if let Some(drag_id) = self.drag_tab_id {
                        // Paint ghost at cursor
                        let ghost_w = (self.tabs.tabs().iter().find(|t| t.id == drag_id)
                            .map(|t| (t.title.len() as f32 * 6.5 + 60.0).clamp(60.0, 180.0))
                            .unwrap_or(120.0)).min(200.0);
                        let ghost_rect = Rect::from_min_size(
                            Pos2::new(pointer_x - ghost_w / 2.0, 6.0),
                            Vec2::new(ghost_w, 27.0),
                        );
                        ui.painter().rect_filled(ghost_rect, CornerRadius::same(6), Color32::from_rgba_unmultiplied(self.theme.accent_primary.r(), self.theme.accent_primary.g(), self.theme.accent_primary.b(), 40));
                        ui.painter().rect_stroke(ghost_rect, CornerRadius::same(6), Stroke::new(1.0, Color32::from_rgba_unmultiplied(self.theme.accent_primary.r(), self.theme.accent_primary.g(), self.theme.accent_primary.b(), 80)), egui::StrokeKind::Inside);
                        if let Some(tab) = self.tabs.tabs().iter().find(|t| t.id == drag_id) {
                            ui.painter().text(
                                Pos2::new(ghost_rect.center().x, ghost_rect.center().y),
                                egui::Align2::CENTER_CENTER,
                                if tab.title.len() > 12 { format!("{}...", &tab.title[..10]) } else { tab.title.to_string() },
                                egui::FontId::proportional(12.0),
                                Color32::from_rgba_unmultiplied(200, 220, 255, 180),
                            );
                        }
                    }

                    // Apply reorder if needed (outside the borrow of tabs)
                    if let Some((id, new_idx)) = reorder_target
                        && self.tabs.reorder_tab(id, new_idx)
                    {
                        ctx.request_repaint();
                    }

                    // Add tab button
                    let add_tab_btn = ui.add(
                        egui::Button::new("")
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(5)
                            .min_size(Vec2::new(26.0, 26.0)),
                    );
                    icons::paint_plus_icon(ui.painter(), add_tab_btn.rect, self.theme.text_muted);
                    if add_tab_btn.clicked() { self.open_new_tab(); }

                    // Spacer — window controls (always visible, macOS-style)
                    let wc_btn_size = 12.0;
                    let wc_gap = 6.0;
                    let wc_needed = 8.0 + wc_btn_size + wc_gap + wc_btn_size + wc_gap + wc_btn_size + 8.0;
                    if ui.available_width() >= wc_needed {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(8.0);

                            // Close (red dot)
                            let close_rect = ui.allocate_space(Vec2::new(wc_btn_size, wc_btn_size)).1;
                            let close_hovered = ui.rect_contains_pointer(close_rect);
                            ui.painter().circle_filled(close_rect.center(), wc_btn_size / 2.0, Color32::from_rgb(255, 95, 87));
                            if close_hovered {
                                icons::paint_close_icon(ui.painter(), Rect::from_center_size(close_rect.center(), Vec2::new(8.0, 8.0)), Color32::from_rgba_unmultiplied(80, 20, 15, 200));
                            } else {
                                ui.painter().circle_filled(close_rect.center(), wc_btn_size / 4.0, Color32::from_rgba_unmultiplied(160, 40, 40, 120));
                            }
                            ui.add_space(wc_gap);
                            let close_clicked = ui.allocate_rect(close_rect, egui::Sense::click()).clicked();

                            // Minimize (yellow dot)
                            let min_rect = ui.allocate_space(Vec2::new(wc_btn_size, wc_btn_size)).1;
                            let min_hovered = ui.rect_contains_pointer(min_rect);
                            ui.painter().circle_filled(min_rect.center(), wc_btn_size / 2.0, Color32::from_rgb(254, 188, 46));
                            if min_hovered {
                                ui.painter().line_segment(
                                    [Pos2::new(min_rect.center().x - 2.5, min_rect.center().y), Pos2::new(min_rect.center().x + 2.5, min_rect.center().y)],
                                    Stroke::new(1.5, Color32::from_rgba_unmultiplied(120, 80, 15, 200)),
                                );
                            } else {
                                ui.painter().circle_filled(min_rect.center(), wc_btn_size / 4.0, Color32::from_rgba_unmultiplied(160, 100, 25, 120));
                            }
                            ui.add_space(wc_gap);
                            let min_clicked = ui.allocate_rect(min_rect, egui::Sense::click()).clicked();

                            // Maximize (green dot)
                            let max_rect = ui.allocate_space(Vec2::new(wc_btn_size, wc_btn_size)).1;
                            let max_hovered = ui.rect_contains_pointer(max_rect);
                            ui.painter().circle_filled(max_rect.center(), wc_btn_size / 2.0, Color32::from_rgb(40, 200, 64));
                            if max_hovered {
                                let sq = Rect::from_center_size(max_rect.center(), Vec2::new(5.0, 5.0));
                                ui.painter().rect_stroke(sq, CornerRadius::same(1), Stroke::new(1.5, Color32::from_rgba_unmultiplied(15, 90, 25, 200)), egui::StrokeKind::Inside);
                            } else {
                                ui.painter().circle_filled(max_rect.center(), wc_btn_size / 4.0, Color32::from_rgba_unmultiplied(25, 120, 40, 120));
                            }
                            let max_clicked = ui.allocate_rect(max_rect, egui::Sense::click()).clicked();

                            if min_clicked { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true)); }
                            if max_clicked { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(true)); }
                            if close_clicked { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
                        });
                    }

                    if let Some(id) = clicked {
                        self.tabs.activate(id);
                        if let Some(tab) = self.tabs.active_tab() { self.omnibox.text = tab.url.to_string(); }
                    }
                    if let Some(id) = closed { self.close_tab(id); }
                });
            });
        // Tab context menu
        self.render_tab_context_menu(ctx);
    }

    fn render_tab_context_menu(&mut self, ctx: &Context) {
        let Some((pos, tab_id)) = self.context_menu else { return; };
        let tab_url = self.tabs.tabs().iter().find(|t| t.id == tab_id).map(|t| t.url.as_str()).unwrap_or("").to_owned();
        let tab_title = self.tabs.tabs().iter().find(|t| t.id == tab_id).map(|t| t.title.as_str()).unwrap_or("").to_owned();

        let area_id = egui::Id::new("tab_context_menu");
        egui::Area::new(area_id)
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                let mut close = false;
                let mut action: Option<&str> = None;

                let frame = egui::Frame {
                    fill: self.theme.surface_sidebar,
                    stroke: egui::Stroke::new(1.0, self.theme.border_strong),
                    corner_radius: egui::CornerRadius::same(8),
                    inner_margin: egui::Margin::same(4),
                    ..Default::default()
                };
                frame.show(ui, |ui| {
                    ui.set_min_width(170.0);
                    for item in &["Close Tab", "Close Others", "Close Right", "Reload", "Duplicate"] {
                        let resp = ui.selectable_label(false, *item);
                        if resp.clicked() {
                            close = true;
                            action = Some(item);
                        }
                    }
                });

                if close {
                    match action {
                        Some("Close Tab") => { self.close_tab(tab_id); }
                        Some("Close Others") => {
                            let ids: Vec<_> = self.tabs.tabs().iter().filter(|t| t.id != tab_id).map(|t| t.id).collect();
                            for id in ids { self.close_tab(id); }
                        }
                        Some("Close Right") => {
                            let ids: Vec<_> = self.tabs.tabs().iter().skip_while(|t| t.id != tab_id).skip(1).map(|t| t.id).collect();
                            for id in ids { self.close_tab(id); }
                        }
                        Some("Reload") => {
                            self.omnibox.text = tab_url.clone();
                            self.navigate_input(tab_url.clone(), crate::app::HistoryMode::Replace);
                        }
                        Some("Duplicate") => {
                            self.open_new_tab();
                            if let Some(new_id) = self.active_tab_id() {
                                self.tabs.navigate_tab(new_id, tab_url.clone(), tab_title.clone());
                            }
                        }
                        _ => {}
                    }
                    self.context_menu = None;
                }

                // Close on click outside
                if ui.input(|i| i.pointer.any_click()) && !ui.rect_contains_pointer(ui.min_rect()) {
                    self.context_menu = None;
                }
            });
    }

    fn history_suggestions(&self) -> Vec<SuggestionItem> {
        let Some(ref storage) = self.storage else { return vec![] };
        let Ok(entries) = storage.history.recently_visited(20) else { return vec![] };
        entries.into_iter().map(|e| SuggestionItem {
            kind: if e.is_bookmarked { SuggestionKind::Bookmark } else { SuggestionKind::History },
            text: if e.title.is_empty() { e.url.clone() } else { e.title.clone() },
            url: e.url,
        }).collect()
    }

    // ── ADDRESS BAR ─────────────────────────────────────────

    fn render_address_bar(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("fortrust_address_bar")
            .exact_height(37.0)
            .frame(Frame {
                fill: self.theme.surface_tab_bar,
                inner_margin: Margin::symmetric(8, 0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Back button with SVG
                    let back_enabled = self.can_go_back();
                    let back_color = if back_enabled { self.theme.text_secondary } else { self.theme.text_muted };
                    let back_resp = ui.add(
                        egui::Button::new("")
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(5)
                            .min_size(Vec2::new(28.0, 28.0)),
                    );
                    icons::paint_back_icon(ui.painter(), back_resp.rect, back_color);
                    if back_resp.clicked() && back_enabled { self.go_back(); }

                    // Forward button with SVG
                    let fwd_enabled = self.can_go_forward();
                    let fwd_color = if fwd_enabled { self.theme.text_secondary } else { self.theme.text_muted };
                    let fwd_resp = ui.add(
                        egui::Button::new("")
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(5)
                            .min_size(Vec2::new(28.0, 28.0)),
                    );
                    icons::paint_forward_icon(ui.painter(), fwd_resp.rect, fwd_color);
                    if fwd_resp.clicked() && fwd_enabled { self.go_forward(); }

                    // Reload button with SVG
                    let reload_resp = ui.add(
                        egui::Button::new("")
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(5)
                            .min_size(Vec2::new(28.0, 28.0)),
                    );
                    icons::paint_reload_icon(ui.painter(), reload_resp.rect, self.theme.text_secondary);
                    if reload_resp.clicked() { self.reload(); }

                    ui.add_space(4.0);

                    // Address pill
                    let history_suggestions = self.history_suggestions();
                    if let Some(url) = self.omnibox.render(ui, &self.theme, &history_suggestions) {
                        self.navigate_input(url, HistoryMode::Push);
                    }

                    // Right toolbar buttons (hide if too narrow)
                    ui.add_space(4.0);
                    if ui.available_width() > 160.0 {

                    // Favorites with SVG
                    let is_bm = self.current_bookmarked();
                    let star_resp = ui.add(
                        egui::Button::new("")
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(5)
                            .min_size(Vec2::new(28.0, 28.0)),
                    );
                    let star_color = if is_bm { self.theme.accent_primary } else { self.theme.text_muted };
                    icons::paint_star_icon(ui.painter(), star_resp.rect, star_color);
                    if star_resp.clicked() { self.toggle_bookmark(); }

                    // Sidebar toggle with SVG
                    let sb_resp = ui.add(
                        egui::Button::new("")
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(5)
                            .min_size(Vec2::new(28.0, 28.0)),
                    );
                    icons::paint_sidebar_icon(ui.painter(), sb_resp.rect, self.theme.text_muted);
                    if sb_resp.clicked() {
                        if self.sidebar_anim.is_open() { self.sidebar_anim.close(); }
                        else { self.sidebar_anim.open(); }
                    }

                    // Menu button with SVG
                    let menu_resp = ui.add(
                        egui::Button::new("")
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(5)
                            .min_size(Vec2::new(28.0, 28.0)),
                    );
                    icons::paint_menu_icon(ui.painter(), menu_resp.rect, self.theme.text_muted);

                    // Shield button
                    self.shield.render_button(ui, &self.theme);

                    // Memory meter
                    if self.config.ui.show_memory_meter {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(format!("MB {:.1}", self.total_memory_mb))
                                .size(11.0)
                                .color(self.theme.text_muted),
                        );
                    }
                    } // end available_width > 160 for right toolbar
                });
            });
    }

    // ── CENTRAL PANEL ──────────────────────────────────────

    fn render_central_panel(&mut self, ctx: &Context) {
        let mut navigate: Option<String> = None;

        egui::CentralPanel::default()
            .frame(Frame {
                fill: self.theme.surface_deepest,
                inner_margin: Margin::ZERO,
                outer_margin: Margin::ZERO,
                ..Default::default()
            })
            .show(ctx, |ui| {
                // Render icon rail on the left
                egui::SidePanel::left("fortrust_icon_rail")
                    .exact_width(30.0)
                    .resizable(false)
                    .frame(Frame {
                        fill: self.theme.surface_rail,
                        inner_margin: Margin::ZERO,
                        outer_margin: Margin::ZERO,
                        ..Default::default()
                    })
                    .show_inside(ui, |ui| {
                        self.sidebar_state.render_icon_rail(ui, &self.theme, &mut self.sidebar_anim);
                    });

                // Render sidebar overlay
                let old_theme = self.config.ui.theme.clone();
                let downloads_entries = self.download_manager.all_downloads();
                if let Some(url) = self.sidebar_state.render_overlay(ui, &self.theme, &mut self.sidebar_anim, &mut self.config, self.storage.as_ref(), &downloads_entries, &mut self.workspaces) {
                    navigate = Some(url);
                }
                // Process pending download actions from sidebar
                if let Some((dl_id, action)) = self.sidebar_state.pending_download_cmd.take() {
                    match action {
                        DownloadAction::Pause => self.download_manager.pause_download(dl_id),
                        DownloadAction::Resume => self.download_manager.resume_download(dl_id, &self.download_dir),
                        DownloadAction::Remove => self.download_manager.remove_download(dl_id),
                    }
                }
                if self.config.ui.theme != old_theme { self.refresh_theme(ctx); }

                // Main content area
                let active_url = self.active_state()
                    .and_then(TabPageState::current_url)
                    .map(str::to_owned)
                    .or_else(|| self.tabs.active_tab().map(|tab| tab.url.to_string()))
                    .unwrap_or_else(|| "fortrust://start".to_owned());

                // Loading progress bar
                let is_loading = self.active_tab_id().is_some_and(|id|
                    self.tab_pages.get(&id).and_then(|s| s.loading_url.as_deref()).is_some_and(|u| u != active_url)
                );
                if is_loading {
                    let bar_rect = Rect::from_min_size(
                        Pos2::new(ui.max_rect().min.x, ui.max_rect().min.y),
                        Vec2::new(ui.max_rect().width(), 3.0),
                    );
                    let progress = (self.animation_phase * 3.0).sin() * 0.5 + 0.5;
                    let fill_w = bar_rect.width() * (0.15 + progress * 0.55);
                    let fill_x = bar_rect.min.x + (bar_rect.width() - fill_w) * progress;
                    ui.painter().rect_filled(bar_rect, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(79, 158, 255, 40));
                    ui.painter().rect_filled(
                        Rect::from_min_size(Pos2::new(fill_x, bar_rect.min.y), Vec2::new(fill_w, 3.0)),
                        CornerRadius::ZERO, self.theme.accent_primary,
                    );
                }

                if active_url == "fortrust://start" {
                    self.speed_dial.ads_blocked = self.shield.ads_blocked;
                    self.speed_dial.trackers_blocked = self.shield.trackers_blocked;
                    self.speed_dial.tab_count = self.tabs.tabs().len();
                    self.speed_dial.blocked_requests = self.shield.ads_blocked as u64 + self.shield.trackers_blocked as u64;
                    self.speed_dial.doh_enabled = self.config.privacy.https_only_mode;
                    self.speed_dial.fingerprinting_protection = self.config.privacy.fingerprint_noise;
                    let dt = self.last_frame.elapsed().as_secs_f32().min(0.05) * self.motion_scale();
                    if let Some(url) = self.speed_dial.render(ui, &self.theme, dt, &mut self.needs_new_tab, self.storage.as_ref()) {
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
            ui.add_space(80.0);
            let icon_rect = Rect::from_min_size(
                Pos2::new(ui.max_rect().center().x - 20.0, ui.cursor().min.y),
                Vec2::new(40.0, 40.0),
            );
            icons::paint_shield_icon_rect(ui.painter(), icon_rect, self.theme.accent_shield_off);
            ui.add_space(50.0);
            ui.label(
                egui::RichText::new("Request Blocked")
                    .size(30.0).strong()
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
        let Some(tab_id) = self.active_tab_id() else { return; };

        let is_loading = self.tab_pages.get(&tab_id)
            .map(|s| s.loading_url.as_deref() == Some(url))
            .unwrap_or(false);
        if is_loading { self.render_loading(ui, url); return; }

        let load_error = self.tab_pages.get(&tab_id)
            .and_then(|s| s.load_error.as_deref().map(str::to_owned));
        if let Some(error) = load_error { self.render_error(ui, url, &error); return; }

        let page_url = self.tab_pages.get(&tab_id)
            .and_then(|s| s.page.as_ref().map(|p| p.url.clone()));
        if let Some(ref page_url_val) = page_url
            && page_url_val == url
            && let Some(page) = self.tab_pages.get(&tab_id).and_then(|s| s.page.as_ref())
        {
            self.paint_engine_page(ui, page);
            return;
        }

        if let Some(texture) = self.tab_pages.get(&tab_id).and_then(|s| s.renderer_frame.as_ref()) {
            let size = texture.size_vec2();
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            ui.painter().image(
                texture.id(),
                rect,
                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            return;
        }

        let viewport = Viewport {
            width: ui.available_width().max(320.0),
            height: ui.available_height().max(240.0),
        };
        if let Ok(page) = self.internal_engine.internal_page("fortrust://empty", viewport) {
            self.paint_engine_page(ui, &page);
        } else {
            self.render_error(ui, url, "no rendered page is available for this tab");
        }
    }

    fn render_loading(&mut self, ui: &mut egui::Ui, url: &str) {
        let rect = ui.max_rect();
        let center = Pos2::new(rect.center().x, rect.top() + rect.height() * 0.42);
        let phase = self.animation_phase;
        let radius = 26.0 + (phase.sin() * 0.5 + 0.5) * 10.0;
        ui.painter().circle_filled(center, radius + 14.0, Color32::from_rgba_unmultiplied(79, 158, 255, 32));
        ui.painter().circle_filled(center, radius, self.theme.accent_primary);
        ui.painter().text(
            Pos2::new(center.x, center.y + 64.0), egui::Align2::CENTER_CENTER,
            "Fortrust is loading", egui::FontId::proportional(28.0), self.theme.text_primary,
        );
        ui.painter().text(
            Pos2::new(center.x, center.y + 94.0), egui::Align2::CENTER_CENTER,
            url, egui::FontId::proportional(13.0), self.theme.text_secondary,
        );
    }

    fn render_error(&mut self, ui: &mut egui::Ui, url: &str, error: &str) {
        ui.vertical_centered(|ui| {
            ui.add_space(64.0);
            let icon_cx = ui.max_rect().center().x;
            let icon_cy = ui.cursor().min.y + 18.0;
            ui.painter().circle_stroke(Pos2::new(icon_cx, icon_cy), 16.0, Stroke::new(2.5, self.theme.accent_shield_off));
            ui.painter().line_segment(
                [Pos2::new(icon_cx - 10.0, icon_cy - 10.0), Pos2::new(icon_cx + 10.0, icon_cy + 10.0)],
                Stroke::new(2.5, self.theme.accent_shield_off),
            );
            ui.add_space(46.0);
            ui.label(egui::RichText::new("Load Error").size(28.0).strong().color(self.theme.accent_shield_off));
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
                    painter.rect_filled(to_egui_rect(content.min, *rect), 0.0, to_egui_color(*color));
                }
                DisplayCommand::DrawText { rect, text, color, font_size_px, .. } => {
                    painter.text(
                        Pos2::new(content.min.x + rect.x, content.min.y + rect.y),
                        egui::Align2::LEFT_TOP, text,
                        egui::FontId::proportional(*font_size_px),
                        to_egui_color(*color),
                    );
                }
                DisplayCommand::DrawBorder {
                    rect,
                    top_width,
                    right_width,
                    bottom_width,
                    left_width,
                    top_color,
                    right_color,
                    bottom_color,
                    left_color,
                    top_style: _,
                    right_style: _,
                    bottom_style: _,
                    left_style: _,
                } => {
                    paint_border(content.min, &painter, *rect, *top_width, *right_width, *bottom_width, *left_width, *top_color, *right_color, *bottom_color, *left_color);
                }
                DisplayCommand::DrawBoxShadow {
                    rect, offset_x, offset_y, blur, spread, color, inset: _,
                } => {
                    if color.a > 0 {
                        let shadow_rect = Rect::from_min_size(
                            Pos2::new(content.min.x + rect.x + offset_x - spread, content.min.y + rect.y + offset_y - spread),
                            Vec2::new(rect.width + spread * 2.0, rect.height + spread * 2.0),
                        );
                        let alpha = (color.a as f32 / 255.0 * 0.4 * (1.0 - (blur / 20.0).min(0.8))) as u8;
                        let shadow_color = Color32::from_rgba_unmultiplied(color.r, color.g, color.b, alpha);
                        painter.rect_filled(shadow_rect, 6.0, shadow_color);
                    }
                }
                DisplayCommand::DrawOutline {
                    rect, width, color, style: _,
                } => {
                    if width > &0.0 && color.a > 0 {
                        let r = Rect::from_min_size(
                            Pos2::new(content.min.x + rect.x, content.min.y + rect.y),
                            Vec2::new(rect.width, rect.height),
                        );
                        painter.rect_stroke(r, 0.0, egui::Stroke::new(*width, to_egui_color(*color)), egui::StrokeKind::Outside);
                    }
                }
                DisplayCommand::ClipPush(_) | DisplayCommand::ClipPop => {}
            }
        }
    }

    fn render_page_info(&mut self, ctx: &Context) {
        let url = self.active_state().and_then(|s| s.current_url())
            .or_else(|| self.tabs.active_tab().map(|t| t.url.as_str()))
            .unwrap_or("fortrust://start")
            .to_owned();
        let is_secure = url.starts_with("https://");
        let cookie_count = self.storage.as_ref().map(|s| s.cookies.count()).unwrap_or(0);

        let mut open = self.page_info_open;
        egui::Window::new("Page Info")
            .id(egui::Id::new("page_info_panel"))
            .anchor(egui::Align2::RIGHT_TOP, [-20.0, 80.0])
            .resizable(false)
            .default_width(320.0)
            .collapsible(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("Connection").strong().size(13.0));
                if is_secure {
                    ui.colored_label(Color32::from_rgb(40, 200, 64), "HTTPS - Secure connection");
                } else {
                    ui.colored_label(Color32::from_rgb(255, 160, 60), "HTTP - Not secure");
                }
                ui.add_space(8.0);
                ui.label(egui::RichText::new("URL").strong().size(13.0));
                ui.label(egui::RichText::new(&url).size(11.5).color(self.theme.text_secondary));
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Privacy").strong().size(13.0));
                ui.label(format!("Cookies: {cookie_count}"));
                ui.label(format!("Ads blocked: {}", self.shield.ads_blocked));
                ui.label(format!("Trackers blocked: {}", self.shield.trackers_blocked));
                if let Some(tab_state) = self.active_state().and_then(|s| s.page.as_ref()) {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Page Size").strong().size(13.0));
                    let size_kb = tab_state.security.body_bytes as f64 / 1024.0;
                    ui.label(format!("{size_kb:.1} KB"));
                    ui.label(format!("Display commands: {}", tab_state.security.display_commands));
                }
            });
        self.page_info_open = open;
    }
}

impl eframe::App for FortrustApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.05);
        self.last_frame = std::time::Instant::now();
        let motion_scale = self.motion_scale();
        self.animation_phase = ctx.input(|i| i.time as f32 * (1.15 + motion_scale * 0.45));

        // Keyboard shortcuts
        ctx.input_mut(|i| {
            let ctrl = i.modifiers.ctrl;
            let shift = i.modifiers.shift;
            if ctrl && i.key_pressed(egui::Key::T) {
                self.open_new_tab();
            } else if ctrl && i.key_pressed(egui::Key::W) {
                if let Some(id) = self.active_tab_id() {
                    self.close_tab(id);
                }
            } else if ctrl && shift && i.key_pressed(egui::Key::Tab) {
                self.cycle_tab(false);
            } else if ctrl && i.key_pressed(egui::Key::Tab) {
                self.cycle_tab(true);
            } else if ctrl && i.key_pressed(egui::Key::L) {
                self.omnibox.focused = true;
                self.omnibox.text.clear();
            } else if ctrl && i.key_pressed(egui::Key::R) {
                self.reload();
            } else if i.key_pressed(egui::Key::I) && ctrl {
                self.page_info_open = !self.page_info_open;
            } else if i.key_pressed(egui::Key::Escape) {
                if self.sidebar_anim.is_open() {
                    self.sidebar_anim.close();
                } else {
                    self.omnibox.focused = false;
                }
            }
            false
        });

        // Tick sidebar animation
        self.sidebar_anim.tick(dt * motion_scale);
        if !self.sidebar_anim.overlay_offset.is_settled() {
            ctx.request_repaint();
        }

        // Poll loading engine
        self.poll_loading(ctx);

        // Save download state periodically
        self.save_download_state();

        // Clear startup overlay after deadline
        if let Some(dl) = self.startup_deadline {
            if std::time::Instant::now() >= dl {
                self.startup_deadline = None;
            } else {
                ctx.request_repaint_after(dl - std::time::Instant::now());
            }
        }

        // Apply egui style
        apply_egui_style(ctx, &self.theme, &self.config.ui);

        // Render shield popup
        if self.config.ui.show_privacy_panel {
            self.shield.render_popup(ctx, &self.theme);
        }

        // Background wallpaper
        let screen_rect = ctx.content_rect();
        backgrounds::paint_background(ctx, screen_rect, &self.theme, &self.config.ui);

        // Render the layout
        self.render_tab_bar(ctx);
        self.render_address_bar(ctx);
        self.render_central_panel(ctx);

        // Page info panel
        if self.page_info_open {
            self.render_page_info(ctx);
        }

        // Keep repainting for animations
        let frame_delay = (33.0 / motion_scale.max(0.5)).round().clamp(16.0, 40.0) as u64;
        ctx.request_repaint_after(Duration::from_millis(frame_delay));

        // Startup overlay
        if self.startup_deadline.is_some() {
            egui::Area::new("startup_overlay".into())
                .order(egui::Order::Foreground)
                .fixed_pos(Pos2::new(24.0, 24.0))
                .interactable(false)
                .show(ctx, |ui| {
                    Frame {
                        fill: self.theme.glass_bg,
                        inner_margin: Margin::symmetric(18, 14),
                        corner_radius: CornerRadius::same(16),
                        stroke: Stroke::new(1.0, self.theme.glass_border),
                        shadow: egui::epaint::Shadow {
                            offset: [0, 8], blur: 24, spread: 0,
                            color: Color32::from_black_alpha(50),
                        },
                        ..Default::default()
                    }.show(ui, |ui| {
                        ui.label(egui::RichText::new("Fortrust starting").size(18.0).strong().color(self.theme.text_primary));
                        ui.label(egui::RichText::new("Private browser shell is loading.").size(12.0).color(self.theme.text_secondary));
                    });
                });
        }
    }

}

fn frame_bytes_to_rgba(texture_data: &[u8], width: u32, height: u32, stride: u32) -> Option<Vec<u8>> {
    let width = width as usize;
    let height = height as usize;
    let stride = stride as usize;
    let expected = width.checked_mul(height)?.checked_mul(4)?;

    if stride == width.checked_mul(4)? && texture_data.len() == expected {
        return Some(texture_data.to_vec());
    }

    if stride < width.checked_mul(4)? {
        return None;
    }

    let mut rgba = vec![0u8; expected];
    for row in 0..height {
        let src_start = row.checked_mul(stride)?;
        let src_end = src_start.checked_add(width.checked_mul(4)?)?;
        let dst_start = row.checked_mul(width.checked_mul(4)?)?;
        let dst_end = dst_start.checked_add(width.checked_mul(4)?)?;
        if src_end > texture_data.len() || dst_end > rgba.len() {
            return None;
        }
        rgba[dst_start..dst_end].copy_from_slice(&texture_data[src_start..src_end]);
    }

    Some(rgba)
}

fn apply_egui_style(ctx: &egui::Context, theme: &FortrustTheme, ui_config: &fortrust_core::UiConfig) {
    let mut style = (*ctx.style()).clone();
    style.visuals.panel_fill = Color32::TRANSPARENT;
    style.visuals.window_fill = theme.glass_bg;
    style.visuals.window_stroke = Stroke::new(1.0, theme.glass_border);
    let density = if ui_config.compact_density { 0.88 } else { 1.08 };
    style.spacing.item_spacing = Vec2::new(8.0 * density, 4.0 * density);
    style.spacing.button_padding = Vec2::new(8.0 * density, 5.0 * density);
    style.spacing.interact_size = Vec2::new(40.0 * density, 24.0 * density);
    ctx.set_style(style);
}

fn to_egui_rect(origin: Pos2, rect: EngineRect) -> Rect {
    Rect::from_min_size(Pos2::new(origin.x + rect.x, origin.y + rect.y), Vec2::new(rect.width, rect.height))
}

fn to_egui_color(color: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

#[allow(clippy::too_many_arguments)]
fn paint_border(
    origin: Pos2,
    painter: &egui::Painter,
    rect: EngineRect,
    top_width: f32,
    right_width: f32,
    bottom_width: f32,
    left_width: f32,
    top_color: Color,
    right_color: Color,
    bottom_color: Color,
    left_color: Color,
) {
    let r = Rect::from_min_size(Pos2::new(origin.x + rect.x, origin.y + rect.y), Vec2::new(rect.width, rect.height));
    if top_width > 0.0 && top_color.a > 0 {
        painter.rect_filled(Rect::from_min_size(r.left_top(), Vec2::new(r.width(), top_width)), 0.0, to_egui_color(top_color));
    }
    if bottom_width > 0.0 && bottom_color.a > 0 {
        painter.rect_filled(Rect::from_min_size(Pos2::new(r.left(), r.bottom() - bottom_width), Vec2::new(r.width(), bottom_width)), 0.0, to_egui_color(bottom_color));
    }
    if left_width > 0.0 && left_color.a > 0 {
        painter.rect_filled(Rect::from_min_size(r.left_top(), Vec2::new(left_width, r.height())), 0.0, to_egui_color(left_color));
    }
    if right_width > 0.0 && right_color.a > 0 {
        painter.rect_filled(Rect::from_min_size(Pos2::new(r.right() - right_width, r.top()), Vec2::new(right_width, r.height())), 0.0, to_egui_color(right_color));
    }
}

fn normalize_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("fortrust://") || trimmed.starts_with("about:")
        || trimmed.starts_with("http://") || trimmed.starts_with("https://")
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
    if input.is_empty() || input.contains(' ') { return false; }
    if input.starts_with('[') && input.contains(']') { return true; }
    if input.eq_ignore_ascii_case("localhost") || input.starts_with("localhost:") { return true; }
    input.contains('.')
}

fn title_from_url(url: &str) -> String {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .trim_end_matches('/')
        .chars().take(24).collect()
}
