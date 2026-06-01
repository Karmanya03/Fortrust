use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use fortrust_core::{RequestContext, ResourceType};
use fortrust_ipc::{MessageReceiver, MessageSender, NetProcessCommand, NetProcessEvent};
use tokio::net::TcpStream;
use url::Url;

use crate::{FetchSource, NetworkError, NetworkResponse};

/// A network client that routes all fetch requests through the external `netproc` binary
/// via TCP IPC. Each `NetprocClient` owns one TCP connection and sends/receives framed
/// `NetProcessCommand` / `NetProcessEvent` messages.
pub struct NetprocClient {
    sender: MessageSender,
    receiver: MessageReceiver,
    next_id: AtomicU64,
}

impl NetprocClient {
    /// Connect to a running netproc at the given `host:port` address.
    pub async fn connect(addr: &str) -> Result<Self, NetworkError> {
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|e| NetworkError::Transport(format!("netproc connect: {e}")))?;
        let (sender, receiver) = fortrust_ipc::create_tcp_endpoint(stream);
        Ok(Self {
            sender,
            receiver,
            next_id: AtomicU64::new(1),
        })
    }

    /// Fetch a URL through the external netproc process.
    ///
    /// The netproc performs the full pipeline: privacy inspection, URL upgrade,
    /// cache lookup, HTTP transport, and cache storage. The result is sent back
    /// as `ResponseBody` + `RequestComplete` events over the TCP connection.
    ///
    /// Known limitations in the current implementation:
    /// - Response headers are not returned (empty `HeaderMap`)
    /// - The response URL is the original request URL (redirects not tracked)
    pub async fn fetch(&self, context: RequestContext) -> Result<NetworkResponse, NetworkError> {
        let request_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let original_url = context.url.clone();

        let resource_type_str = resource_type_to_string(context.resource_type);

        self.sender
            .send_net_command(&NetProcessCommand::FetchUrl {
                request_id,
                url: context.url,
                headers: Vec::new(),
                method: "GET".to_owned(),
                resource_type: resource_type_str,
                top_level_url: context.top_level_url,
            })
            .await
            .map_err(|e| NetworkError::Transport(format!("netproc send: {e}")))?;

        let mut body = Vec::new();

        loop {
            let event = self
                .receiver
                .recv_net_event()
                .await
                .map_err(|e| NetworkError::Transport(format!("netproc recv: {e}")))?;

            match event {
                NetProcessEvent::ResponseBody {
                    request_id: id,
                    chunk,
                    last: _,
                } => {
                    if id == request_id {
                        body.extend_from_slice(&chunk);
                    }
                }
                NetProcessEvent::RequestComplete {
                    request_id: id,
                    status,
                    total_bytes: _,
                    source,
                } => {
                    if id == request_id {
                        let response_url = Url::parse(&original_url)
                            .map_err(|_| NetworkError::InvalidEffectiveUrl)?;
                        let fetch_source = match source.as_str() {
                            s if s.contains("Cache") => FetchSource::Cache,
                            s if s.contains("Revalidated") => FetchSource::RevalidatedCache,
                            _ => FetchSource::Network,
                        };
                        return Ok(NetworkResponse {
                            url: response_url,
                            status,
                            headers: http::HeaderMap::new(),
                            body: Bytes::from(body),
                            source: fetch_source,
                        });
                    }
                }
                NetProcessEvent::RequestFailed {
                    request_id: id,
                    error,
                } => {
                    if id == request_id {
                        return Err(NetworkError::Transport(error));
                    }
                }
                _ => {}
            }
        }
    }
}

fn resource_type_to_string(rt: ResourceType) -> String {
    match rt {
        ResourceType::Document => "document",
        ResourceType::Script => "script",
        ResourceType::Image => "image",
        ResourceType::Stylesheet => "stylesheet",
        ResourceType::Xhr => "xhr",
        ResourceType::Media => "media",
        ResourceType::Font => "font",
        ResourceType::Other => "other",
    }
    .to_owned()
}
