pub mod config;
pub mod images;
pub mod privacy;
pub mod tabs;
pub mod workspaces;

pub use config::{BrowserConfig, PerformanceConfig, PrivacyConfig, UiConfig};
pub use images::{DecodedImage, ImageRegistry};
pub use privacy::{
    BlockReason, PrivacyEngine, PrivacyNote, PrivacyStats, ReferrerPolicy, RequestContext,
    RequestDecision, ResourceType, compute_referer,
};
pub use tabs::{MemoryReport, Tab, TabId, TabManager, TabStatus};
pub use workspaces::{Workspace, WorkspaceId, WorkspaceManager};

pub const BROWSER_NAME: &str = "Fortrust";
