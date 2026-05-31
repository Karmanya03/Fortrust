pub mod adblock_engine;
pub mod blocker;
pub mod cookie_policy;
pub mod csp;
pub mod fingerprint;
pub mod https_upgrade;

pub use adblock_engine::{CosmeticResources, PrivacyFilter, PrivacySettings};
pub use blocker::{AdBlocker, BlockerDecision, FilterListProvider, RequestClassifier};
pub use cookie_policy::{CookiePolicy, SameSitePolicy};
pub use csp::{CspDirective, CspPolicy, CspSource, PolicyDirective};
pub use fingerprint::{AudioNoise, CanvasNoise, FingerprintGuard, NoiseStrategy};
pub use https_upgrade::{HttpsDecision, HttpsUpgrader, UpgradeRule};

use std::time::{Duration, SystemTime};

#[derive(Debug, Clone)]
pub struct PrivacyStats {
    pub ads_blocked: u64,
    pub trackers_blocked: u64,
    pub https_upgrades: u64,
    pub fingerprint_attempts_blocked: u64,
    pub third_party_cookies_blocked: u64,
    pub scripts_blocked: u64,
    pub data_saved_bytes: u64,
    pub cosmetic_elements_hidden: u64,
    pub session_start: SystemTime,
}

impl Default for PrivacyStats {
    fn default() -> Self {
        Self {
            ads_blocked: 0,
            trackers_blocked: 0,
            https_upgrades: 0,
            fingerprint_attempts_blocked: 0,
            third_party_cookies_blocked: 0,
            scripts_blocked: 0,
            data_saved_bytes: 0,
            cosmetic_elements_hidden: 0,
            session_start: SystemTime::now(),
        }
    }
}

impl PrivacyStats {
    pub fn session_duration(&self) -> Duration {
        SystemTime::now()
            .duration_since(self.session_start)
            .unwrap_or_default()
    }

    pub fn blocked_per_minute(&self) -> f64 {
        let mins = self.session_duration().as_secs_f64() / 60.0;
        if mins > 0.0 {
            (self.ads_blocked + self.trackers_blocked) as f64 / mins
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct PrivacyManager {
    pub ad_blocker: AdBlocker,
    pub fingerprint_guard: FingerprintGuard,
    pub https_upgrader: HttpsUpgrader,
    pub cookie_policy: CookiePolicy,
    pub stats: PrivacyStats,
    pub csp_enabled: bool,
}

impl PrivacyManager {
    pub fn new() -> Self {
        let mut pm = Self {
            ad_blocker: AdBlocker::new(),
            fingerprint_guard: FingerprintGuard::new(),
            https_upgrader: HttpsUpgrader::new(),
            cookie_policy: CookiePolicy::BlockThirdParty,
            stats: PrivacyStats::default(),
            csp_enabled: true,
        };

        // Load embedded default hosts-format blocklist if present.
        // This is a small sample embedded list; in production this should be replaced with a curated list.
        #[allow(clippy::expect_used)]
        {
            const BYTES: &[u8] = include_bytes!("../../../assets/blocklists/hosts_default.txt");
            pm.ad_blocker.load_hosts_from_bytes(BYTES);
        }

        pm
    }

    pub fn should_block_request(
        &mut self,
        url: &str,
        source_url: &str,
        resource_type: &str,
    ) -> BlockerDecision {
        let decision = self.ad_blocker.should_block(url, source_url, resource_type);
        match &decision {
            BlockerDecision::Block { reason, .. } => match reason.as_str() {
                "ad" => self.stats.ads_blocked += 1,
                "tracker" => self.stats.trackers_blocked += 1,
                _ => {}
            },
            BlockerDecision::Allow => {}
        }
        decision
    }

    pub fn upgrade_url(&mut self, url: &str) -> HttpsDecision {
        let decision = self.https_upgrader.evaluate(url);
        if matches!(decision, HttpsDecision::Upgraded(_)) {
            self.stats.https_upgrades += 1;
            self.stats.data_saved_bytes += 100;
        }
        decision
    }

    pub fn check_csp(
        &self,
        policy: &CspPolicy,
        resource_url: &str,
        directive: &CspDirective,
    ) -> bool {
        if !self.csp_enabled {
            return true;
        }
        policy.allows(directive, resource_url)
    }

    pub fn get_cosmetic_resources(&mut self, url: &str) -> CosmeticResources {
        let filter = PrivacyFilter::load();
        let resources = filter.get_cosmetic_resources(url);
        let total = resources.hide_selectors.len() + resources.procedural_actions.len();
        self.stats.cosmetic_elements_hidden += total as u64;
        resources
    }

    pub fn reset_stats(&mut self) {
        self.stats = PrivacyStats::default();
    }
}

impl Default for PrivacyManager {
    fn default() -> Self {
        Self::new()
    }
}
