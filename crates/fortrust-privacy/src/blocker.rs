use ahash::AHashSet;
use compact_str::CompactString;
use tracing::debug;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockerDecision {
    Allow,
    Block {
        reason: String,
        rule: Option<String>,
        category: BlockCategory,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockCategory {
    Ad,
    Tracker,
    Malware,
    Social,
    Analytics,
    CookieConsent,
    Other,
}

#[derive(Debug, Clone)]
pub struct FilterListProvider {
    host_based: AHashSet<CompactString>,
    url_pattern: Vec<(String, BlockCategory)>,
    domain_allowlist: AHashSet<CompactString>,
}

impl FilterListProvider {
    pub fn new() -> Self {
        Self {
            host_based: built_in_tracker_hosts(),
            url_pattern: built_in_url_patterns(),
            domain_allowlist: AHashSet::new(),
        }
    }

    /// Load host-based blocklist entries from a hosts-format byte slice.
    /// Lines starting with '#' are ignored. Each remaining non-empty token is treated as a blocked host.
    pub fn load_hosts_from_bytes(&mut self, bytes: &[u8]) {
        if let Ok(text) = std::str::from_utf8(bytes) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') { continue; }
                // hosts files often have: 0.0.0.0 domain.com
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(host) = parts.last() {
                    let host = host.trim().trim_start_matches('.');
                    if !host.is_empty() && !host.contains('/') && !host.contains(':') {
                        self.host_based.insert(CompactString::from(host));
                    }
                }
            }
        }
    }

    pub fn check_url(&self, url: &str, _source_url: &str, _resource_type: &str) -> BlockerDecision {
        if self.is_allowlisted(url) {
            return BlockerDecision::Allow;
        }

        if let Some(category) = self.match_url_pattern(url) {
            let reason = match category {
                BlockCategory::Ad => "ad".to_owned(),
                BlockCategory::Tracker => "tracker".to_owned(),
                BlockCategory::Malware => "malware".to_owned(),
                BlockCategory::Social => "social_widget".to_owned(),
                BlockCategory::Analytics => "analytics".to_owned(),
                BlockCategory::CookieConsent => "cookie_consent".to_owned(),
                BlockCategory::Other => "other".to_owned(),
            };
            return BlockerDecision::Block {
                reason,
                rule: Some(url.to_owned()),
                category,
            };
        }

        if let Some(host) = extract_host(url) {
            if self.host_based.contains(host.as_str()) {
                return BlockerDecision::Block {
                    reason: "tracker_host".to_owned(),
                    rule: Some(host),
                    category: BlockCategory::Tracker,
                };
            }

            if self.is_subdomain_blocked(&host) {
                return BlockerDecision::Block {
                    reason: "tracker_subdomain".to_owned(),
                    rule: Some(host),
                    category: BlockCategory::Tracker,
                };
            }
        }

        BlockerDecision::Allow
    }

    fn match_url_pattern(&self, url: &str) -> Option<BlockCategory> {
        let lower = url.to_ascii_lowercase();
        for (pattern, category) in &self.url_pattern {
            if lower.contains(pattern) {
                return Some(*category);
            }
        }
        None
    }

    fn is_allowlisted(&self, url: &str) -> bool {
        let Some(host) = extract_host(url) else {
            return false;
        };
        self.domain_allowlist.contains(host.as_str())
    }

    fn is_subdomain_blocked(&self, host: &str) -> bool {
        self.host_based.iter().any(|blocked| {
            host.strip_suffix(blocked.as_str())
                .is_some_and(|prefix| prefix.ends_with('.') || prefix.is_empty())
        })
    }

    pub fn add_to_allowlist(&mut self, domain: &str) {
        self.domain_allowlist.insert(CompactString::from(domain));
    }

    pub fn remove_from_allowlist(&mut self, domain: &str) {
        self.domain_allowlist.remove(domain);
    }
}

impl Default for FilterListProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct AdBlocker {
    filters: FilterListProvider,
}

impl Default for AdBlocker {
    fn default() -> Self {
        Self::new()
    }
}

impl AdBlocker {
    pub fn new() -> Self {
        Self {
            filters: FilterListProvider::new(),
        }
    }

    /// Load host-based blocklist entries from a hosts-format byte slice into the internal filters.
    pub fn load_hosts_from_bytes(&mut self, bytes: &[u8]) {
        self.filters.load_hosts_from_bytes(bytes);
    }

    pub fn should_block(
        &self,
        url: &str,
        source_url: &str,
        resource_type: &str,
    ) -> BlockerDecision {
        let decision = self.filters.check_url(url, source_url, resource_type);

        if matches!(decision, BlockerDecision::Block { .. }) {
            debug!(
                url = url,
                resource_type = resource_type,
                "Ad/tracker blocked"
            );
        }

        decision
    }
}

pub struct RequestClassifier;

impl RequestClassifier {
    pub fn classify(url: &str, content_type: Option<&str>) -> &'static str {
        if let Some(ct) = content_type {
            if ct.starts_with("text/html") {
                return "document";
            }
            if ct.starts_with("text/css") {
                return "stylesheet";
            }
            if ct.starts_with("application/javascript")
                || ct.starts_with("text/javascript")
                || ct.starts_with("application/x-javascript")
            {
                return "script";
            }
            if ct.starts_with("image/") {
                return "image";
            }
            if ct.starts_with("font/") || ct.contains("font") || url.contains(".woff") {
                return "font";
            }
            if ct.starts_with("audio/") || ct.starts_with("video/") {
                return "media";
            }
        }

