use tokio::task::JoinSet;

pub struct FortrustSearch {
    client: reqwest::Client,
    pub config: SearchConfig,
}

pub struct SearchConfig {
    pub enabled_backends: Vec<SearchBackend>,
    pub max_results: usize,
    pub safe_search: SafeSearchMode,
    pub language: String,
    pub deduplicate: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            enabled_backends: vec![SearchBackend::DuckDuckGo, SearchBackend::BraveSearch],
            max_results: 10,
            safe_search: SafeSearchMode::Moderate,
            language: "en-US".to_owned(),
            deduplicate: true,
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum SearchBackend {
    BraveSearch,
    Mojeek,
    DuckDuckGo,
    Stract,
    Wikipedia,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SafeSearchMode {
    Off,
    Moderate,
    Strict,
}

#[derive(Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source_backend: SearchBackend,
    pub relevance_score: f32,
}

impl FortrustSearch {
    pub async fn new(config: SearchConfig) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Fortrust/1.0 (+https://fortrust.browser)")
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        Self { client, config }
    }

    pub async fn search(&self, query: &str) -> Vec<SearchResult> {
        let mut join_set = JoinSet::new();

        for backend in &self.config.enabled_backends {
            let client = self.client.clone();
            let query = query.to_string();
            let backend = backend.clone();
            let max = self.config.max_results;

            join_set.spawn(async move {
                match backend {
                    SearchBackend::BraveSearch => fetch_brave(&client, &query, max).await,
                    SearchBackend::DuckDuckGo => fetch_ddg(&client, &query, max).await,
                    SearchBackend::Mojeek => fetch_mojeek(&client, &query, max).await,
                    SearchBackend::Stract => fetch_stract(&client, &query, max).await,
                    SearchBackend::Wikipedia => fetch_wikipedia(&client, &query).await,
                }
            });
        }

        let mut all_results: Vec<SearchResult> = Vec::new();
        while let Some(Ok(mut results)) = join_set.join_next().await {
            all_results.append(&mut results);
        }

        if self.config.deduplicate {
            dedup_results(&mut all_results);
        }

        rank_results(&mut all_results);

        all_results.truncate(self.config.max_results);
        all_results
    }
}

async fn fetch_ddg(client: &reqwest::Client, query: &str, max: usize) -> Vec<SearchResult> {
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}&kp=-1&kl=wt-wt",
        urlencoding::encode(query)
    );

    let Ok(resp) = client
        .get(&url)
        .header("Accept-Language", "en-US,en;q=0.9")
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

    // Simple regex-based DDG HTML parsing
    // In production, use html5ever for proper parsing
    for chunk in html.split("<h2 class=\"result__title\">").skip(1) {
        if results.len() >= max {
            break;
        }

        let title = chunk
            .split('>')
            .nth(1)
            .and_then(|s| s.split('<').next())
            .unwrap_or("")
            .to_owned();

        let url = chunk
            .split("href=\"")
            .nth(1)
            .and_then(|s| s.split('\"').next())
            .unwrap_or("")
            .to_owned();

        let snippet = chunk
            .split("class=\"result__snippet\">")
            .nth(1)
            .and_then(|s| s.split("</").next().or_else(|| s.split("<").next()))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();

        if !title.is_empty() {
            results.push(SearchResult {
                title,
                url: html_unescape(&url),
                snippet: html_unescape(&snippet),
                source_backend: SearchBackend::DuckDuckGo,
                relevance_score: 0.6,
            });
        }
    }

    results
}

async fn fetch_brave(client: &reqwest::Client, query: &str, max: usize) -> Vec<SearchResult> {
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}&safesearch=off",
        urlencoding::encode(query),
        max.min(20)
    );

    let Ok(resp) = client
        .get(&url)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", "")
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
                        snippet: r["description"].as_str().unwrap_or("").to_string(),
                        source_backend: SearchBackend::BraveSearch,
                        relevance_score: r["age"].as_str().map(|_| 0.8).unwrap_or(0.6),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_mojeek(client: &reqwest::Client, query: &str, max: usize) -> Vec<SearchResult> {
    let url = format!(
        "https://api.mojeek.com/search?q={}&fmt=json&s={}",
        urlencoding::encode(query),
        max
    );

    let Ok(resp) = client.get(&url).send().await else {
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
                        snippet: r["s"].as_str().unwrap_or("").to_string(),
                        source_backend: SearchBackend::Mojeek,
                        relevance_score: 0.7,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn fetch_stract(client: &reqwest::Client, query: &str, max: usize) -> Vec<SearchResult> {
    let url = format!(
        "https://stract.com/beta/api/search?query={}&num_results={}",
        urlencoding::encode(query),
        max
    );

    let Ok(resp) = client.get(&url).send().await else {
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
                        snippet: r["snippet"].as_str().unwrap_or("").to_string(),
                        source_backend: SearchBackend::Stract,
                        relevance_score: 0.75,
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
        .header("User-Agent", "Fortrust/1.0")
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
                        snippet: r["snippet"].as_str().unwrap_or("").to_string(),
                        source_backend: SearchBackend::Wikipedia,
                        relevance_score: 0.5,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn dedup_results(results: &mut Vec<SearchResult>) {
    let mut seen = std::collections::HashSet::new();
    results.retain(|r| {
        let normalized = normalize_url_for_dedup(&r.url);
        seen.insert(normalized)
    });
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
