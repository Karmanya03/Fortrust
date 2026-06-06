use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tokio::task::JoinSet;
use url::Url;

pub struct FortrustSearch {
    client: reqwest::Client,
    pub config: SearchConfig,
    cache: Option<Arc<SearchCache>>,
}

pub struct SearchConfig {
    pub enabled_backends: Vec<SearchBackend>,
    pub max_results: usize,
    pub safe_search: SafeSearchMode,
    pub language: String,
    pub deduplicate: bool,
    pub cache_enabled: bool,
    pub cache_max_entries: usize,
    pub cache_ttl_secs: u64,
    pub strict_result_safety: bool,
    pub block_non_https_results: bool,
    pub block_ip_address_hosts: bool,
    pub block_local_network_hosts: bool,
    pub strip_result_tracking_params: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            enabled_backends: vec![
                SearchBackend::DuckDuckGo,
                SearchBackend::Stract,
                SearchBackend::Wikipedia,
            ],
            max_results: 10,
            safe_search: SafeSearchMode::Moderate,
            language: "en-US".to_owned(),
            deduplicate: true,
            cache_enabled: true,
            cache_max_entries: 128,
            cache_ttl_secs: 300,
            strict_result_safety: true,
            block_non_https_results: false,
            block_ip_address_hosts: true,
            block_local_network_hosts: true,
            strip_result_tracking_params: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchBackend {
    BraveSearch,
    Mojeek,
    DuckDuckGo,
    Stract,
    Wikipedia,
    SearXNG(String), // e.g. "https://searx.be"
}

#[derive(Clone, Copy, PartialEq)]
pub enum SafeSearchMode {
    Off,
    Moderate,
    Strict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultSafety {
    Trusted,
    Neutral,
    Warning,
    Blocked,
}

#[derive(Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub display_url: String,
    pub host: String,
    pub archive_url: Option<String>,
    pub snippet: String,
    pub source_backend: SearchBackend,
    pub relevance_score: f32,
    pub privacy_score: u8,
    pub safety: ResultSafety,
    pub safety_notes: Vec<String>,
    pub stripped_tracking_params: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchReport {
    pub query: String,
    pub page: usize,
    pub requested_backends: usize,
    pub raw_results: usize,
    pub returned_results: usize,
    pub filtered_results: usize,
    pub duplicate_results: usize,
    pub cache_hit: bool,
}

#[derive(Clone)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub report: SearchReport,
}

struct SearchCache {
    ttl: Duration,
    max_entries: usize,
    state: Mutex<CacheState>,
}

struct CacheState {
    entries: HashMap<String, CacheEntry>,
    order: VecDeque<String>,
}

struct CacheEntry {
    results: Vec<SearchResult>,
    expires_at: Instant,
}

impl SearchCache {
    fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries: max_entries.max(1),
            state: Mutex::new(CacheState {
                entries: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    async fn get(&self, key: &str) -> Option<Vec<SearchResult>> {
        let mut state = self.state.lock().await;
        let now = Instant::now();
        if let Some(entry) = state.entries.get(key) {
            if now > entry.expires_at {
                state.entries.remove(key);
                if let Some(idx) = state.order.iter().position(|k| k == key) {
                    state.order.remove(idx);
                }
                return None;
            }
            let results = entry.results.clone();
            if let Some(idx) = state.order.iter().position(|k| k == key) {
                state.order.remove(idx);
            }
            state.order.push_back(key.to_owned());
            return Some(results);
        }
        None
    }

    async fn insert(&self, key: String, results: Vec<SearchResult>) {
        let mut state = self.state.lock().await;
        let expires_at = Instant::now() + self.ttl;
        state.entries.insert(key.clone(), CacheEntry { results, expires_at });
        if let Some(idx) = state.order.iter().position(|k| k == &key) {
            state.order.remove(idx);
        }
        state.order.push_back(key.clone());
        while state.order.len() > self.max_entries {
            if let Some(old_key) = state.order.pop_front() {
                state.entries.remove(&old_key);
            }
        }
    }
}

impl FortrustSearch {
    pub async fn new(config: SearchConfig) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("FortrustSearch/1.0")
            .timeout(std::time::Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::limited(3))
            .pool_idle_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        let cache = if config.cache_enabled && config.cache_max_entries > 0 && config.cache_ttl_secs > 0 {
            Some(Arc::new(SearchCache::new(
                Duration::from_secs(config.cache_ttl_secs),
                config.cache_max_entries,
            )))
        } else {
            None
        };
        Self { client, config, cache }
    }

    /// Fetch autocomplete suggestions for a partial query using DuckDuckGo's
    /// privacy-respecting autocomplete API. Returns up to 8 suggestions.
    pub async fn suggest(&self, query: &str) -> Vec<String> {
        let query = query.trim();
        if query.len() < 2 {
            return vec![];
        }
        let url = format!(
            "https://duckduckgo.com/ac/?q={}&type=list",
            urlencoding::encode(query)
        );
        let Ok(resp) = self
            .client
            .get(&url)
            .header("DNT", "1")
            .header("Sec-GPC", "1")
            .timeout(Duration::from_secs(2))
            .send()
            .await
        else {
            return vec![];
        };
        let Ok(json) = resp.json::<Vec<serde_json::Value>>().await else {
            return vec![];
        };
        json.iter()
            .filter_map(|item| item["phrase"].as_str().map(|s| s.to_owned()))
            .take(8)
            .collect()
    }

    pub async fn search(&self, query: &str, page: usize) -> Vec<SearchResult> {
        self.search_with_report(query, page).await.results
    }

    pub async fn search_with_report(&self, query: &str, page: usize) -> SearchResponse {
        let query = query.trim();
        let mut report = SearchReport {
            query: query.to_owned(),
            page,
            requested_backends: self.config.enabled_backends.len(),
            ..SearchReport::default()
        };
        if query.is_empty() {
            return SearchResponse { results: Vec::new(), report };
        }

        let cache_key = self.cache.as_ref().map(|_| self.cache_key(query, page));
        if let (Some(cache), Some(key)) = (&self.cache, cache_key.as_ref())
            && let Some(results) = cache.get(key).await {
                report.raw_results = results.len();
                report.returned_results = results.len();
                report.cache_hit = true;
                return SearchResponse { results, report };
            }

        let mut join_set = JoinSet::new();

        for backend in &self.config.enabled_backends {
            let client = self.client.clone();
            let query = query.to_owned();
            let backend = backend.clone();
            let max = self.config.max_results;
            let safe_search = self.config.safe_search;
            let language = self.config.language.clone();

            join_set.spawn(async move {
                match backend {
                    SearchBackend::BraveSearch => fetch_brave(&client, &query, max, page).await,
                    SearchBackend::DuckDuckGo => {
                        fetch_ddg(&client, &query, max, safe_search, &language, page).await
                    }
                    SearchBackend::Mojeek => fetch_mojeek(&client, &query, max, page).await,
                    SearchBackend::Stract => fetch_stract(&client, &query, max, page).await,
                    SearchBackend::Wikipedia => fetch_wikipedia(&client, &query).await,
                    SearchBackend::SearXNG(instance) => fetch_searxng(&client, &query, max, &instance, page, safe_search).await,
                }
            });
        }

        let mut all_results: Vec<SearchResult> = Vec::new();
        while let Some(Ok(mut results)) = join_set.join_next().await {
            all_results.append(&mut results);
        }

        report.raw_results = all_results.len();
        let before_safety = all_results.len();
        all_results = all_results
            .into_iter()
            .filter_map(|result| sanitize_result(result, &self.config))
            .collect();
        report.filtered_results = before_safety.saturating_sub(all_results.len());

        rank_results(&mut all_results);

        if self.config.deduplicate {
            let before_dedup = all_results.len();
            dedup_results(&mut all_results);
            report.duplicate_results = before_dedup.saturating_sub(all_results.len());
        }

        all_results.truncate(self.config.max_results);
        
        for r in &mut all_results {
            if r.archive_url.is_none() {
                r.archive_url = Some(format!("https://archive.is/latest/{}", urlencoding::encode(&r.url)));
            }
        }
        
        if let (Some(cache), Some(key)) = (&self.cache, cache_key) {
            cache.insert(key, all_results.clone()).await;
        }

        report.returned_results = all_results.len();
        SearchResponse { results: all_results, report }
    }
}

impl FortrustSearch {
    fn cache_key(&self, query: &str, page: usize) -> String {
        let backends = self
            .config
            .enabled_backends
            .iter()
            .map(search_backend_key)
            .collect::<Vec<_>>()
            .join(",");
        let safe = match self.config.safe_search {
            SafeSearchMode::Off => "off",
            SafeSearchMode::Moderate => "moderate",
            SafeSearchMode::Strict => "strict",
        };
        format!(
            "q={}|p={}|lang={}|safe={}|max={}|dedup={}|backends={}",
            query.trim().to_lowercase(),
            page,
            self.config.language.to_ascii_lowercase(),
            safe,
            self.config.max_results,
            self.config.deduplicate,
            backends
        )
    }
}

fn search_backend_key(backend: &SearchBackend) -> String {
    match backend {
        SearchBackend::BraveSearch => "brave".to_owned(),
        SearchBackend::Mojeek => "mojeek".to_owned(),
        SearchBackend::DuckDuckGo => "ddg".to_owned(),
        SearchBackend::Stract => "stract".to_owned(),
        SearchBackend::Wikipedia => "wikipedia".to_owned(),
        SearchBackend::SearXNG(instance) => format!("searxng:{instance}"),
    }
}

async fn fetch_ddg(
    client: &reqwest::Client,
    query: &str,
    max: usize,
    safe_search: SafeSearchMode,
    language: &str,
    page: usize,
) -> Vec<SearchResult> {
    let kp = match safe_search {
        SafeSearchMode::Off => "-2",
        SafeSearchMode::Moderate => "-1",
        SafeSearchMode::Strict => "1",
    };
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}&kp={}&kl={}&s={}",
        urlencoding::encode(query),
        kp,
        ddg_region(language),
        page.saturating_sub(1) * 30
    );