        let lower = url.to_ascii_lowercase();
        if lower.ends_with(".js") {
            return "script";
        }
        if lower.ends_with(".css") {
            return "stylesheet";
        }
        if lower.ends_with(".png")
            || lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".gif")
            || lower.ends_with(".webp")
            || lower.ends_with(".svg")
            || lower.ends_with(".ico")
        {
            return "image";
        }
        if lower.ends_with(".woff")
            || lower.ends_with(".woff2")
            || lower.ends_with(".ttf")
            || lower.ends_with(".otf")
        {
            return "font";
        }
        if lower.ends_with(".mp4")
            || lower.ends_with(".webm")
            || lower.ends_with(".mp3")
            || lower.ends_with(".wav")
        {
            return "media";
        }
        if lower.ends_with(".html") || lower.ends_with(".htm") {
            return "document";
        }
        if lower.ends_with(".json") || lower.ends_with(".xml") {
            return "xhr";
        }

        "other"
    }
}

fn extract_host(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .or_else(|| url::Url::parse(&format!("https://{url}")).ok())
        .and_then(|parsed| parsed.host_str().map(str::to_ascii_lowercase))
}

fn built_in_tracker_hosts() -> AHashSet<CompactString> {
    [
        "doubleclick.net",
        "google-analytics.com",
        "googletagmanager.com",
        "googleadservices.com",
        "googleads.g.doubleclick.net",
        "pagead2.googlesyndication.com",
        "facebook.net",
        "facebook.com/tr",
        "connect.facebook.net",
        "scorecardresearch.com",
        "hotjar.com",
        "static.hotjar.com",
        "segment.io",
        "segment.com",
        "cdn.segment.com",
        "mixpanel.com",
        "api.mixpanel.com",
        "amplitude.com",
        "api.amplitude.com",
        "fullstory.com",
        "rs.fullstory.com",
        "crazyegg.com",
        "dnn506yrbagrg.cloudfront.net",
        "optimizely.com",
        "cdn.optimizely.com",
        "mouseflow.com",
        "cdn.mouseflow.com",
        "clarity.ms",
        "c.clarity.ms",
        "hubspot.com",
        "track.hubspot.com",
        "linkedin.com/px",
        "ads.linkedin.com",
        "twitter.com/i/jot",
        "analytics.twitter.com",
        "pixel.quantserve.com",
        "secure.quantserve.com",
        "browser.sentry-cdn.com",
        "o73581.ingest.sentry.io",
        "cdn.braze.com",
        "appboy.com",
        "tealiumiq.com",
        "tags.tiqcdn.com",
        "bluekai.com",
        "tags.bluekai.com",
        "exelator.com",
        "sync.exelator.com",
        "demdex.net",
        "dpm.demdex.net",
        "adsafeprotected.com",
        "static.adsafeprotected.com",
        "moatads.com",
        "js.moatads.com",
    ]
    .into_iter()
    .map(CompactString::from)
    .collect()
}

fn built_in_url_patterns() -> Vec<(String, BlockCategory)> {
    vec![
        ("/pagead/".to_owned(), BlockCategory::Ad),
        ("/ads/".to_owned(), BlockCategory::Ad),
        ("/adserver".to_owned(), BlockCategory::Ad),
        ("/banner".to_owned(), BlockCategory::Ad),
        ("doubleclick.net".to_owned(), BlockCategory::Ad),
        ("adservice.google.".to_owned(), BlockCategory::Ad),
        ("google-analytics.com".to_owned(), BlockCategory::Analytics),
        ("analytics.".to_owned(), BlockCategory::Analytics),
        ("/gtag/".to_owned(), BlockCategory::Analytics),
        ("/gtm.js".to_owned(), BlockCategory::Analytics),
        ("facebook.net".to_owned(), BlockCategory::Social),
        ("facebook.com/tr".to_owned(), BlockCategory::Social),
        ("twitter.com/i/jot".to_owned(), BlockCategory::Social),
        ("linkedin.com/px".to_owned(), BlockCategory::Social),
        ("hotjar.com".to_owned(), BlockCategory::Analytics),
        ("cdn.segment.".to_owned(), BlockCategory::Analytics),
        ("/amplitude.".to_owned(), BlockCategory::Analytics),
        ("/fullstory.".to_owned(), BlockCategory::Analytics),
        ("clarity.ms".to_owned(), BlockCategory::Analytics),
        ("hubspot.".to_owned(), BlockCategory::Analytics),
        ("/cookie-notice".to_owned(), BlockCategory::CookieConsent),
        ("/cookie-consent".to_owned(), BlockCategory::CookieConsent),
        ("cookiebot.".to_owned(), BlockCategory::CookieConsent),
        ("onetrust.".to_owned(), BlockCategory::CookieConsent),
        ("scorecardresearch.".to_owned(), BlockCategory::Tracker),
    ]
}
