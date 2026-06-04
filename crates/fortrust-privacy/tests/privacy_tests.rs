use fortrust_privacy::{
    AdBlocker, BlockerDecision, CookiePolicy, CspDirective, CspPolicy, CspSource,
    FingerprintGuard, HttpsDecision, HttpsUpgrader, PolicyDirective, PrivacyFilter,
    PrivacyManager, PrivacyStats, RequestClassifier,
};

#[test]
fn adblocker_blocks_known_ad_domain() {
    let blocker = AdBlocker::new();
    let decision = blocker.should_block("https://doubleclick.net/pixel?id=123", "https://example.com", "image");
    assert!(matches!(decision, BlockerDecision::Block { .. }));
}

#[test]
fn adblocker_allows_safe_domain() {
    let blocker = AdBlocker::new();
    let decision = blocker.should_block("https://example.com/style.css", "https://example.com", "stylesheet");
    assert_eq!(decision, BlockerDecision::Allow);
}

#[test]
fn adblocker_blocks_google_analytics() {
    let blocker = AdBlocker::new();
    let decision = blocker.should_block("https://www.google-analytics.com/collect", "https://example.com", "script");
    assert!(matches!(decision, BlockerDecision::Block { .. }));
}

#[test]
fn adblocker_blocks_facebook_tracker() {
    let blocker = AdBlocker::new();
    let decision = blocker.should_block("https://connect.facebook.net/en_US/fbevents.js", "https://example.com", "script");
    assert!(matches!(decision, BlockerDecision::Block { .. }));
}

#[test]
fn adblocker_blocks_pagead_pattern() {
    let blocker = AdBlocker::new();
    let decision = blocker.should_block("https://example.com/pagead/ads", "https://example.com", "script");
    assert!(matches!(decision, BlockerDecision::Block { .. }));
}

#[test]
fn https_upgrader_upgrades_http() {
    let upgrader = HttpsUpgrader::new();
    let decision = upgrader.evaluate("http://example.com/page");
    assert!(matches!(decision, HttpsDecision::Upgraded(u) if u == "https://example.com/page"));
}

#[test]
fn https_upgrader_allows_https() {
    let upgrader = HttpsUpgrader::new();
    let decision = upgrader.evaluate("https://example.com/page");
    assert_eq!(decision, HttpsDecision::AlreadyHttps);
}

#[test]
fn privacy_filter_uses_adblock_engine() {
    let filter = PrivacyFilter::load();
    let blocked = filter.should_block("https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js", "https://example.com", "script");
    assert!(blocked, "PrivacyFilter should block pagead2.googlesyndication.com");
}

#[test]
fn privacy_filter_allows_safe() {
    let filter = PrivacyFilter::load();
    let blocked = filter.should_block("https://example.com/style.css", "https://example.com", "stylesheet");
    assert!(!blocked, "PrivacyFilter should allow example.com");
}

#[test]
fn privacy_filter_respects_disabled_setting() {
    let mut filter = PrivacyFilter::load();
    filter.settings.block_ads = false;
    filter.settings.block_trackers = false;
    let blocked = filter.should_block("https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js", "https://example.com", "script");
    assert!(!blocked, "PrivacyFilter should allow when blocking disabled");
}

#[test]
fn privacy_filter_strips_tracking_params() {
    let filter = PrivacyFilter::load();
    let mut url = url::Url::parse("https://example.com/page?utm_source=fb&q=hello&utm_medium=cpc").unwrap();
    filter.strip_tracking_params(&mut url);
    assert!(!url.query().unwrap_or("").contains("utm_source"));
    assert!(!url.query().unwrap_or("").contains("utm_medium"));
    assert!(url.query().unwrap_or("").contains("q=hello"));
}

#[test]
fn privacy_manager_tracks_stats() {
    let mut pm = PrivacyManager::new();
    pm.should_block_request("https://doubleclick.net/ad", "https://example.com", "image");
    pm.should_block_request("https://google-analytics.com/ga", "https://example.com", "script");
    pm.should_block_request("https://safe-site.com/style.css", "https://example.com", "stylesheet");
    assert!(pm.stats.ads_blocked + pm.stats.trackers_blocked >= 1);
}

#[test]
fn privacy_manager_https_upgrade() {
    let mut pm = PrivacyManager::new();
    let decision = pm.upgrade_url("http://example.com/page");
    assert!(matches!(decision, HttpsDecision::Upgraded(_)));
}

#[test]
fn cookie_policy_blocks_third_party() {
    let policy = CookiePolicy::BlockThirdParty;
    assert!(policy.allows_domain("example.com", "example.com"));
    assert!(!policy.allows_domain("tracker.com", "example.com"));
}

#[test]
fn cookie_policy_accepts_all() {
    let policy = CookiePolicy::AcceptAll;
    assert!(policy.allows_domain("tracker.com", "example.com"));
}

#[test]
fn fingerprint_guard_generates_noise() {
    let guard = FingerprintGuard::new();
    let res = guard.get_screen_resolution();
    assert!(res.0 > 0 && res.1 > 0);
    let tz = guard.get_timezone();
    assert!(!tz.is_empty());
}

#[test]
fn request_classifier_detects_resource_types() {
    assert_eq!(RequestClassifier::classify("https://example.com/script.js", None), "script");
    assert_eq!(RequestClassifier::classify("https://example.com/style.css", None), "stylesheet");
    assert_eq!(RequestClassifier::classify("https://example.com/image.png", None), "image");
    assert_eq!(RequestClassifier::classify("https://example.com/font.woff2", None), "font");
    assert_eq!(RequestClassifier::classify("https://example.com/video.mp4", None), "media");
    assert_eq!(RequestClassifier::classify("https://example.com/data.json", None), "xhr");
    assert_eq!(RequestClassifier::classify("https://example.com/page.html", None), "document");
}

#[test]
fn csp_policy_blocks_script_from_untrusted_source() {
    let mut policy = CspPolicy::new();
    policy.directives.push(PolicyDirective {
        directive: CspDirective::ScriptSrc,
        sources: vec![CspSource::Host("https://example.com".to_owned())].into(),
    });
    assert!(policy.allows(&CspDirective::ScriptSrc, "https://example.com/app.js"));
    assert!(!policy.allows(&CspDirective::ScriptSrc, "https://evil.com/hack.js"));
}

#[test]
fn privacy_stats_session_duration() {
    let stats = PrivacyStats::default();
    let dur = stats.session_duration();
    assert!(dur.as_secs() < 5);
}

#[test]
fn privacy_stats_blocked_per_minute() {
    let stats = PrivacyStats { ads_blocked: 10, trackers_blocked: 20, ..Default::default() };
    let rate = stats.blocked_per_minute();
    assert!(rate >= 0.0);
}

#[test]
fn privacy_manager_reset_stats() {
    let mut pm = PrivacyManager::new();
    pm.stats.ads_blocked = 100;
    pm.reset_stats();
    assert_eq!(pm.stats.ads_blocked, 0);
    assert_eq!(pm.stats.trackers_blocked, 0);
}
