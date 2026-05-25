use smallvec::SmallVec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CspDirective {
    DefaultSrc,
    ScriptSrc,
    StyleSrc,
    ImgSrc,
    ConnectSrc,
    FontSrc,
    FrameSrc,
    MediaSrc,
    ObjectSrc,
    ManifestSrc,
    WorkerSrc,
    FrameAncestors,
    FormAction,
    BaseUri,
    ReportUri,
    BlockAllMixedContent,
    UpgradeInsecureRequests,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CspSource {
    None,
    Self_,
    UnsafeInline,
    UnsafeEval,
    StrictDynamic,
    Https,
    Data,
    Blob,
    MediaSrc,
    Filesystem,
    Host(String),
    Scheme(String),
    Nonce(String),
    Hash(String),
    ReportSample,
}

#[derive(Debug, Clone)]
pub struct PolicyDirective {
    pub directive: CspDirective,
    pub sources: SmallVec<[CspSource; 8]>,
}

#[derive(Debug, Clone)]
pub struct CspPolicy {
    pub directives: Vec<PolicyDirective>,
    pub report_uri: Option<String>,
    pub block_all_mixed_content: bool,
    pub upgrade_insecure_requests: bool,
}

impl CspPolicy {
    pub fn new() -> Self {
        Self {
            directives: Vec::new(),
            report_uri: None,
            block_all_mixed_content: true,
            upgrade_insecure_requests: true,
        }
    }

    pub fn parse(header: &str) -> Self {
        let mut policy = Self::new();
        for directive_str in header.split(';') {
            let directive_str = directive_str.trim();
            if directive_str.is_empty() {
                continue;
            }

            let parts: Vec<&str> = directive_str.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let directive_name = parts[0].to_ascii_lowercase();
            let sources: SmallVec<[CspSource; 8]> = parts[1..]
                .iter()
                .filter_map(|s| parse_csp_source(s))
                .collect();

            if let Some(directive) = csp_directive_from_str(&directive_name) {
                match directive {
                    CspDirective::BlockAllMixedContent => {
                        policy.block_all_mixed_content = true;
                    }
                    CspDirective::UpgradeInsecureRequests => {
                        policy.upgrade_insecure_requests = true;
                    }
                    CspDirective::ReportUri => {
                        policy.report_uri = sources.first().and_then(|s| {
                            if let CspSource::Host(h) = s {
                                Some(h.clone())
                            } else {
                                None
                            }
                        });
                    }
                    _ => {
                        policy.directives.push(PolicyDirective {
                            directive,
                            sources,
                        });
                    }
                }
            }
        }
        policy
    }

    pub fn allows(&self, directive: &CspDirective, resource_url: &str) -> bool {
        let applicable = self
            .directives
            .iter()
            .find(|d| &d.directive == directive)
            .or_else(|| {
                self.directives
                    .iter()
                    .find(|d| d.directive == CspDirective::DefaultSrc)
            });

        let Some(policy) = applicable else {
            return true;
        };

        if policy.sources.is_empty() {
            return false;
        }

        policy.sources.iter().any(|source| source_matches(source, resource_url))
    }

    pub fn allows_inline_script(&self) -> bool {
        self.allows_source(&CspDirective::ScriptSrc, &CspSource::UnsafeInline)
            || self.allows_source(&CspDirective::DefaultSrc, &CspSource::UnsafeInline)
    }

    pub fn allows_eval(&self) -> bool {
        self.allows_source(&CspDirective::ScriptSrc, &CspSource::UnsafeEval)
            || self.allows_source(&CspDirective::DefaultSrc, &CspSource::UnsafeEval)
    }

    fn allows_source(&self, directive: &CspDirective, source: &CspSource) -> bool {
        let applicable = self
            .directives
            .iter()
            .find(|d| &d.directive == directive)
            .or_else(|| {
                self.directives
                    .iter()
                    .find(|d| d.directive == CspDirective::DefaultSrc)
            });

        let Some(policy) = applicable else {
            return true;
        };

        policy.sources.iter().any(|s| s == source)
    }

    pub fn is_strict(&self) -> bool {
        self.directives.iter().any(|d| {
            d.sources
                .iter()
                .any(|s| matches!(s, CspSource::None))
        })
    }
}

impl Default for CspPolicy {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_csp_source(source: &str) -> Option<CspSource> {
    match source {
        "'none'" => Some(CspSource::None),
        "'self'" => Some(CspSource::Self_),
        "'unsafe-inline'" => Some(CspSource::UnsafeInline),
        "'unsafe-eval'" => Some(CspSource::UnsafeEval),
        "'strict-dynamic'" => Some(CspSource::StrictDynamic),
        "https:" => Some(CspSource::Scheme("https".to_owned())),
        "data:" => Some(CspSource::Data),
        "blob:" => Some(CspSource::Blob),
        "mediasrc:" => Some(CspSource::MediaSrc),
        "filesystem:" => Some(CspSource::Filesystem),
        s if s.starts_with("'nonce-") && s.ends_with('\'') => {
            let nonce = &s[7..s.len() - 1];
            Some(CspSource::Nonce(nonce.to_owned()))
        }
        s if s.starts_with("'sha") && s.ends_with('\'') => {
            Some(CspSource::Hash(s.to_owned()))
        }
        s if s.starts_with("http://") || s.starts_with("https://") || s.contains('.') => {
            Some(CspSource::Host(s.to_owned()))
        }
        _ => None,
    }
}

fn csp_directive_from_str(s: &str) -> Option<CspDirective> {
    match s {
        "default-src" => Some(CspDirective::DefaultSrc),
        "script-src" => Some(CspDirective::ScriptSrc),
        "style-src" => Some(CspDirective::StyleSrc),
        "img-src" => Some(CspDirective::ImgSrc),
        "connect-src" => Some(CspDirective::ConnectSrc),
        "font-src" => Some(CspDirective::FontSrc),
        "frame-src" => Some(CspDirective::FrameSrc),
        "media-src" => Some(CspDirective::MediaSrc),
        "object-src" => Some(CspDirective::ObjectSrc),
        "manifest-src" => Some(CspDirective::ManifestSrc),
        "worker-src" => Some(CspDirective::WorkerSrc),
        "frame-ancestors" => Some(CspDirective::FrameAncestors),
        "form-action" => Some(CspDirective::FormAction),
        "base-uri" => Some(CspDirective::BaseUri),
        "report-uri" => Some(CspDirective::ReportUri),
        "block-all-mixed-content" => Some(CspDirective::BlockAllMixedContent),
        "upgrade-insecure-requests" => Some(CspDirective::UpgradeInsecureRequests),
        _ => None,
    }
}

fn source_matches(source: &CspSource, resource_url: &str) -> bool {
    match source {
        CspSource::None => false,
        CspSource::Self_ => true,
        CspSource::Https => resource_url.starts_with("https://"),
        CspSource::Data => resource_url.starts_with("data:"),
        CspSource::Blob => resource_url.starts_with("blob:"),
        CspSource::Host(pattern) => resource_url.contains(pattern.as_str()),
        CspSource::Scheme(scheme) => resource_url.starts_with(&format!("{scheme}:")),
        _ => true,
    }
}