    let Ok(resp) = client
        .get(&url)
        .header("Accept-Language", language)
        .header("DNT", "1")
        .header("Sec-GPC", "1")
        .header("Cache-Control", "no-store")
        .send()
        .await
    else {
        return vec![];
    };

    let Ok(html) = resp.text().await else {
        return vec![];
    };
    parse_ddg_html(&html, max)
}

fn parse_ddg_html(html: &str, max: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    for chunk in html.split("result__title").skip(1) {
        if results.len() >= max {
            break;
        }

        let Some(anchor_start) = chunk.find("<a") else {
            continue;
        };
        let anchor = &chunk[anchor_start..];
        let Some(url) = extract_attr(anchor, "href") else {
            continue;
        };
        let Some(title_html) = between(anchor, ">", "</a>") else {
            continue;
        };

        let title = html_unescape(&strip_tags(title_html)).trim().to_owned();
        let url = unwrap_ddg_redirect(&html_unescape(&url));

        let snippet = chunk
            .split("class=\"result__snippet\">")
            .nth(1)
            .and_then(|s| s.split("</").next().or_else(|| s.split("<").next()))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();

        if !title.is_empty() && is_http_url(&url) {
            results.push(SearchResult {
                title,
                url,
                display_url: String::new(),
                host: String::new(),
                snippet: html_unescape(&snippet),
                source_backend: SearchBackend::DuckDuckGo,
                archive_url: None,
                relevance_score: 0.6,
                privacy_score: 0,
                safety: ResultSafety::Neutral,
                safety_notes: Vec::new(),
                stripped_tracking_params: 0,
            });
        }
    }

    results
}

