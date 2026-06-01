use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::Mutex;

use ahash::AHashSet;
use compact_str::CompactString;
use fortrust_privacy::{HttpsDecision, HttpsUpgrader};
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

/// Compute the value of the `Referer` header for a request under the given
/// policy. Returns `None` when the policy says not to send a Referer.
///
/// `request_url` is the URL the browser is navigating to. `top_level_url` is
/// the URL of the page that initiated the navigation (when known).
pub fn compute_referer(
    request_url: &str,
    top_level_url: Option<&str>,
    policy: ReferrerPolicy,
) -> Option<String> {
    match policy {
        ReferrerPolicy::NoReferrer => None,
        ReferrerPolicy::SameOrigin => {
            let top = top_level_url?;
            same_origin(request_url, top).then(|| top.to_owned())
        }
        ReferrerPolicy::StrictOrigin => {
            let top = top_level_url?;
            // Strict-Origin: only send the origin when the request URL is
            // HTTPS (or both are HTTP). Never send when downgrading to HTTP.
            let req = Url::parse(request_url).ok()?;
            let top_parsed = Url::parse(top).ok()?;
            if req.scheme() == "https" && top_parsed.scheme() == "https" && same_origin(request_url, top) {
                Some(origin_of(top_parsed))
            } else {
                None
            }
        }
        ReferrerPolicy::Origin => {
            let top = top_level_url?;
            let top_parsed = Url::parse(top).ok()?;
            Some(origin_of(top_parsed))
        }
        ReferrerPolicy::StrictOriginWhenCrossOrigin => {
            let top = top_level_url?;
            if same_origin(request_url, top) {
                Some(top.to_owned())
            } else {
                let top_parsed = Url::parse(top).ok()?;
                if top_parsed.scheme() == "https" {
                    Some(origin_of(top_parsed))
                } else {
                    None
                }
            }
        }
    }
}

fn same_origin(a: &str, b: &str) -> bool {
    let Ok(pa) = Url::parse(a) else { return false };
    let Ok(pb) = Url::parse(b) else { return false };
    pa.scheme() == pb.scheme() && pa.host_str() == pb.host_str() && pa.port() == pb.port()
}

fn origin_of(url: Url) -> String {
    let mut out = String::with_capacity(url.scheme().len() + url.host_str().map(str::len).unwrap_or(0) + 8);
    out.push_str(url.scheme());
    out.push_str("://");
    if let Some(h) = url.host_str() { out.push_str(h); }
    if let Some(p) = url.port() {
        out.push(':');
        out.push_str(&p.to_string());
    }
    out
}

#[cfg(test)]
mod referrer_tests {
    use super::*;

    #[test]
    fn no_referrer_never_sends() {
        assert_eq!(compute_referer("https://other.com/page", Some("https://example.com/"), ReferrerPolicy::NoReferrer), None);
    }

    #[test]
    fn same_origin_sends_for_same_origin() {
        assert_eq!(
            compute_referer("https://example.com/other", Some("https://example.com/here"), ReferrerPolicy::SameOrigin),
            Some("https://example.com/here".to_owned())
        );
    }

    #[test]
    fn same_origin_omits_for_cross_origin() {
        assert_eq!(
            compute_referer("https://other.com/x", Some("https://example.com/"), ReferrerPolicy::SameOrigin),
            None
        );
    }

    #[test]
    fn strict_origin_does_not_send_on_http_downgrade() {
        assert_eq!(
            compute_referer("http://example.com/x", Some("https://example.com/"), ReferrerPolicy::StrictOrigin),
            None
        );
    }

    #[test]
    fn strict_origin_sends_origin_on_https_to_https() {
        assert_eq!(
            compute_referer("https://example.com/x", Some("https://example.com/y"), ReferrerPolicy::StrictOrigin),
            Some("https://example.com".to_owned())
        );
    }

