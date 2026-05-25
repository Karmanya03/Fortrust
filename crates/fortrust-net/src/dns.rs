use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DohProvider {
    Cloudflare,
    Quad9,
    Google,
    Custom(Url),
}

impl DohProvider {
    pub fn endpoint(&self) -> &Url {
        match self {
            Self::Cloudflare => static_url("https://cloudflare-dns.com/dns-query"),
            Self::Quad9 => static_url("https://dns.quad9.net/dns-query"),
            Self::Google => static_url("https://dns.google/dns-query"),
            Self::Custom(url) => url,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DohResolverConfig {
    provider: DohProvider,
    bootstrap_hosts: Vec<String>,
}

impl DohResolverConfig {
    pub fn new(provider: DohProvider) -> Self {
        let bootstrap_hosts = match &provider {
            DohProvider::Cloudflare => vec!["1.1.1.1".to_owned(), "1.0.0.1".to_owned()],
            DohProvider::Quad9 => vec!["9.9.9.9".to_owned(), "149.112.112.112".to_owned()],
            DohProvider::Google => vec!["8.8.8.8".to_owned(), "8.8.4.4".to_owned()],
            DohProvider::Custom(_) => Vec::new(),
        };

        Self {
            provider,
            bootstrap_hosts,
        }
    }

    pub fn privacy_default() -> Self {
        Self::new(DohProvider::Cloudflare)
    }

    pub fn provider(&self) -> &DohProvider {
        &self.provider
    }

    pub fn endpoint(&self) -> &Url {
        self.provider.endpoint()
    }

    pub fn bootstrap_hosts(&self) -> &[String] {
        &self.bootstrap_hosts
    }
}

fn static_url(raw: &'static str) -> &'static Url {
    use std::sync::OnceLock;

    static CLOUDFLARE: OnceLock<Url> = OnceLock::new();
    static QUAD9: OnceLock<Url> = OnceLock::new();
    static GOOGLE: OnceLock<Url> = OnceLock::new();

    match raw {
        "https://cloudflare-dns.com/dns-query" => {
            CLOUDFLARE.get_or_init(|| Url::parse(raw).expect("built-in DoH URL is valid"))
        }
        "https://dns.quad9.net/dns-query" => {
            QUAD9.get_or_init(|| Url::parse(raw).expect("built-in DoH URL is valid"))
        }
        "https://dns.google/dns-query" => {
            GOOGLE.get_or_init(|| Url::parse(raw).expect("built-in DoH URL is valid"))
        }
        _ => unreachable!("only built-in DoH URLs use static_url"),
    }
}
