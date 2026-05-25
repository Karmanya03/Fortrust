use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct KeyEvent {
    pub key: String,
    pub code: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct MouseEvent {
    pub x: f64,
    pub y: f64,
    pub button: u8,
    pub buttons: u8,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum PrivacyEvent {
    AdBlocked { url: String },
    TrackerBlocked { url: String },
    HttpsUpgraded { url: String, effective_url: String },
    FingerprintAttemptBlocked { api: String },
    ThirdPartyCookieBlocked { url: String, cookie_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum LoadState {
    Loading,
    Parsing,
    Layout,
    Painting,
    Loaded,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum BrowserToRenderer {
    Navigate {
        url: String,
    },
    GoBack,
    GoForward,
    Reload,
    Stop,
    ExecuteScript {
        js: String,
    },
    KeyEvent {
        event: KeyEvent,
    },
    MouseEvent {
        event: MouseEvent,
    },
    Resize {
        width: u32,
        height: u32,
    },
    ZoomChange {
        factor: f32,
    },
    ScrollTo {
        x: f64,
        y: f64,
    },
    SetPrivacySettings {
        block_ads: bool,
        block_trackers: bool,
        https_only: bool,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum RendererToBrowser {
    TitleChanged {
        title: String,
    },
    UrlChanged {
        url: String,
    },
    FaviconUpdated {
        data: Vec<u8>,
    },
    LoadProgress {
        percent: f32,
        state: LoadState,
    },
    LoadComplete,
    FrameReady {
        texture_data: Vec<u8>,
        width: u32,
        height: u32,
        stride: u32,
    },
    PrivacyEvent {
        event: PrivacyEvent,
    },
    Alert {
        message: String,
    },
    ConsoleMessage {
        level: String,
        message: String,
    },
    NewTabRequested {
        url: String,
    },
    DownloadRequested {
        url: String,
        filename: String,
        mime_type: String,
    },
    NavigationStart {
        url: String,
    },
    NavigationError {
        url: String,
        error: String,
    },
    DocumentTitleChanged {
        title: String,
    },
    ScrollPosition {
        x: f64,
        y: f64,
    },
    RendererCrashed {
        reason: String,
    },
    MemoryUsage {
        used_mb: u32,
        heap_mb: u32,
    },
    ShutdownAck,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum NetProcessCommand {
    FetchUrl {
        request_id: u64,
        url: String,
        headers: Vec<(String, String)>,
        method: String,
        resource_type: String,
        top_level_url: Option<String>,
    },
    FetchStream {
        request_id: u64,
        url: String,
        headers: Vec<(String, String)>,
    },
    CancelRequest {
        request_id: u64,
    },
    SetDohProvider {
        provider: String,
    },
    ClearCache,
    PrefetchUrls {
        urls: Vec<String>,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum NetProcessEvent {
    ResponseHeaders {
        request_id: u64,
        status: u16,
        headers: Vec<(String, String)>,
    },
    ResponseBody {
        request_id: u64,
        chunk: Vec<u8>,
        last: bool,
    },
    RequestComplete {
        request_id: u64,
        status: u16,
        total_bytes: u64,
        source: String,
    },
    RequestFailed {
        request_id: u64,
        error: String,
    },
    RequestBlocked {
        request_id: u64,
        reason: String,
        original_url: String,
    },
    CacheHit {
        request_id: u64,
        cached_bytes: u64,
    },
    ShutdownAck,
}