async fn fetch_brave(client: &reqwest::Client, query: &str, max: usize, page: usize) -> Vec<SearchResult> {
    let Ok(token) = std::env::var("BRAVE_SEARCH_API_KEY") else {
        return Vec::new();
    };
    let token = token.trim();
    if token.is_empty() {
        return Vec::new();
    }

    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}&safesearch=off&offset={}",
        urlencoding::encode(query),
        max.min(20),
        page.saturating_sub(1)
    );

    let Ok(resp) = client
        .get(&url)
        .header("Accept", "application/json")
        .header("DNT", "1")
        .header("Sec-GPC", "1")
        .header("X-Subscription-Token", token)
        .send()
        .await
    else {
        return vec![];
    };

    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return vec![];
    };

    json["web"]["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(max)
                .filter_map(|r| {
                    Some(SearchResult {
                        title: r["title"].as_str()?.to_string(),
                        url: r["url"].as_str()?.to_string(),
                        display_url: String::new(),
                        host: String::new(),
                        snippet: r["description"].as_str().unwrap_or("").to_string(),
                        source_backend: SearchBackend::BraveSearch,
                        archive_url: None,
                        relevance_score: r["age"].as_str().map(|_| 0.8).unwrap_or(0.6),
                        privacy_score: 0,
                        safety: ResultSafety::Neutral,
                        safety_notes: Vec::new(),
                        stripped_tracking_params: 0,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_mojeek(client: &reqwest::Client, query: &str, max: usize, page: usize) -> Vec<SearchResult> {
    let url = format!(
        "https://api.mojeek.com/search?q={}&fmt=json&s={}&start={}",
        urlencoding::encode(query),
        max,
        (page.saturating_sub(1) * max) + 1
    );

    let Ok(resp) = client
        .get(&url)
        .header("DNT", "1")
        .header("Sec-GPC", "1")
        .send()
        .await
    else {
        return vec![];
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return vec![];
    };

    json["r"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(max)
                .filter_map(|r| {
                    Some(SearchResult {
                        title: r["t"].as_str()?.to_string(),
                        url: r["u"].as_str()?.to_string(),
                        display_url: String::new(),
                        host: String::new(),
                        snippet: r["s"].as_str().unwrap_or("").to_string(),
                        source_backend: SearchBackend::Mojeek,
                        archive_url: None,
                        relevance_score: 0.7,
                        privacy_score: 0,
                        safety: ResultSafety::Neutral,
                        safety_notes: Vec::new(),
                        stripped_tracking_params: 0,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_stract(client: &reqwest::Client, query: &str, max: usize, page: usize) -> Vec<SearchResult> {
    let url = format!(
        "https://stract.com/beta/api/search?query={}&num_results={}&page={}",
        urlencoding::encode(query),
        max,
        page
    );

    let Ok(resp) = client
        .get(&url)
        .header("DNT", "1")
        .header("Sec-GPC", "1")
        .send()
        .await
    else {
        return vec![];
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return vec![];
    };

    json["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(max)
                .filter_map(|r| {
                    Some(SearchResult {
                        title: r["title"].as_str()?.to_string(),
                        url: r["url"].as_str()?.to_string(),
                        display_url: String::new(),
                        host: String::new(),
                        snippet: r["snippet"].as_str().unwrap_or("").to_string(),
                        source_backend: SearchBackend::Stract,
                        archive_url: None,
                        relevance_score: 0.75,
                        privacy_score: 0,
                        safety: ResultSafety::Neutral,
                        safety_notes: Vec::new(),
                        stripped_tracking_params: 0,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_wikipedia(client: &reqwest::Client, query: &str) -> Vec<SearchResult> {
    let url = format!(
        "https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch={}&format=json&srlimit=3",
        urlencoding::encode(query)
    );

    let Ok(resp) = client
        .get(&url)
        .header("User-Agent", "FortrustSearch/1.0")
        .header("DNT", "1")
        .header("Sec-GPC", "1")
        .send()
        .await
    else {
        return vec![];
    };

    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return vec![];
    };

    json["query"]["search"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    Some(SearchResult {
                        title: r["title"].as_str()?.to_string(),
                        url: format!(
                            "https://en.wikipedia.org/wiki/{}",
                            urlencoding::encode(r["title"].as_str()?)
                        ),
                        display_url: String::new(),
                        host: String::new(),
                        snippet: r["snippet"].as_str().unwrap_or("").to_string(),
                        source_backend: SearchBackend::Wikipedia,
                        archive_url: None,
                        relevance_score: 0.5,
                        privacy_score: 0,
                        safety: ResultSafety::Neutral,
                        safety_notes: Vec::new(),
                        stripped_tracking_params: 0,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_searxng(
    client: &reqwest::Client,
    query: &str,
    max: usize,
    instance: &str,
    page: usize,
    safe_search: SafeSearchMode,
) -> Vec<SearchResult> {
    let safe_param = match safe_search {
        SafeSearchMode::Off => "0",
        SafeSearchMode::Moderate => "1",
        SafeSearchMode::Strict => "2",
    };
    
    let url = format!(
        "{}/search?q={}&format=json&pageno={}&safesearch={}",
        instance,
        urlencoding::encode(query),
        page,
        safe_param
    );

    let Ok(resp) = client
        .get(&url)
        .header("DNT", "1")
        .header("Sec-GPC", "1")
        .send()
        .await
    else {
        return vec![];
    };

    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return vec![];
    };

    json["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(max)
                .filter_map(|r| {
                    let instance_str = instance.to_string();
                    Some(SearchResult {
                        title: r["title"].as_str()?.to_string(),
                        url: r["url"].as_str()?.to_string(),
                        display_url: String::new(),
                        host: String::new(),
                        snippet: r["content"].as_str().unwrap_or("").to_string(),
                        source_backend: SearchBackend::SearXNG(instance_str),
                        archive_url: None,
                        relevance_score: r["score"].as_f64().map(|s| (s as f32) / 10.0).unwrap_or(0.7),
                        privacy_score: 0,
                        safety: ResultSafety::Neutral,
                        safety_notes: Vec::new(),
                        stripped_tracking_params: 0,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn sanitize_result(mut result: SearchResult, config: &SearchConfig) -> Option<SearchResult> {
    result.title = collapse_whitespace(&html_unescape(&strip_tags(&result.title)));
    result.snippet = collapse_whitespace(&html_unescape(&strip_tags(&result.snippet)));

    if result.title.is_empty() {
        return None;
    }

    let mut url = Url::parse(result.url.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }

    let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    let mut notes = Vec::new();
    let mut safety = ResultSafety::Trusted;
    let mut privacy_score: i32 = 100;

    if url.scheme() != "https" {
        notes.push("Plain HTTP result".to_owned());
        privacy_score -= 25;
        safety = ResultSafety::Warning;
        if config.block_non_https_results && config.strict_result_safety {
            return None;
        }
    }

    if is_ip_address_host(&host) {
        notes.push("IP-address host hidden from private search results".to_owned());
        privacy_score -= 35;
        safety = ResultSafety::Blocked;
        if config.block_ip_address_hosts && config.strict_result_safety {
            return None;
        }
    }

    if is_local_network_host(&host) {
        notes.push("Local-network host hidden from private search results".to_owned());
        privacy_score -= 45;
        safety = ResultSafety::Blocked;
        if config.block_local_network_hosts && config.strict_result_safety {
            return None;
        }
    }

    let stripped_tracking_params = if config.strip_result_tracking_params {
        strip_tracking_params_from_url(&mut url)
    } else {
        0
    };
    if stripped_tracking_params > 0 {
        notes.push(format!(
            "{stripped_tracking_params} tracking parameter{} stripped",
            if stripped_tracking_params == 1 { "" } else { "s" }
        ));
        privacy_score -= 2 * stripped_tracking_params as i32;
    }

    if host_has_suspicious_shape(&host) {
        notes.push("Unusual hostname shape".to_owned());
        privacy_score -= 15;
        if safety == ResultSafety::Trusted {
            safety = ResultSafety::Warning;
        }
    }

    if title_looks_like_ad(&result.title) || title_looks_like_ad(&result.snippet) {
        notes.push("Ad-like result text".to_owned());
        privacy_score -= 12;
        if safety == ResultSafety::Trusted {
            safety = ResultSafety::Warning;
        }
    }

    if notes.is_empty() {
        notes.push("HTTPS result with tracking cleanup applied".to_owned());
    }

    result.url = url.to_string();
    result.host = host.clone();
    result.display_url = display_url(&url);
    result.privacy_score = privacy_score.clamp(0, 100) as u8;
    result.safety = safety;
    result.safety_notes = notes;
    result.stripped_tracking_params = stripped_tracking_params;
    Some(result)
}

fn strip_tracking_params_from_url(url: &mut Url) -> usize {
    let Some(_) = url.query() else {
        return 0;
    };
    let original = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect::<Vec<_>>();
    let before = original.len();
    let kept = original
        .into_iter()
        .filter(|(key, _)| !is_tracking_query_param(key))
        .collect::<Vec<_>>();
    let stripped = before.saturating_sub(kept.len());
    if stripped > 0 {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }
    stripped
}

fn is_tracking_query_param(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("utm_")
        || matches!(
            key.as_str(),
            "fbclid"
                | "gclid"
                | "dclid"
                | "msclkid"
                | "mc_cid"
                | "mc_eid"
                | "igshid"
                | "vero_id"
                | "_hsenc"
                | "_hsmi"
                | "yclid"
                | "twclid"
                | "scid"
                | "rb_clickid"
        )
}

fn is_ip_address_host(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
        || (host.starts_with('[') && host.ends_with(']'))
}

fn is_local_network_host(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".local")
        || host.ends_with(".localhost")
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || is_private_172(host)
        || host == "::1"
        || host.eq_ignore_ascii_case("[::1]")
}

fn is_private_172(host: &str) -> bool {
    let mut parts = host.split('.');
    let Some("172") = parts.next() else {
        return false;
    };
    let Some(second) = parts.next().and_then(|part| part.parse::<u8>().ok()) else {
        return false;
    };
    (16..=31).contains(&second)
}

fn host_has_suspicious_shape(host: &str) -> bool {
    host.len() > 80
        || host.matches('-').count() > 6
        || host.split('.').any(|label| label.len() > 40)
        || host.contains("xn--")
}

fn title_looks_like_ad(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "sponsored",
        "advertisement",
        "coupon code",
        "limited time offer",
        "download now",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn display_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    let mut path = url.path().trim_end_matches('/').to_owned();
    if path.len() > 56 {
        path.truncate(56);
        path.push_str("...");
    }
    if path.is_empty() || path == "/" {
        host.to_owned()
    } else {
        format!("{host}{path}")
    }
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn title_similarity(a: &str, b: &str) -> f32 {
    let tokens_a: std::collections::HashSet<_> = a.to_lowercase().split_whitespace().map(|s| s.to_string()).collect();
    let tokens_b: std::collections::HashSet<_> = b.to_lowercase().split_whitespace().map(|s| s.to_string()).collect();
    if tokens_a.is_empty() && tokens_b.is_empty() { return 1.0; }
    if tokens_a.is_empty() || tokens_b.is_empty() { return 0.0; }
    let intersection = tokens_a.intersection(&tokens_b).count();
    let union = tokens_a.union(&tokens_b).count();
    intersection as f32 / union as f32
}

fn dedup_results(results: &mut Vec<SearchResult>) {
    let mut seen_urls = std::collections::HashSet::new();
    let mut retained: Vec<SearchResult> = Vec::new();
    
    for r in std::mem::take(results) {
        let normalized = normalize_url_for_dedup(&r.url);
        if seen_urls.insert(normalized) {
            let mut is_dup = false;
            for prev in &retained {
                if title_similarity(&r.title, &prev.title) > 0.85 {
                    is_dup = true;
                    break;
                }
            }
            if !is_dup {
                retained.push(r);
            }
        }
    }
    *results = retained;
}

fn normalize_url_for_dedup(url: &str) -> String {
    let Ok(mut u) = url::Url::parse(url) else {
        return url.to_lowercase();
    };
    u.set_fragment(None);
    let mut s = u.to_string();
    if s.ends_with('/') {
        s.pop();
    }
    s.replace("://www.", "://").to_lowercase()
}

fn ddg_region(language: &str) -> &'static str {
    match language.to_ascii_lowercase().as_str() {
        "en-us" => "us-en",
        "en-gb" => "uk-en",
        "en-ca" => "ca-en",
        "en-au" => "au-en",
        _ => "wt-wt",
    }
}

fn extract_attr(fragment: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    fragment
        .split(&needle)
        .nth(1)
        .and_then(|s| s.split('"').next())
        .map(str::to_owned)
}

fn between<'a>(input: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_idx = input.find(start)? + start.len();
    let end_idx = input[start_idx..].find(end)? + start_idx;
    Some(&input[start_idx..end_idx])
}

fn strip_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

fn unwrap_ddg_redirect(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url)
        && parsed.domain().is_some_and(|domain| domain.ends_with("duckduckgo.com"))
        && parsed.path().contains("/l/")
        && let Some(value) = parsed
            .query_pairs()
            .find_map(|(key, value)| (key == "uddg").then(|| value.into_owned()))
    {
        return value;
    }
    url.to_owned()
}

fn is_http_url(url: &str) -> bool {
    url::Url::parse(url)
        .map(|url| matches!(url.scheme(), "http" | "https"))
        .unwrap_or(false)
}

fn rank_results(results: &mut [SearchResult]) {
    let mut url_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for r in results.iter() {
        *url_counts
            .entry(normalize_url_for_dedup(&r.url))
            .or_insert(0) += 1;
    }

    for r in results.iter_mut() {
        let key = normalize_url_for_dedup(&r.url);
        let count = *url_counts.get(&key).unwrap_or(&1);
        r.relevance_score += (count as f32 - 1.0) * 0.15;
        r.relevance_score = r.relevance_score.min(1.0);
    }

    results.sort_by(|a, b| b.relevance_score.partial_cmp(&a.relevance_score).unwrap());
}

fn html_unescape(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddg_parser_strips_markup_and_unwraps_redirects() {
        let html = r#"
        <h2 class="result__title">
          <a class="result__a" href="https://duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fprivate%3Fq%3Done&amp;rut=abc">
            Fortrust <b>Privacy</b>
          </a>
        </h2>
        <a class="result__snippet">Private &amp; secure result.</a>
        "#;

        let results = parse_ddg_html(html, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Fortrust Privacy");
        assert_eq!(results[0].url, "https://example.com/private?q=one");
        assert_eq!(results[0].snippet, "Private & secure result.");
    }

    #[test]
    fn search_config_uses_no_key_backends_by_default() {
        let config = SearchConfig::default();
        assert!(config.enabled_backends.contains(&SearchBackend::DuckDuckGo));
        assert!(config.enabled_backends.contains(&SearchBackend::Wikipedia));
        assert!(!config.enabled_backends.contains(&SearchBackend::BraveSearch));
    }

    #[test]
    fn sanitizer_strips_tracking_params_and_scores_https_results() {
        let result = SearchResult {
            title: " Example   Result ".to_owned(),
            url: "https://example.com/article?utm_source=news&keep=1&fbclid=abc#section".to_owned(),
            display_url: String::new(),
            host: String::new(),
            archive_url: None,
            snippet: " Useful &amp; private ".to_owned(),
            source_backend: SearchBackend::DuckDuckGo,
            relevance_score: 0.5,
            privacy_score: 0,
            safety: ResultSafety::Neutral,
            safety_notes: Vec::new(),
            stripped_tracking_params: 0,
        };

        let sanitized = sanitize_result(result, &SearchConfig::default()).unwrap();
        assert_eq!(sanitized.host, "example.com");
        assert_eq!(sanitized.url, "https://example.com/article?keep=1#section");
        assert_eq!(sanitized.stripped_tracking_params, 2);
        assert_eq!(sanitized.safety, ResultSafety::Trusted);
        assert!(sanitized.privacy_score >= 90);
        assert!(sanitized.display_url.starts_with("example.com/article"));
    }

    #[test]
    fn sanitizer_blocks_local_and_ip_hosts_in_strict_mode() {
        let mut result = SearchResult {
            title: "Local admin".to_owned(),
            url: "https://127.0.0.1:8443/private".to_owned(),
            display_url: String::new(),
            host: String::new(),
            archive_url: None,
            snippet: String::new(),
            source_backend: SearchBackend::Stract,
            relevance_score: 0.5,
            privacy_score: 0,
            safety: ResultSafety::Neutral,
            safety_notes: Vec::new(),
            stripped_tracking_params: 0,
        };

        assert!(sanitize_result(result.clone(), &SearchConfig::default()).is_none());
        result.url = "https://192.168.1.1/router".to_owned();
        assert!(sanitize_result(result, &SearchConfig::default()).is_none());
    }

    #[test]
    fn sanitizer_can_warn_on_http_without_filtering() {
        let config = SearchConfig {
            block_non_https_results: false,
            strict_result_safety: false,
            ..SearchConfig::default()
        };
        let result = SearchResult {
            title: "HTTP Result".to_owned(),
            url: "http://example.org/".to_owned(),
            display_url: String::new(),
            host: String::new(),
            archive_url: None,
            snippet: String::new(),
            source_backend: SearchBackend::Mojeek,
            relevance_score: 0.5,
            privacy_score: 0,
            safety: ResultSafety::Neutral,
            safety_notes: Vec::new(),
            stripped_tracking_params: 0,
        };

        let sanitized = sanitize_result(result, &config).unwrap();
        assert_eq!(sanitized.safety, ResultSafety::Warning);
        assert!(sanitized.privacy_score < 100);
    }
}
