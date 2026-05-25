pub mod config;
pub mod privacy;
pub mod tabs;

pub use config::{BrowserConfig, PerformanceConfig, PrivacyConfig, UiConfig};
pub use privacy::{
    BlockReason, PrivacyEngine, PrivacyNote, RequestContext, RequestDecision, ResourceType,
};
pub use tabs::{MemoryReport, Tab, TabId, TabManager, TabStatus};

pub const BROWSER_NAME: &str = "Fortrust";