    #[test]
    fn origin_always_sends_origin_even_cross_origin() {
        assert_eq!(
            compute_referer("https://other.com/x", Some("https://example.com/"), ReferrerPolicy::Origin),
            Some("https://example.com".to_owned())
        );
    }

    #[test]
    fn strict_origin_when_cross_origin_sends_origin_for_https() {
        assert_eq!(
            compute_referer("https://other.com/x", Some("https://example.com/"), ReferrerPolicy::StrictOriginWhenCrossOrigin),
            Some("https://example.com".to_owned())
        );
    }

    #[test]
    fn strict_origin_when_cross_origin_omits_for_http() {
        assert_eq!(
            compute_referer("https://other.com/x", Some("http://example.com/"), ReferrerPolicy::StrictOriginWhenCrossOrigin),
            None
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReferrerPolicy {
    /// Never send a `Referer` header at all. Most privacy-preserving default.
    #[default]
    NoReferrer,
    /// Send `Referer` only when the request is same-origin with the top-level
    /// site. Cross-origin requests get no Referer.
    SameOrigin,
    /// Send `Referer` only on HTTPS→HTTPS downgrades within the same origin.
    /// Never send for cross-origin or HTTP→HTTPS upgrades.
    StrictOrigin,
    /// Send only the origin (scheme + host + port), never the full URL.
    Origin,
    /// Send the full URL when same-origin, only the origin when cross-origin.
    StrictOriginWhenCrossOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub url: String,
    pub top_level_url: Option<String>,
    pub resource_type: ResourceType,
    /// Which referrer policy to apply. `None` (the default) means use the
    /// browser's default policy (currently `NoReferrer` for privacy).
    pub referrer_policy: Option<ReferrerPolicy>,
}

impl RequestContext {
    pub fn new(url: impl Into<String>, resource_type: ResourceType) -> Self {
        Self {
            url: url.into(),
            top_level_url: None,
            resource_type,
            referrer_policy: None,
        }
    }

    pub fn document(url: impl Into<String>) -> Self {
        Self::new(url, ResourceType::Document)
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self {
            url: String::new(),
            top_level_url: None,
            resource_type: ResourceType::Other,
            referrer_policy: None,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrivacyStats {
    pub ads_blocked: u64,
    pub trackers_blocked: u64,
    pub https_upgrades: u64,
    pub fingerprint_attempts_blocked: u64,
    pub third_party_cookies_blocked: u64,
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

pub struct PrivacyEngine {
    settings: PrivacyConfig,
    tracker_hosts: AHashSet<CompactString>,
    ad_hosts: AHashSet<CompactString>,
    tracking_params: AHashSet<CompactString>,
    https_upgrader: HttpsUpgrader,
    stats: Mutex<PrivacyStats>,
}

impl std::fmt::Debug for PrivacyEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivacyEngine")
            .field("settings", &self.settings)
            .field("tracker_hosts", &self.tracker_hosts)
            .field("ad_hosts", &self.ad_hosts)
            .field("tracking_params", &self.tracking_params)
            .field("https_upgrader", &self.https_upgrader)
            .field("stats", &self.stats.lock().unwrap())
            .finish()
    }
}

impl Clone for PrivacyEngine {
    fn clone(&self) -> Self {
        Self {
            settings: self.settings.clone(),
            tracker_hosts: self.tracker_hosts.clone(),
            ad_hosts: self.ad_hosts.clone(),
            tracking_params: self.tracking_params.clone(),
            https_upgrader: self.https_upgrader.clone(),
            stats: Mutex::new(self.stats.lock().unwrap().clone()),
        }
    }
}

impl PrivacyEngine {
    pub fn new(settings: PrivacyConfig) -> Self {
        Self {
            settings,
            tracker_hosts: built_in_tracker_hosts(),
            ad_hosts: built_in_ad_hosts(),
            tracking_params: built_in_tracking_params(),
            https_upgrader: HttpsUpgrader::new(),
            stats: Mutex::new(PrivacyStats::default()),
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

        // HTTPS upgrade via HttpsUpgrader
        if self.settings.https_only_mode && scheme == "http" {
            match self.https_upgrader.evaluate(&original_url) {
                HttpsDecision::Upgraded(url) => {
                    self.stats.lock().unwrap().https_upgrades += 1;
                    notes.push(PrivacyNote::HttpsUpgraded);
                    parsed = Url::parse(&url).unwrap_or(parsed);
                }
                HttpsDecision::Blocked => {
                    return RequestDecision {
                        original_url,
                        effective_url: None,
                        blocked: Some(BlockReason::MixedContent),
                        third_party: false,
                        stripped_query_pairs: 0,
                        notes,
                    };
                }
                _ => {}
            }
        }

        // Host-based blocking (fast path, preserves categorization)
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if self.settings.block_trackers && host_matches(&self.tracker_hosts, &host) {
            self.stats.lock().unwrap().trackers_blocked += 1;
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
            self.stats.lock().unwrap().ads_blocked += 1;
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
            self.stats.lock().unwrap().third_party_cookies_blocked += 1;
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
            self.stats.lock().unwrap().fingerprint_attempts_blocked += 1;
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

    pub fn stats(&self) -> PrivacyStats {
        self.stats.lock().unwrap().clone()
    }

    /// Returns GPC and DNT header enrichments for outgoing HTTP requests.
    pub fn header_enrichments(&self) -> Vec<(&'static str, &'static str)> {
        let mut headers = Vec::new();
        if self.settings.global_privacy_control {
            headers.push(("Sec-GPC", "1"));
        }
        if self.settings.do_not_track {
            headers.push(("DNT", "1"));
        }
        headers
    }

    pub fn reset_stats(&self) {
        *self.stats.lock().unwrap() = PrivacyStats::default();
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        });

        assert!(!decision.third_party);
    }

    #[test]
    fn blocks_http_subresource_from_https_top_level() {
        let engine = PrivacyEngine::new(PrivacyConfig::default());
        let cases = [
            (ResourceType::Image, "http://example.com/pixel.png"),
            (ResourceType::Stylesheet, "http://example.com/style.css"),
            (ResourceType::Script, "http://example.com/app.js"),
            (ResourceType::Font, "http://example.com/font.woff2"),
            (ResourceType::Media, "http://example.com/video.mp4"),
        ];
        for (resource_type, url) in cases {
            let decision = engine.inspect(&RequestContext {
                url: url.to_owned(),
                top_level_url: Some("https://example.com".to_owned()),
                resource_type,
                ..Default::default()
            });
            assert_eq!(
                decision.blocked,
                Some(BlockReason::MixedContent),
                "expected HTTP subresource {url} ({resource_type:?}) to be blocked under HTTPS top-level"
            );
        }
    }

    #[test]
    fn allows_http_subresource_from_http_top_level() {
        let engine = PrivacyEngine::new(PrivacyConfig::default());
        let decision = engine.inspect(&RequestContext {
            url: "http://example.com/style.css".to_owned(),
            top_level_url: Some("http://example.com".to_owned()),
            resource_type: ResourceType::Stylesheet,
            ..Default::default()
        });
        // Not mixed content (top-level is also http). May be blocked for other reasons
        // (e.g. https-only upgrade), but never for MixedContent.
        if let Some(reason) = decision.blocked {
            assert_ne!(reason, BlockReason::MixedContent);
        }
    }

    #[test]
    fn allows_top_level_navigation_to_http() {
        // User explicitly navigates to an http:// URL — allowed (the page is
        // responsible for its own subresources).
        let engine = PrivacyEngine::new(PrivacyConfig::default());
        let decision = engine.inspect(&RequestContext {
            url: "http://example.com/".to_owned(),
            top_level_url: None,
            resource_type: ResourceType::Document,
            ..Default::default()
        });
        assert_ne!(decision.blocked, Some(BlockReason::MixedContent));
    }
}
