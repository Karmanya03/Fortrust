use std::sync::Arc;
use std::time::Duration;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use url::Url;

#[derive(Debug, Clone)]
pub enum WebSocketMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<u16>, Option<String>),
}

#[derive(Debug, Clone)]
pub enum WebSocketEvent {
    Message(WebSocketMessage),
    Open,
    Close(Option<u16>, String),
    Error(String),
}

#[derive(Debug, Clone)]
pub enum WebSocketError {
    InvalidUrl(String),
    ConnectionFailed(String),
    SendFailed(String),
    AlreadyClosed,
    NotConnected,
}

pub struct WebSocketClient {
    url: Url,
    inner: Arc<Mutex<Option<WebSocketConnectionInner>>>,
    receiver: Arc<Mutex<tokio::sync::mpsc::Receiver<WebSocketEvent>>>,
    sender: tokio::sync::mpsc::Sender<WebSocketCommand>,
}

enum WebSocketCommand {
    Send(WebSocketMessage),
    Close(Option<u16>, Option<String>),
}

struct WebSocketConnectionInner {
    _write: tokio::sync::mpsc::Sender<WebSocketCommand>,
}

impl WebSocketClient {
    pub fn new(url_str: &str) -> Result<Self, WebSocketError> {
        let url = Url::parse(url_str).map_err(|e| {
            WebSocketError::InvalidUrl(format!("{e}"))
        })?;
        match url.scheme() {
            "ws" | "wss" => {}
            _ => return Err(WebSocketError::InvalidUrl(format!(
                "unsupported scheme '{}', expected ws:// or wss://", url.scheme()
            ))),
        }

        let (cmd_tx, mut cmd_rx_all) = tokio::sync::mpsc::channel::<WebSocketCommand>(64);
        let (event_tx, event_rx) = tokio::sync::mpsc::channel::<WebSocketEvent>(64);

        let inner: Arc<Mutex<Option<WebSocketConnectionInner>>> = Arc::new(Mutex::new(None));
        let inner_clone = inner.clone();
        let url_clone = url.clone();
        let cmd_tx_task = cmd_tx.clone();

        tokio::spawn(async move {
            let connect_result = tokio::time::timeout(
                Duration::from_secs(15),
                connect_async(url_clone.as_str()),
            )
            .await;

            let (ws_stream, _) = match connect_result {
                Ok(Ok(stream)) => stream,
                Ok(Err(e)) => {
                    let _ = event_tx.send(WebSocketEvent::Error(format!("{e}"))).await;
                    return;
                }
                Err(_) => {
                    let _ = event_tx.send(WebSocketEvent::Error("connection timeout".into())).await;
                    return;
                }
            };

            let (mut ws_write, mut ws_read) = ws_stream.split();

            {
                let mut guard = inner_clone.lock().await;
                *guard = Some(WebSocketConnectionInner { _write: cmd_tx_task.clone() });
            }

            let _ = event_tx.send(WebSocketEvent::Open).await;

            let event_tx_for_read = event_tx.clone();
            let read_handle = tokio::spawn(async move {
                while let Some(Ok(msg)) = ws_read.next().await {
                    let event = match msg {
                        WsMessage::Text(text) => WebSocketEvent::Message(WebSocketMessage::Text(text.to_string())),
                        WsMessage::Binary(data) => WebSocketEvent::Message(WebSocketMessage::Binary(data.to_vec())),
                        WsMessage::Ping(data) => WebSocketEvent::Message(WebSocketMessage::Ping(data.to_vec())),
                        WsMessage::Pong(data) => WebSocketEvent::Message(WebSocketMessage::Pong(data.to_vec())),
                        WsMessage::Close(frame) => {
                            let (code, reason) = frame
                                .map(|f| (Some(f.code.into()), f.reason.to_string()))
                                .unwrap_or((None, String::new()));
                            WebSocketEvent::Close(code, reason)
                        }
                        WsMessage::Frame(_) => continue,
                    };
                    if event_tx_for_read.send(event).await.is_err() {
                        break;
                    }
                }
            });

            let event_tx_for_cmds = event_tx.clone();
            let write_handle = tokio::spawn(async move {
                while let Some(cmd) = cmd_rx_all.recv().await {
                    let ws_msg = match cmd {
                        WebSocketCommand::Send(WebSocketMessage::Text(t)) => WsMessage::Text(t),
                        WebSocketCommand::Send(WebSocketMessage::Binary(b)) => WsMessage::Binary(b),
                        WebSocketCommand::Send(WebSocketMessage::Ping(d)) => WsMessage::Ping(d),
                        WebSocketCommand::Send(WebSocketMessage::Pong(d)) => WsMessage::Pong(d),
                        WebSocketCommand::Send(WebSocketMessage::Close(c, r)) => {
                            let code = tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(
                                c.unwrap_or(1000u16)
                            );
                            let frame = tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                code,
                                reason: std::borrow::Cow::Owned(r.unwrap_or_default()),
                            };
                            WsMessage::Close(Some(frame))
                        }
                        WebSocketCommand::Close(code, reason) => {
                            let code = tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(
                                code.unwrap_or(1000u16)
                            );
                            let frame = tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                code,
                                reason: std::borrow::Cow::Owned(reason.unwrap_or_default()),
                            };
                            WsMessage::Close(Some(frame))
                        }
                    };
                    if let Err(e) = ws_write.send(ws_msg).await {
                        let _ = event_tx_for_cmds.send(WebSocketEvent::Error(format!("send error: {e}"))).await;
                        break;
                    }
                }
            });

            tokio::select! {
                _ = read_handle => {},
                _ = write_handle => {},
            }

            let _ = event_tx.send(WebSocketEvent::Close(None, "connection closed".into())).await;
        });

        Ok(Self {
            url,
            inner,
            receiver: Arc::new(Mutex::new(event_rx)),
            sender: cmd_tx,
        })
    }

    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    pub async fn send(&self, message: WebSocketMessage) -> Result<(), WebSocketError> {
        self.sender.send(WebSocketCommand::Send(message)).await
            .map_err(|_| WebSocketError::NotConnected)
    }

    pub async fn close(&self, code: Option<u16>, reason: Option<String>) -> Result<(), WebSocketError> {
        self.sender.send(WebSocketCommand::Close(code, reason)).await
            .map_err(|_| WebSocketError::NotConnected)
    }

    pub async fn recv(&self) -> Option<WebSocketEvent> {
        let mut rx = self.receiver.lock().await;
        rx.recv().await
    }

    pub async fn try_recv(&self) -> Option<WebSocketEvent> {
        let mut rx = self.receiver.lock().await;
        rx.try_recv().ok()
    }
}
