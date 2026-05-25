use std::time::{Duration, SystemTime};

use bytes::Bytes;
use http::HeaderMap;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheHeaders {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub cache_control: Option<String>,
    pub vary: Option<String>,
}

impl CacheHeaders {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            etag: header_value(headers, "etag"),
            last_modified: header_value(headers, "last-modified"),
            cache_control: header_value(headers, "cache-control"),
            vary: header_value(headers, "vary"),
        }
    }

    pub fn is_no_store(&self) -> bool {
        cache_control_tokens(self.cache_control.as_deref())
            .any(|token| token.eq_ignore_ascii_case("no-store"))
    }

    pub fn requires_validation(&self) -> bool {
        cache_control_tokens(self.cache_control.as_deref()).any(|token| {
            token.eq_ignore_ascii_case("no-cache") || token.eq_ignore_ascii_case("must-revalidate")
        })
    }

    pub fn max_age(&self) -> Option<Duration> {
        cache_control_tokens(self.cache_control.as_deref()).find_map(|token| {
            let (name, value) = token.split_once('=')?;
            if !name.trim().eq_ignore_ascii_case("max-age") {
                return None;
            }

            value
                .trim()
                .trim_matches('"')
                .parse::<u64>()
                .ok()
                .map(Duration::from_secs)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationHeaders {
    pub if_none_match: Option<String>,
    pub if_modified_since: Option<String>,
}

impl ValidationHeaders {
    pub fn is_empty(&self) -> bool {
        self.if_none_match.is_none() && self.if_modified_since.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheEntry {
    pub url: Url,
    pub status: u16,
    pub headers: CacheHeaders,
    pub body: Bytes,
    pub stored_at: SystemTime,
}

impl CacheEntry {
    pub fn new(
        url: Url,
        status: u16,
        headers: CacheHeaders,
        body: Bytes,
        stored_at: SystemTime,
    ) -> Option<Self> {
        if !is_cacheable_status(status) || headers.is_no_store() {
            return None;
        }

        Some(Self {
            url,
            status,
            headers,
            body,
            stored_at,
        })
    }

    pub fn is_fresh(&self, now: SystemTime) -> bool {
        if self.headers.requires_validation() {
            return false;
        }

        let Some(max_age) = self.headers.max_age() else {
            return false;
        };

        now.duration_since(self.stored_at)
            .is_ok_and(|age| age < max_age)
    }

    pub fn validation_headers(&self) -> ValidationHeaders {
        ValidationHeaders {
            if_none_match: self.headers.etag.clone(),
            if_modified_since: self.headers.last_modified.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheDecision {
    Miss,
    Fresh(CacheEntry),
    Revalidate(CacheEntry, ValidationHeaders),
}

#[derive(Debug, Clone, Default)]
pub struct HttpCache {
    entries: Vec<CacheEntry>,
}

impl HttpCache {
    pub fn lookup(&self, url: &Url, now: SystemTime) -> CacheDecision {
        let Some(entry) = self.entries.iter().find(|entry| &entry.url == url) else {
            return CacheDecision::Miss;
        };

        if entry.is_fresh(now) {
            CacheDecision::Fresh(entry.clone())
        } else {
            CacheDecision::Revalidate(entry.clone(), entry.validation_headers())
        }
    }

    pub fn store(&mut self, entry: CacheEntry) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|stored| stored.url == entry.url)
        {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn header_value(headers: &HeaderMap, name: &'static str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn cache_control_tokens(value: Option<&str>) -> impl Iterator<Item = &str> {
    value
        .into_iter()
        .flat_map(|header| header.split(','))
        .map(str::trim)
}

fn is_cacheable_status(status: u16) -> bool {
    matches!(
        status,
        200 | 203 | 204 | 206 | 300 | 301 | 404 | 405 | 410 | 414 | 501
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_max_age_response_can_be_served_without_network() {
        let url = Url::parse("https://example.com/app.css").unwrap();
        let entry = CacheEntry::new(
            url,
            200,
            CacheHeaders {
                etag: Some("\"abc\"".to_owned()),
                last_modified: None,
                cache_control: Some("max-age=60".to_owned()),
                vary: None,
            },
            Bytes::from_static(b"body"),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        assert!(entry.is_fresh(SystemTime::UNIX_EPOCH + Duration::from_secs(30)));
        assert!(!entry.is_fresh(SystemTime::UNIX_EPOCH + Duration::from_secs(61)));
    }

    #[test]
    fn stale_entries_emit_validation_headers() {
        let url = Url::parse("https://example.com/script.js").unwrap();
        let entry = CacheEntry::new(
            url,
            200,
            CacheHeaders {
                etag: Some("\"v1\"".to_owned()),
                last_modified: Some("Sat, 23 May 2026 12:00:00 GMT".to_owned()),
                cache_control: Some("max-age=0".to_owned()),
                vary: None,
            },
            Bytes::new(),
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();

        let headers = entry.validation_headers();
        assert_eq!(headers.if_none_match.as_deref(), Some("\"v1\""));
        assert_eq!(
            headers.if_modified_since.as_deref(),
            Some("Sat, 23 May 2026 12:00:00 GMT")
        );
    }

    #[test]
    fn stale_cache_lookup_keeps_entry_for_not_modified_response() {
        let mut cache = HttpCache::default();
        let url = Url::parse("https://example.com/script.js").unwrap();
        cache.store(
            CacheEntry::new(
                url.clone(),
                200,
                CacheHeaders {
                    etag: Some("\"v1\"".to_owned()),
                    last_modified: None,
                    cache_control: Some("max-age=0".to_owned()),
                    vary: None,
                },
                Bytes::from_static(b"cached"),
                SystemTime::UNIX_EPOCH,
            )
            .unwrap(),
        );

        let decision = cache.lookup(&url, SystemTime::UNIX_EPOCH + Duration::from_secs(10));
        let CacheDecision::Revalidate(entry, headers) = decision else {
            panic!("stale entry should require validation");
        };

        assert_eq!(entry.body, Bytes::from_static(b"cached"));
        assert_eq!(headers.if_none_match.as_deref(), Some("\"v1\""));
    }

    #[test]
    fn no_store_responses_are_not_cached() {
        let entry = CacheEntry::new(
            Url::parse("https://example.com/private").unwrap(),
            200,
            CacheHeaders {
                etag: None,
                last_modified: None,
                cache_control: Some("private, no-store".to_owned()),
                vary: None,
            },
            Bytes::new(),
            SystemTime::UNIX_EPOCH,
        );

        assert!(entry.is_none());
    }
}
