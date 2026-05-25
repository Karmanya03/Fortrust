use chrono::{DateTime, Utc};
use dashmap::DashMap;
use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::debug;

use crate::StorageError;

const COOKIES_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("cookies");

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct CookieKey {
    pub domain: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieValue {
    pub value: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_access: DateTime<Utc>,
    pub persistent: bool,
    pub host_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CookiePolicy {
    AcceptAll,
    #[default]
    BlockThirdParty,
    BlockAll,
    RejectTrackers,
}

#[derive(Debug, Clone)]
pub struct CookieJar {
    cookies: DashMap<CookieKey, CookieValue>,
    policy: CookiePolicy,
}

impl CookieJar {
    pub fn new() -> Self {
        Self {
            cookies: DashMap::new(),
            policy: CookiePolicy::BlockThirdParty,
        }
    }

    pub fn with_policy(policy: CookiePolicy) -> Self {
        Self {
            cookies: DashMap::new(),
            policy,
        }
    }

    pub fn set(&self, key: CookieKey, value: CookieValue) {
        if !self.should_accept(&key, &value) {
            debug!("Cookie rejected by policy: {}@{}", key.name, key.domain);
            return;
        }
        self.cookies.insert(key, value);
    }

    pub fn get(&self, domain: &str, path: &str, name: &str) -> Option<CookieValue> {
        let key = CookieKey {
            domain: domain.to_owned(),
            name: name.to_owned(),
            path: path.to_owned(),
        };
        self.cookies.get(&key).map(|r| r.clone())
    }

    pub fn get_for_url(&self, url: &url::Url) -> Vec<(CookieKey, CookieValue)> {
        let domain = url.host_str().unwrap_or("");
        let path = url.path();
        self.cookies
            .iter()
            .filter(|entry| {
                let (key, value) = entry.pair();
                if value.expires_at.is_some_and(|exp| exp < Utc::now()) {
                    return false;
                }
                if value.secure && url.scheme() != "https" {
                    return false;
                }
                if !domain_matches(&key.domain, domain) {
                    return false;
                }
                if !path_matches(&key.path, path) {
                    return false;
                }
                true
            })
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect()
    }

    pub fn remove(&self, key: &CookieKey) {
        self.cookies.remove(key);
    }

    pub fn remove_for_domain(&self, domain: &str) {
        self.cookies.retain(|key, _| key.domain != domain);
    }

    pub fn clear(&self) {
        self.cookies.clear();
    }

    pub fn count(&self) -> usize {
        self.cookies.len()
    }

    fn should_accept(&self, key: &CookieKey, value: &CookieValue) -> bool {
        match self.policy {
            CookiePolicy::AcceptAll => true,
            CookiePolicy::BlockAll => false,
            CookiePolicy::BlockThirdParty => !value.host_only || is_third_party(&key.domain),
            CookiePolicy::RejectTrackers => !looks_like_tracker(&key.domain, &key.name),
        }
    }

    pub fn set_policy(&mut self, policy: CookiePolicy) {
        self.policy = policy;
    }

    pub fn policy(&self) -> CookiePolicy {
        self.policy
    }
}

impl Default for CookieJar {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CookieDatabase {
    #[allow(dead_code)]
    db: Option<Arc<Database>>,
    jar: CookieJar,
}

impl CookieDatabase {
    pub fn new(db: Arc<Database>) -> Result<Self, StorageError> {
        let write_txn = db.begin_write()?;
        write_txn.open_table(COOKIES_TABLE)?;
        write_txn.commit()?;
        Ok(Self {
            db: Some(db),
            jar: CookieJar::new(),
        })
    }

    pub fn empty() -> Self {
        Self {
            db: None,
            jar: CookieJar::new(),
        }
    }

    pub fn jar(&self) -> &CookieJar {
        &self.jar
    }

    pub fn cookie_policy(&self) -> CookiePolicy {
        self.jar.policy()
    }

    pub fn set_cookie_policy(&mut self, policy: CookiePolicy) {
        self.jar.set_policy(policy);
    }

    pub fn count(&self) -> usize {
        self.jar.count()
    }
}

fn domain_matches(cookie_domain: &str, request_domain: &str) -> bool {
    cookie_domain == request_domain || request_domain.ends_with(&format!(".{cookie_domain}"))
}

fn path_matches(cookie_path: &str, request_path: &str) -> bool {
    request_path.starts_with(cookie_path)
}

fn is_third_party(domain: &str) -> bool {
    let known_first_parties = [
        "google.com",
        "facebook.com",
        "youtube.com",
        "twitter.com",
        "instagram.com",
        "linkedin.com",
        "reddit.com",
        "amazon.com",
        "netflix.com",
        "github.com",
        "stackoverflow.com",
    ];
    !known_first_parties.iter().any(|&d| domain.ends_with(d))
}

fn looks_like_tracker(domain: &str, name: &str) -> bool {
    let tracker_domains = [
        "doubleclick.net",
        "google-analytics.com",
        "googletagmanager.com",
        "facebook.net",
        "scorecardresearch.com",
        "hotjar.com",
        "adsystem.com",
        "adservice.google.com",
    ];
    let tracker_names = ["_ga", "_gid", "_fbp", "_gclid", " IDE"];

    tracker_domains.iter().any(|d| domain.contains(d))
        || tracker_names.iter().any(|n| name.starts_with(n))
}
