use tracing::debug;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpsDecision {
    AlreadyHttps,
    Upgraded(String),
    NotUpgradable,
    Blocked,
}

#[derive(Debug, Clone)]
pub struct UpgradeRule {
    pub domain: String,
    pub include_subdomains: bool,
}

#[derive(Debug, Clone)]
pub struct HttpsUpgrader {
    rules: Vec<UpgradeRule>,
    always_upgrade: bool,
}

impl HttpsUpgrader {
    pub fn new() -> Self {
        Self {
            rules: Self::built_in_rules(),
            always_upgrade: true,
        }
    }

    pub fn evaluate(&self, url_str: &str) -> HttpsDecision {
        let Ok(url) = url::Url::parse(url_str) else {
            return HttpsDecision::NotUpgradable;
        };

        if url.scheme() == "https" {
            return HttpsDecision::AlreadyHttps;
        }

        if url.scheme() != "http" {
            return HttpsDecision::NotUpgradable;
        }

        if self.always_upgrade {
            if let Some(host) = url.host_str() {
                // Skip localhost and private IPs
                if host == "localhost"
                    || host == "127.0.0.1"
                    || host.starts_with("10.")
                    || host.starts_with("192.168.")
                    || host.starts_with("172.")
                {
                    return HttpsDecision::NotUpgradable;
                }
            }

            let mut upgraded = url.clone();
            let _ = upgraded.set_scheme("https");
            let result = upgraded.to_string();

            if self.has_rule(url.host_str().unwrap_or("")) {
                debug!("HTTPS upgraded (rule match): {url_str} -> {result}");
            }

            return HttpsDecision::Upgraded(result);
        }

        HttpsDecision::NotUpgradable
    }

    pub fn add_rule(&mut self, domain: &str, include_subdomains: bool) {
        self.rules.push(UpgradeRule {
            domain: domain.to_ascii_lowercase(),
            include_subdomains,
        });
    }

    fn has_rule(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.rules.iter().any(|rule| {
            if rule.include_subdomains {
                host == rule.domain || host.ends_with(&format!(".{}", rule.domain))
            } else {
                host == rule.domain
            }
        })
    }

    pub fn set_always_upgrade(&mut self, always: bool) {
        self.always_upgrade = always;
    }

    fn built_in_rules() -> Vec<UpgradeRule> {
        vec![
            UpgradeRule {
                domain: "google.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "youtube.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "facebook.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "twitter.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "instagram.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "linkedin.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "reddit.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "amazon.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "github.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "stackoverflow.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "wikipedia.org".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "mozilla.org".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "duckduckgo.com".to_owned(),
                include_subdomains: true,
            },
            UpgradeRule {
                domain: "brave.com".to_owned(),
                include_subdomains: true,
            },
        ]
    }
}

impl Default for HttpsUpgrader {
    fn default() -> Self {
        Self::new()
    }
}
