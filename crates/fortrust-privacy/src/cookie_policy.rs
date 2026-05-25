use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CookiePolicy {
    AcceptAll,
    #[default]
    BlockThirdParty,
    BlockAll,
    RejectTrackers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SameSitePolicy {
    Strict,
    Lax,
    None,
}

impl CookiePolicy {
    pub fn allows_domain(&self, domain: &str, top_level_domain: &str) -> bool {
        match self {
            Self::AcceptAll => true,
            Self::BlockAll => false,
            Self::BlockThirdParty => {
                domain == top_level_domain
                    || domain.ends_with(&format!(".{top_level_domain}"))
            }
            Self::RejectTrackers => {
                let is_tracker = [
                    "doubleclick.net",
                    "google-analytics.com",
                    "facebook.net",
                    "scorecardresearch.com",
                    "hotjar.com",
                    "adsystem.com",
                ]
                .iter()
                .any(|tracker| domain.contains(tracker));
                !is_tracker
            }
        }
    }

    pub fn allows_third_party(&self) -> bool {
        matches!(self, Self::AcceptAll)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::AcceptAll => "Accept All",
            Self::BlockThirdParty => "Block Third-Party",
            Self::BlockAll => "Block All",
            Self::RejectTrackers => "Reject Trackers",
        }
    }
}
