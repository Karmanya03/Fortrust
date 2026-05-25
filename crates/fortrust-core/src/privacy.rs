use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;

use ahash::AHashSet;
use compact_str::CompactString;
use smallvec::SmallVec;
use url::Url;

use crate::config::PrivacyConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Document,
    Script,
    Image,
    Stylesheet,
    Xhr,
    Media,
    Font,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub url: String,
    pub top_level_url: Option<String>,
    pub resource_type: ResourceType,
}

impl RequestContext {
    pub fn document(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            top_level_url: None,
            resource_type: ResourceType::Document,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    InvalidUrl,
    UnsupportedScheme,
    TrackerDomain,
    AdDomain,
    MixedContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyNote {
    HttpsUpgraded,
    TrackingQueryStripped,
    ThirdPartyCookieBlocked,
    GlobalPrivacyControl,
    DoNotTrack,
    FingerprintNoiseEnabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestDecision {
    pub original_url: String,
    pub effective_url: Option<String>,
    pub blocked: Option<BlockReason>,
    pub third_party: bool,
    pub stripped_query_pairs: usize,
    pub notes: SmallVec<[PrivacyNote; 6]>,
}

impl RequestDecision {
    pub fn is_allowed(&self) -> bool {
        self.blocked.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct PrivacyEngine {
    settings: PrivacyConfig,
    tracker_hosts: AHashSet<CompactString>,
    ad_hosts: AHashSet<CompactString>,
    tracking_params: AHashSet<CompactString>,
}

impl PrivacyEngine {
    pub fn new(settings: PrivacyConfig) -> Self {
        Self {
            settings,
            tracker_hosts: built_in_tracker_hosts(),
            ad_hosts: built_in_ad_hosts(),
            tracking_params: built_in_tracking_params(),
        }
    }

    pub fn inspect(&self, context: &RequestContext) -> RequestDecision {
        let original_url = context.url.clone();
        let Ok(mut parsed) = Url::parse(&context.url) else {
            return RequestDecision {
                original_url,
                effective_url: None,
                blocked: Some(BlockReason::InvalidUrl),
                third_party: false,
                stripped_query_pairs: 0,
                notes: SmallVec::new(),
            };
        };

        let mut notes = SmallVec::new();
        let scheme = parsed.scheme();
        if !matches!(scheme, "http" | "https" | "fortrust" | "about") {
            return RequestDecision {
                original_url,
                effective_url: None,
                blocked: Some(BlockReason::UnsupportedScheme),
                third_party: false,
                stripped_query_pairs: 0,
                notes,
            };
        }

        if context.resource_type != ResourceType::Document
            && scheme == "http"
            && context
                .top_level_url
                .as_deref()
                .is_some_and(|top| top.starts_with("https://"))
        {
            return RequestDecision {
                original_url,
                effective_url: None,
                blocked: Some(BlockReason::MixedContent),
                third_party: false,
                stripped_query_pairs: 0,
                notes,
            };
        }

        if self.settings.https_only_mode && scheme == "http" {
            let _ = parsed.set_scheme("https");
            notes.push(PrivacyNote::HttpsUpgraded);
        }

        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if self.settings.block_trackers && host_matches(&self.tracker_hosts, &host) {
            return RequestDecision {
                original_url,
                effective_url: None,
                blocked: Some(BlockReason::TrackerDomain),
                third_party: false,
                stripped_query_pairs: 0,
                notes,
            };
        }

        if self.settings.block_ads && host_matches(&self.ad_hosts, &host) {
            return RequestDecision {
                original_url,
                effective_url: None,
                blocked: Some(BlockReason::AdDomain),
                third_party: false,
                stripped_query_pairs: 0,
                notes,
            };
        }

        let third_party = self.is_third_party(&parsed, context.top_level_url.as_deref());
        if self.settings.block_third_party_cookies && third_party {
            notes.push(PrivacyNote::ThirdPartyCookieBlocked);
        }

        let stripped_query_pairs = if self.settings.strip_tracking_query_params {
            strip_tracking_query_params(&mut parsed, &self.tracking_params)
        } else {
            0
        };
        if stripped_query_pairs > 0 {
            notes.push(PrivacyNote::TrackingQueryStripped);
        }

        if self.settings.global_privacy_control {
            notes.push(PrivacyNote::GlobalPrivacyControl);
        }
        if self.settings.do_not_track {
            notes.push(PrivacyNote::DoNotTrack);
        }
        if self.settings.fingerprint_noise {
            notes.push(PrivacyNote::FingerprintNoiseEnabled);
        }

        RequestDecision {
            original_url,
            effective_url: Some(parsed.to_string()),
            blocked: None,
            third_party,
            stripped_query_pairs,
            notes,
        }
    }

    pub fn fingerprint_noise_seed(&self, origin: &str, epoch_days: u64) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.settings.per_profile_fingerprint_salt.hash(&mut hasher);
        origin.hash(&mut hasher);
        epoch_days.hash(&mut hasher);
        hasher.finish()
    }

    fn is_third_party(&self, url: &Url, top_level_url: Option<&str>) -> bool {
        let Some(top_level_url) = top_level_url else {
            return false;
        };
        let Ok(top) = Url::parse(top_level_url) else {
            return false;
        };

        let current = url.host_str().map(site_key);
        let top = top.host_str().map(site_key);
        current.is_some() && top.is_some() && current != top
    }
}

fn strip_tracking_query_params(url: &mut Url, tracking_params: &AHashSet<CompactString>) -> usize {
    let pairs = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    if pairs.is_empty() {
        return 0;
    }

    let original_len = pairs.len();
    let kept = pairs
        .into_iter()
        .filter(|(key, _)| !is_tracking_param(key, tracking_params))
        .collect::<Vec<_>>();
    let stripped = original_len.saturating_sub(kept.len());

    if stripped > 0 {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }

    stripped
}

fn is_tracking_param(key: &str, tracking_params: &AHashSet<CompactString>) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.starts_with("utm_") || tracking_params.contains(normalized.as_str())
}

fn host_matches(hosts: &AHashSet<CompactString>, host: &str) -> bool {
    hosts.iter().any(|blocked| {
        let blocked = blocked.as_str();
        host == blocked
            || host
                .strip_suffix(blocked)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

fn site_key(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.parse::<IpAddr>().is_ok() {
        return host;
    }

    let mut labels = host.rsplit('.').take(2).collect::<Vec<_>>();
    labels.reverse();
    labels.join(".")
}

fn built_in_tracker_hosts() -> AHashSet<CompactString> {
    [
        "doubleclick.net",
        "google-analytics.com",
        "googletagmanager.com",
        "facebook.net",
        "scorecardresearch.com",
        "hotjar.com",
        "segment.io",
        "mixpanel.com",
    ]
    .into_iter()
    .map(CompactString::from)
    .collect()
}

fn built_in_ad_hosts() -> AHashSet<CompactString> {
    [
        "adsystem.com",
        "adservice.google.com",
        "taboola.com",
        "outbrain.com",
        "zedo.com",
        "adnxs.com",
    ]
    .into_iter()
    .map(CompactString::from)
    .collect()
}

fn built_in_tracking_params() -> AHashSet<CompactString> {
    [
        "fbclid", "gclid", "dclid", "msclkid", "mc_cid", "mc_eid", "igshid", "vero_id", "_hsenc",
        "_hsmi",
    ]
    .into_iter()
    .map(CompactString::from)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrades_http_and_strips_tracking_params() {
        let engine = PrivacyEngine::new(PrivacyConfig::default());
        let decision = engine.inspect(&RequestContext::document(
            "http://example.com/?utm_source=news&keep=1&fbclid=abc",
        ));

        assert!(decision.is_allowed());
        assert_eq!(
            decision.effective_url.as_deref(),
            Some("https://example.com/?keep=1")
        );
        assert_eq!(decision.stripped_query_pairs, 2);
        assert!(decision.notes.contains(&PrivacyNote::HttpsUpgraded));
        assert!(decision.notes.contains(&PrivacyNote::TrackingQueryStripped));
    }

    #[test]
    fn blocks_known_tracker_hosts() {
        let engine = PrivacyEngine::new(PrivacyConfig::default());
        let decision = engine.inspect(&RequestContext {
            url: "https://stats.google-analytics.com/collect".to_owned(),
            top_level_url: Some("https://example.com".to_owned()),
            resource_type: ResourceType::Script,
        });

        assert_eq!(decision.blocked, Some(BlockReason::TrackerDomain));
    }

    #[test]
    fn marks_third_party_cookie_policy() {
        let engine = PrivacyEngine::new(PrivacyConfig::default());
        let decision = engine.inspect(&RequestContext {
            url: "https://cdn.example.net/app.js".to_owned(),
            top_level_url: Some("https://example.com".to_owned()),
            resource_type: ResourceType::Script,
        });

        assert!(decision.third_party);
        assert!(
            decision
                .notes
                .contains(&PrivacyNote::ThirdPartyCookieBlocked)
        );
    }

    #[test]
    fn local_ip_site_keys_stay_exact() {
        let engine = PrivacyEngine::new(PrivacyConfig::default());
        let decision = engine.inspect(&RequestContext {
            url: "http://127.0.0.1:3000/app.js".to_owned(),
            top_level_url: Some("http://127.0.0.1:3000".to_owned()),
            resource_type: ResourceType::Script,
        });

        assert!(!decision.third_party);
    }
}
