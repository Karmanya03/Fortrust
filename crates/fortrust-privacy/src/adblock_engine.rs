use adblock::engine::Engine as AdblockEngine;
use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use url::Url;

#[derive(Debug, Clone, Default)]
pub struct CosmeticResources {
    /// CSS rules for hiding elements (e.g., ".ad-banner { display: none !important }")
    pub hide_selectors: Vec<String>,
    /// Procedural cosmetic filter scriptlets
    pub procedural_actions: Vec<String>,
    /// JavaScript to be injected for cosmetic purposes
    pub injected_script: Option<String>,
    /// Whether generic hide rules are disabled
    pub generichide: bool,
}

#[derive(Debug, Clone)]
pub struct PrivacySettings {
    pub block_ads: bool,
    pub block_trackers: bool,
    pub block_fingerprinting: bool,
    pub https_only: bool,
    pub block_third_party_cookies: bool,
    pub strip_tracking_params: bool,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            block_ads: true,
            block_trackers: true,
            block_fingerprinting: true,
            https_only: true,
            block_third_party_cookies: true,
            strip_tracking_params: true,
        }
    }
}

pub struct PrivacyFilter {
    adblock_engine: AdblockEngine,
    pub settings: PrivacySettings,
}

impl PrivacyFilter {
    pub fn load() -> Self {
        let mut filter_set = FilterSet::new(true);

        let lists: &[(&str, &str)] = &[
            (
                "easylist",
                include_str!("../../../assets/filter-lists/easylist.txt"),
            ),
            (
                "easyprivacy",
                include_str!("../../../assets/filter-lists/easyprivacy.txt"),
            ),
            (
                "brave-unbreak",
                include_str!("../../../assets/filter-lists/brave-unbreak.txt"),
            ),
        ];

        for (_name, content) in lists {
            filter_set.add_filter_list(
                content,
                ParseOptions {
                    format: adblock::lists::FilterFormat::Standard,
                    ..Default::default()
                },
            );
        }

        Self {
            adblock_engine: AdblockEngine::from_filter_set(filter_set, true),
            settings: PrivacySettings::default(),
        }
    }

    pub fn should_block(&self, url: &str, source_url: &str, resource_type: &str) -> bool {
        if !self.settings.block_ads && !self.settings.block_trackers {
            return false;
        }
        let Ok(req) = Request::new(url, source_url, resource_type) else {
            return false;
        };
        let result = self.adblock_engine.check_network_request(&req);
        result.matched && result.exception.is_none()
    }

    /// Get cosmetic filtering resources (CSS hide selectors, scriptlets) for a given URL.
    /// These hide ad placeholders, cookie banners, etc. using `##` selector rules from filter lists.
    /// Returns CSS rules like `.ad-banner { display: none !important }` for each hide selector.
    pub fn get_cosmetic_resources(&self, url: &str) -> CosmeticResources {
        if !self.settings.block_ads && !self.settings.block_trackers {
            return CosmeticResources::default();
        }
        let resources = self.adblock_engine.url_cosmetic_resources(url);
        let hide_selectors: Vec<String> = resources
            .hide_selectors
            .iter()
            .map(|sel| format!("{sel} {{ display: none !important; }}"))
            .collect();
        CosmeticResources {
            hide_selectors,
            procedural_actions: resources
                .procedural_actions
                .iter()
                .map(|a| format!("{a} {{ display: none !important; }}"))
                .collect(),
            injected_script: Some(resources.injected_script.clone()).filter(|s| !s.is_empty()),
            generichide: resources.generichide,
        }
    }

    pub fn strip_tracking_params(&self, url: &mut Url) {
        if !self.settings.strip_tracking_params {
            return;
        }

        const STRIP_PARAMS: &[&str] = &[
            "utm_source",
            "utm_medium",
            "utm_campaign",
            "utm_term",
            "utm_content",
            "fbclid",
            "gclid",
            "gclsrc",
            "dclid",
            "msclkid",
            "mc_eid",
            "ref",
            "referrer",
            "_ga",
            "igshid",
            "zanpid",
        ];

        let query: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(k, _)| !STRIP_PARAMS.contains(&k.as_ref()))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        if query.is_empty() {
            url.set_query(None);
        } else {
            url.set_query(Some(
                &query
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("&"),
            ));
        }
    }
}

pub fn create_placeholder_filter_lists() {
    let paths = [
        "assets/filter-lists/easylist.txt",
        "assets/filter-lists/easyprivacy.txt",
        "assets/filter-lists/brave-unbreak.txt",
    ];

    for path_str in &paths {
        let path = std::path::Path::new(path_str);
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let content = format!(
                "! {} — placeholder\n! Download the actual list from the official sources\n",
                path.file_stem().unwrap_or_default().to_string_lossy()
            );
            let _ = std::fs::write(path, content);
        }
    }
}
