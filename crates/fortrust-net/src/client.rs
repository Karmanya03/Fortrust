use std::sync::Arc;
use std::time::SystemTime;

use bytes::Bytes;
use fortrust_core::{
    BlockReason, PrivacyEngine, PrivacyNote, RequestContext, RequestDecision, ResourceType,
};
use futures_util::{StreamExt, stream};
use http::{HeaderMap, HeaderName, HeaderValue};
use rustls::ClientConfig;
use url::Url;

use crate::cache::{CacheDecision, CacheEntry, CacheHeaders, HttpCache};
use crate::dns::DohResolverConfig;
use crate::tls::rustls_client_config;
use crate::transport::{
    HttpTransport, ReqwestTransport, TransportBodyStream, TransportError, TransportRequest,
    TransportResponse, TransportStreamResponse,
};

const DEFAULT_MAX_BUFFERED_BODY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct PreparedRequest {
    pub original_context: RequestContext,
    pub decision: RequestDecision,
    pub url: Url,
    pub headers: HeaderMap,
    pub cache_decision: CacheDecision,
    pub doh_endpoint: Url,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkError {
    Blocked(BlockReason),
    InvalidEffectiveUrl,
    BodyTooLarge { limit_bytes: usize },
    Transport(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchSource {
    Cache,
    Network,
    RevalidatedCache,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkResponse {
    pub url: Url,
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub source: FetchSource,
}

pub struct NetworkStreamResponse {
    pub url: Url,
    pub status: u16,
    pub headers: HeaderMap,
    pub body: TransportBodyStream,
    pub source: FetchSource,
}

pub struct NetworkClient {
    privacy: PrivacyEngine,
    cache: HttpCache,
    doh: DohResolverConfig,
    tls: Arc<ClientConfig>,
    transport: Arc<dyn HttpTransport>,
    max_buffered_body_bytes: usize,
}

impl Clone for NetworkClient {
    fn clone(&self) -> Self {
        Self {
            privacy: self.privacy.clone(),
            cache: self.cache.clone(),
            doh: self.doh.clone(),
            tls: Arc::clone(&self.tls),
            transport: Arc::clone(&self.transport),
            max_buffered_body_bytes: self.max_buffered_body_bytes,
        }
    }
}

impl NetworkClient {
    pub fn new(privacy: PrivacyEngine) -> Result<Self, NetworkError> {
        let transport = ReqwestTransport::new()
            .map_err(|error| NetworkError::Transport(format!("{error:?}")))?;
        Ok(Self {
            privacy,
            cache: HttpCache::default(),
            doh: DohResolverConfig::privacy_default(),
            tls: rustls_client_config(),
            transport: Arc::new(transport),
            max_buffered_body_bytes: DEFAULT_MAX_BUFFERED_BODY_BYTES,
        })
    }

    pub fn in_memory(privacy: PrivacyEngine) -> Self {
        Self {
            privacy,
            cache: HttpCache::default(),
            doh: DohResolverConfig::privacy_default(),
            tls: rustls_client_config(),
            transport: Arc::new(NoopTransport),
            max_buffered_body_bytes: DEFAULT_MAX_BUFFERED_BODY_BYTES,
        }
    }

    pub fn with_parts(
        privacy: PrivacyEngine,
        cache: HttpCache,
        doh: DohResolverConfig,
        tls: Arc<ClientConfig>,
        transport: Arc<dyn HttpTransport>,
    ) -> Self {
        Self {
            privacy,
            cache,
            doh,
            tls,
            transport,
            max_buffered_body_bytes: DEFAULT_MAX_BUFFERED_BODY_BYTES,
        }
    }

    pub fn prepare(&self, context: RequestContext) -> Result<PreparedRequest, NetworkError> {
        let decision = self.privacy.inspect(&context);
        if let Some(reason) = decision.blocked {
            return Err(NetworkError::Blocked(reason));
        }

        let effective = decision
            .effective_url
            .as_deref()
            .ok_or(NetworkError::InvalidEffectiveUrl)?;
        let url = Url::parse(effective).map_err(|_| NetworkError::InvalidEffectiveUrl)?;
        let cache_decision = self.cache.lookup(&url, SystemTime::now());

        let mut headers = privacy_headers(&decision.notes);
        add_fetch_metadata_headers(&mut headers, context.resource_type, decision.third_party);
        add_cache_validation_headers(&mut headers, &cache_decision);

        Ok(PreparedRequest {
            original_context: context,
            decision,
            url,
            headers,
            cache_decision,
            doh_endpoint: self.doh.endpoint().clone(),
        })
    }

    pub async fn fetch(
        &mut self,
        context: RequestContext,
    ) -> Result<NetworkResponse, NetworkError> {
        let prepared = self.prepare(context)?;

        if let CacheDecision::Fresh(entry) = &prepared.cache_decision {
            return Ok(response_from_cache_entry(
                prepared.url,
                entry.clone(),
                FetchSource::Cache,
            ));
        }

        let network = self.send_buffered(&prepared).await?;

        if network.status == 304
            && let CacheDecision::Revalidate(entry, _) = prepared.cache_decision
        {
            return Ok(response_from_cache_entry(
                prepared.url,
                entry,
                FetchSource::RevalidatedCache,
            ));
        }

        let response = response_from_transport(prepared.url.clone(), network);
        let _ = self.store_response(
            prepared.url,
            response.status,
            &response.headers,
            response.body.clone(),
            SystemTime::now(),
        );

        Ok(response)
    }

    pub async fn fetch_stream(
        &self,
        context: RequestContext,
    ) -> Result<NetworkStreamResponse, NetworkError> {
        let prepared = self.prepare(context)?;

        if let CacheDecision::Fresh(entry) = &prepared.cache_decision {
            return Ok(stream_response_from_cache_entry(
                prepared.url,
                entry.clone(),
                FetchSource::Cache,
            ));
        }

        let network = self
            .transport
            .send_stream(TransportRequest {
                url: prepared.url.clone(),
                headers: prepared.headers.clone(),
            })
            .await
            .map_err(NetworkError::from)?;

        if network.status == 304
            && let CacheDecision::Revalidate(entry, _) = prepared.cache_decision
        {
            return Ok(stream_response_from_cache_entry(
                prepared.url,
                entry,
                FetchSource::RevalidatedCache,
            ));
        }

        Ok(stream_response_from_transport(prepared.url, network))
    }

    async fn send_buffered(
        &self,
        prepared: &PreparedRequest,
    ) -> Result<TransportResponse, NetworkError> {
        let response = self
            .transport
            .send_stream(TransportRequest {
                url: prepared.url.clone(),
                headers: prepared.headers.clone(),
            })
            .await
            .map_err(NetworkError::from)?;

        let status = response.status;
        let headers = response.headers;
        let body = collect_body_limited(response.body, self.max_buffered_body_bytes).await?;

        Ok(TransportResponse {
            status,
            headers,
            body,
        })
    }

    pub fn store_response(
        &mut self,
        url: Url,
        status: u16,
        headers: &HeaderMap,
        body: Bytes,
        now: SystemTime,
    ) -> bool {
        let cache_headers = CacheHeaders::from_headers(headers);
        let Some(entry) = CacheEntry::new(url, status, cache_headers, body, now) else {
            return false;
        };

        self.cache.store(entry);
        true
    }

    pub fn cache(&self) -> &HttpCache {
        &self.cache
    }

    pub fn doh(&self) -> &DohResolverConfig {
        &self.doh
    }

    pub fn tls_config(&self) -> &Arc<ClientConfig> {
        &self.tls
    }

    pub fn max_buffered_body_bytes(&self) -> usize {
        self.max_buffered_body_bytes
    }
}

impl From<TransportError> for NetworkError {
    fn from(error: TransportError) -> Self {
        Self::Transport(format!("{error:?}"))
    }
}

fn privacy_headers(notes: &[PrivacyNote]) -> HeaderMap {
    let mut headers = HeaderMap::new();

    if notes.contains(&PrivacyNote::GlobalPrivacyControl) {
        headers.insert(
            HeaderName::from_static("sec-gpc"),
            HeaderValue::from_static("1"),
        );
    }

    if notes.contains(&PrivacyNote::DoNotTrack) {
        headers.insert(
            HeaderName::from_static("dnt"),
            HeaderValue::from_static("1"),
        );
    }

    headers
}

fn add_fetch_metadata_headers(
    headers: &mut HeaderMap,
    resource_type: ResourceType,
    third_party: bool,
) {
    headers.insert(
        HeaderName::from_static("sec-fetch-site"),
        HeaderValue::from_static(if third_party {
            "cross-site"
        } else {
            "same-origin"
        }),
    );

    let destination = match resource_type {
        ResourceType::Document => "document",
        ResourceType::Script => "script",
        ResourceType::Image => "image",
        ResourceType::Stylesheet => "style",
        ResourceType::Xhr => "empty",
        ResourceType::Media => "video",
        ResourceType::Font => "font",
        ResourceType::Other => "empty",
    };
    headers.insert(
        HeaderName::from_static("sec-fetch-dest"),
        HeaderValue::from_static(destination),
    );
}

fn add_cache_validation_headers(headers: &mut HeaderMap, cache_decision: &CacheDecision) {
    let CacheDecision::Revalidate(_, validation) = cache_decision else {
        return;
    };

    if let Some(etag) = &validation.if_none_match
        && let Ok(value) = HeaderValue::from_str(etag)
    {
        headers.insert(HeaderName::from_static("if-none-match"), value);
    }

    if let Some(last_modified) = &validation.if_modified_since
        && let Ok(value) = HeaderValue::from_str(last_modified)
    {
        headers.insert(HeaderName::from_static("if-modified-since"), value);
    }
}

fn response_from_cache_entry(url: Url, entry: CacheEntry, source: FetchSource) -> NetworkResponse {
    let mut headers = HeaderMap::new();
    if let Some(etag) = entry.headers.etag
        && let Ok(value) = HeaderValue::from_str(&etag)
    {
        headers.insert(HeaderName::from_static("etag"), value);
    }
    if let Some(last_modified) = entry.headers.last_modified
        && let Ok(value) = HeaderValue::from_str(&last_modified)
    {
        headers.insert(HeaderName::from_static("last-modified"), value);
    }
    if let Some(cache_control) = entry.headers.cache_control
        && let Ok(value) = HeaderValue::from_str(&cache_control)
    {
        headers.insert(HeaderName::from_static("cache-control"), value);
    }
    if let Some(vary) = entry.headers.vary
        && let Ok(value) = HeaderValue::from_str(&vary)
    {
        headers.insert(HeaderName::from_static("vary"), value);
    }

    NetworkResponse {
        url,
        status: entry.status,
        headers,
        body: entry.body,
        source,
    }
}

fn response_from_transport(url: Url, response: TransportResponse) -> NetworkResponse {
    NetworkResponse {
        url,
        status: response.status,
        headers: response.headers,
        body: response.body,
        source: FetchSource::Network,
    }
}

fn stream_response_from_cache_entry(
    url: Url,
    entry: CacheEntry,
    source: FetchSource,
) -> NetworkStreamResponse {
    let response = response_from_cache_entry(url, entry, source);
    NetworkStreamResponse {
        url: response.url,
        status: response.status,
        headers: response.headers,
        body: Box::pin(stream::once(async move { Ok(response.body) })),
        source: response.source,
    }
}

fn stream_response_from_transport(
    url: Url,
    response: TransportStreamResponse,
) -> NetworkStreamResponse {
    NetworkStreamResponse {
        url,
        status: response.status,
        headers: response.headers,
        body: response.body,
        source: FetchSource::Network,
    }
}

async fn collect_body_limited(
    mut body: TransportBodyStream,
    limit_bytes: usize,
) -> Result<Bytes, NetworkError> {
    let mut chunks = Vec::new();
    let mut total = 0usize;

    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(NetworkError::from)?;
        total = total.saturating_add(chunk.len());
        if total > limit_bytes {
            return Err(NetworkError::BodyTooLarge { limit_bytes });
        }
        chunks.push(chunk);
    }

    let mut merged = Vec::with_capacity(total);
    for chunk in chunks {
        merged.extend_from_slice(&chunk);
    }

    Ok(Bytes::from(merged))
}

struct NoopTransport;

impl HttpTransport for NoopTransport {
    fn send<'a>(
        &'a self,
        _request: TransportRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<TransportResponse, TransportError>> + Send + 'a,
        >,
    > {
        Box::pin(async {
            Err(TransportError::Request(
                "no transport installed for this client".to_owned(),
            ))
        })
    }

    fn send_stream<'a>(
        &'a self,
        _request: TransportRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<TransportStreamResponse, TransportError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Err(TransportError::Request(
                "no transport installed for this client".to_owned(),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::time::Duration;

    use fortrust_core::{PrivacyConfig, ResourceType};
    use futures_util::stream;
    use tokio::sync::Mutex;

    use super::*;

    #[test]
    fn prepare_blocks_before_network_metadata_is_built() {
        let client = NetworkClient::in_memory(PrivacyEngine::new(PrivacyConfig::default()));
        let blocked = client
            .prepare(RequestContext {
                url: "https://stats.google-analytics.com/collect".to_owned(),
                top_level_url: Some("https://example.com".to_owned()),
                resource_type: ResourceType::Script,
            })
            .unwrap_err();

        assert_eq!(blocked, NetworkError::Blocked(BlockReason::TrackerDomain));
    }

    #[test]
    fn prepare_applies_privacy_url_and_headers() {
        let client = NetworkClient::in_memory(PrivacyEngine::new(PrivacyConfig::default()));
        let prepared = client
            .prepare(RequestContext::document(
                "http://example.com/?utm_campaign=sale&q=boots",
            ))
            .unwrap();

        assert_eq!(prepared.url.as_str(), "https://example.com/?q=boots");
        assert_eq!(prepared.headers.get("sec-gpc").unwrap(), "1");
        assert_eq!(prepared.headers.get("dnt").unwrap(), "1");
        assert_eq!(prepared.headers.get("sec-fetch-dest").unwrap(), "document");
    }

    #[test]
    fn stale_cache_entries_add_conditional_request_headers() {
        let privacy = PrivacyEngine::new(PrivacyConfig::default());
        let mut cache = HttpCache::default();
        let url = Url::parse("https://example.com/app.js").unwrap();
        cache.store(
            CacheEntry::new(
                url.clone(),
                200,
                CacheHeaders {
                    etag: Some("\"v1\"".to_owned()),
                    last_modified: Some("Sat, 23 May 2026 12:00:00 GMT".to_owned()),
                    cache_control: Some("max-age=1".to_owned()),
                    vary: None,
                },
                Bytes::from_static(b"console.log(1)"),
                SystemTime::now() - Duration::from_secs(10),
            )
            .unwrap(),
        );
        let client = NetworkClient::with_parts(
            privacy,
            cache,
            DohResolverConfig::privacy_default(),
            rustls_client_config(),
            Arc::new(NoopTransport),
        );

        let prepared = client
            .prepare(RequestContext {
                url: url.to_string(),
                top_level_url: Some("https://example.com".to_owned()),
                resource_type: ResourceType::Script,
            })
            .unwrap();

        assert_eq!(prepared.headers.get("if-none-match").unwrap(), "\"v1\"");
        assert_eq!(
            prepared.headers.get("if-modified-since").unwrap(),
            "Sat, 23 May 2026 12:00:00 GMT"
        );
    }

    #[tokio::test]
    async fn fetch_serves_fresh_cache_without_transport() {
        let privacy = PrivacyEngine::new(PrivacyConfig::default());
        let mut cache = HttpCache::default();
        let url = Url::parse("https://example.com/app.css").unwrap();
        cache.store(
            CacheEntry::new(
                url.clone(),
                200,
                CacheHeaders {
                    etag: Some("\"v1\"".to_owned()),
                    last_modified: None,
                    cache_control: Some("max-age=60".to_owned()),
                    vary: None,
                },
                Bytes::from_static(b"cached"),
                SystemTime::now(),
            )
            .unwrap(),
        );
        let transport = Arc::new(RecordingTransport::new(vec![TransportResponse {
            status: 500,
            headers: HeaderMap::new(),
            body: Bytes::from_static(b"should not be used"),
        }]));
        let mut client = NetworkClient::with_parts(
            privacy,
            cache,
            DohResolverConfig::privacy_default(),
            rustls_client_config(),
            transport.clone(),
        );

        let response = client
            .fetch(RequestContext {
                url: url.to_string(),
                top_level_url: Some("https://example.com".to_owned()),
                resource_type: ResourceType::Stylesheet,
            })
            .await
            .unwrap();

        assert_eq!(response.source, FetchSource::Cache);
        assert_eq!(response.body, Bytes::from_static(b"cached"));
        assert_eq!(transport.sent_count().await, 0);
    }

    #[tokio::test]
    async fn fetch_reuses_stale_cache_on_not_modified() {
        let privacy = PrivacyEngine::new(PrivacyConfig::default());
        let mut cache = HttpCache::default();
        let url = Url::parse("https://example.com/app.js").unwrap();
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
                Bytes::from_static(b"cached-js"),
                SystemTime::UNIX_EPOCH,
            )
            .unwrap(),
        );
        let transport = Arc::new(RecordingTransport::new(vec![TransportResponse {
            status: 304,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        }]));
        let mut client = NetworkClient::with_parts(
            privacy,
            cache,
            DohResolverConfig::privacy_default(),
            rustls_client_config(),
            transport.clone(),
        );

        let response = client
            .fetch(RequestContext {
                url: url.to_string(),
                top_level_url: Some("https://example.com".to_owned()),
                resource_type: ResourceType::Script,
            })
            .await
            .unwrap();

        assert_eq!(response.source, FetchSource::RevalidatedCache);
        assert_eq!(response.body, Bytes::from_static(b"cached-js"));
        let sent = transport.sent_requests().await;
        assert_eq!(sent[0].headers.get("if-none-match").unwrap(), "\"v1\"");
    }

    #[tokio::test]
    async fn fetch_stores_cacheable_network_response() {
        let privacy = PrivacyEngine::new(PrivacyConfig::default());
        let mut headers = HeaderMap::new();
        headers.insert("cache-control", HeaderValue::from_static("max-age=60"));
        headers.insert("etag", HeaderValue::from_static("\"network\""));
        let transport = Arc::new(RecordingTransport::new(vec![TransportResponse {
            status: 200,
            headers,
            body: Bytes::from_static(b"network-body"),
        }]));
        let mut client = NetworkClient::with_parts(
            privacy,
            HttpCache::default(),
            DohResolverConfig::privacy_default(),
            rustls_client_config(),
            transport,
        );

        let response = client
            .fetch(RequestContext::document("https://example.com/index.html"))
            .await
            .unwrap();

        assert_eq!(response.source, FetchSource::Network);
        assert_eq!(response.body, Bytes::from_static(b"network-body"));
        assert_eq!(client.cache().len(), 1);
    }

    #[tokio::test]
    async fn fetch_stream_exposes_network_body_without_buffering() {
        let privacy = PrivacyEngine::new(PrivacyConfig::default());
        let transport = Arc::new(RecordingTransport::new(vec![TransportResponse {
            status: 200,
            headers: HeaderMap::new(),
            body: Bytes::from_static(b"stream-body"),
        }]));
        let client = NetworkClient::with_parts(
            privacy,
            HttpCache::default(),
            DohResolverConfig::privacy_default(),
            rustls_client_config(),
            transport,
        );

        let mut response = client
            .fetch_stream(RequestContext::document("https://example.com/index.html"))
            .await
            .unwrap();

        assert_eq!(response.source, FetchSource::Network);
        assert_eq!(
            response.body.next().await.unwrap().unwrap(),
            Bytes::from_static(b"stream-body")
        );
        assert!(response.body.next().await.is_none());
    }

    struct RecordingTransport {
        responses: Mutex<Vec<TransportResponse>>,
        requests: Mutex<Vec<TransportRequest>>,
    }

    impl RecordingTransport {
        fn new(mut responses: Vec<TransportResponse>) -> Self {
            responses.reverse();
            Self {
                responses: Mutex::new(responses),
                requests: Mutex::new(Vec::new()),
            }
        }

        async fn sent_count(&self) -> usize {
            self.requests.lock().await.len()
        }

        async fn sent_requests(&self) -> Vec<TransportRequest> {
            self.requests.lock().await.clone()
        }
    }

    impl HttpTransport for RecordingTransport {
        fn send<'a>(
            &'a self,
            request: TransportRequest,
        ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, TransportError>> + Send + 'a>>
        {
            Box::pin(async move {
                self.requests.lock().await.push(request);
                self.responses
                    .lock()
                    .await
                    .pop()
                    .ok_or_else(|| TransportError::Request("missing test response".to_owned()))
            })
        }

        fn send_stream<'a>(
            &'a self,
            request: TransportRequest,
        ) -> Pin<
            Box<dyn Future<Output = Result<TransportStreamResponse, TransportError>> + Send + 'a>,
        > {
            Box::pin(async move {
                let response = self.send(request).await?;
                Ok(TransportStreamResponse {
                    status: response.status,
                    headers: response.headers,
                    body: Box::pin(stream::once(async move { Ok(response.body) })),
                })
            })
        }
    }
}
