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
            enabled_backends: vec![
                SearchBackend::DuckDuckGo,
                SearchBackend::Stract,
                SearchBackend::Wikipedia,
            ],
            max_results: 10,
            safe_search: SafeSearchMode::Moderate,
            language: "en-US".to_owned(),
            deduplicate: true,
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
            .user_agent("FortrustSearch/1.0")
            .timeout(std::time::Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::limited(3))
            .pool_idle_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap();
        Self { client, config }
    }

    pub async fn search(&self, query: &str) -> Vec<SearchResult> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
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
                    SearchBackend::BraveSearch => fetch_brave(&client, &query, max).await,
                    SearchBackend::DuckDuckGo => {
                        fetch_ddg(&client, &query, max, safe_search, &language).await
                    }
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

async fn fetch_ddg(
    client: &reqwest::Client,
    query: &str,
    max: usize,
    safe_search: SafeSearchMode,
    language: &str,
) -> Vec<SearchResult> {
    let kp = match safe_search {
        SafeSearchMode::Off => "-2",
        SafeSearchMode::Moderate => "-1",
        SafeSearchMode::Strict => "1",
    };
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}&kp={}&kl={}",
        urlencoding::encode(query),
        kp,
        ddg_region(language)
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
                snippet: html_unescape(&snippet),
                source_backend: SearchBackend::DuckDuckGo,
                relevance_score: 0.6,
            });
        }
    }

    results
}

async fn fetch_brave(client: &reqwest::Client, query: &str, max: usize) -> Vec<SearchResult> {
    let Ok(token) = std::env::var("BRAVE_SEARCH_API_KEY") else {
        return Vec::new();
    };
    let token = token.trim();
    if token.is_empty() {
        return Vec::new();
    }

    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}&safesearch=off",
        urlencoding::encode(query),
        max.min(20)
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
}
