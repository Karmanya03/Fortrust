use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use http::HeaderMap;
use url::Url;

pub type TransportBodyStream = Pin<Box<dyn Stream<Item = Result<Bytes, TransportError>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportRequest {
    pub url: Url,
    pub headers: HeaderMap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Bytes,
}

pub struct TransportStreamResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: TransportBodyStream,
}

#[derive(Debug)]
pub enum TransportError {
    Builder(String),
    Request(String),
}

pub trait HttpTransport: Send + Sync {
    fn send<'a>(
        &'a self,
        request: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, TransportError>> + Send + 'a>>;

    fn send_stream<'a>(
        &'a self,
        request: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TransportStreamResponse, TransportError>> + Send + 'a>>;
}

#[derive(Debug, Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .https_only(true)
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .tcp_nodelay(true)
            .user_agent("Fortrust/0.1 Trust Engine")
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .map_err(|error| TransportError::Builder(error.to_string()))?;

        Ok(Self { client })
    }
}

impl HttpTransport for ReqwestTransport {
    fn send<'a>(
        &'a self,
        request: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, TransportError>> + Send + 'a>> {
        Box::pin(async move {
            let mut response = self.send_stream(request).await?;
            let mut body = Vec::new();
            while let Some(chunk) = response.body.next().await {
                body.extend_from_slice(&chunk?);
            }

            Ok(TransportResponse {
                status: response.status,
                headers: response.headers,
                body: Bytes::from(body),
            })
        })
    }

    fn send_stream<'a>(
        &'a self,
        request: TransportRequest,
    ) -> Pin<Box<dyn Future<Output = Result<TransportStreamResponse, TransportError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut builder = self.client.get(request.url.clone());
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }

            let response = builder
                .send()
                .await
                .map_err(|error| TransportError::Request(error.to_string()))?;
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            let body =
                Box::pin(response.bytes_stream().map(|chunk| {
                    chunk.map_err(|error| TransportError::Request(error.to_string()))
                }));

            Ok(TransportStreamResponse {
                status,
                headers,
                body,
            })
        })
    }
}
