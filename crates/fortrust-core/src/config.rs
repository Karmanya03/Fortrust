use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserConfig {
    pub privacy: PrivacyConfig,
    pub performance: PerformanceConfig,
    pub ui: UiConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub block_ads: bool,
    pub block_trackers: bool,
    pub block_third_party_cookies: bool,
    pub strip_tracking_query_params: bool,
    pub https_only_mode: bool,
    pub global_privacy_control: bool,
    pub do_not_track: bool,
    pub fingerprint_noise: bool,
    pub per_profile_fingerprint_salt: u64,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            block_ads: true,
            block_trackers: true,
            block_third_party_cookies: true,
            strip_tracking_query_params: true,
            https_only_mode: true,
            global_privacy_control: true,
            do_not_track: true,
            fingerprint_noise: true,
            per_profile_fingerprint_salt: 0x464f_5254_5255_5354,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub max_active_renderer_mb: u16,
    pub max_warm_renderer_mb: u16,
    pub max_total_tab_ram_mb: u32,
    pub warm_tab_limit: usize,
    pub suspend_background_after_ticks: u64,
    pub suspended_snapshot_kb: u16,
    pub lazy_renderer_start: bool,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            max_active_renderer_mb: 96,
            max_warm_renderer_mb: 48,
            max_total_tab_ram_mb: 384,
            warm_tab_limit: 2,
            suspend_background_after_ticks: 4,
            suspended_snapshot_kb: 384,
            lazy_renderer_start: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: String,
    pub compact_density: bool,
    pub show_privacy_panel: bool,
    pub show_memory_meter: bool,
        // Wallpaper choice: "none", "watercolor", "forest"
        pub wallpaper: String,
        // Strength 0-100 for wallpaper visibility
        pub wallpaper_strength: u8,
    pub glass_strength: u8,
    pub motion_strength: u8,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            // Default to light Opera-like surface per user request
            // Image reference: centered light search with soft watercolor background
            theme: "light".to_owned(),
            compact_density: true,
            show_privacy_panel: true,
            show_memory_meter: true,
            wallpaper: "watercolor".to_owned(),
            wallpaper_strength: 84,
            glass_strength: 82,
            motion_strength: 70,
        }
    }
}
