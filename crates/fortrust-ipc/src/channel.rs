use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tracing::debug;

use crate::codec::{BincodeCodec, CodecError, FramedMessage};
use crate::messages::{BrowserToRenderer, NetProcessCommand, NetProcessEvent, RendererToBrowser};

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("Channel closed")]
    ChannelClosed,
    #[error("Send error: {0}")]
    Send(String),
    #[error("Receive error: {0}")]
    Receive(String),
    #[error("Codec error: {0}")]
    Codec(#[from] CodecError),
    #[error("Timeout")]
    Timeout,
    #[error("Protocol error: {0}")]
    Protocol(String),
}

impl From<mpsc::error::SendError<Vec<u8>>> for IpcError {
    fn from(error: mpsc::error::SendError<Vec<u8>>) -> Self {
        Self::Send(error.to_string())
    }
}

#[derive(Clone)]
pub struct MessageSender {
    sender: Sender<Vec<u8>>,
}

#[derive(Clone)]
pub struct MessageReceiver {
    receiver: Arc<Mutex<Receiver<Vec<u8>>>>,
    buffer: Arc<Mutex<BytesMut>>,
}

pub struct IpcChannel {
    sender: MessageSender,
    receiver: MessageReceiver,
}

impl IpcChannel {
    pub fn new(tx: Sender<Vec<u8>>, rx: Receiver<Vec<u8>>) -> Self {
        Self {
            sender: MessageSender { sender: tx },
            receiver: MessageReceiver {
                receiver: Arc::new(Mutex::new(rx)),
                buffer: Arc::new(Mutex::new(BytesMut::new())),
            },
        }
    }

    pub fn split(self) -> (MessageSender, MessageReceiver) {
        (self.sender, self.receiver)
    }

    pub fn sender(&self) -> &MessageSender {
        &self.sender
    }

    pub fn receiver(&self) -> &MessageReceiver {
        &self.receiver
    }
}

impl MessageSender {
    pub async fn send_browser_message(&self, message: &BrowserToRenderer) -> Result<(), IpcError> {
        let data = FramedMessage::new(message)?.into_bytes();
        self.sender.send(data).await?;
        debug!("Sent BrowserToRenderer message");
        Ok(())
    }

    pub async fn send_renderer_message(&self, message: &RendererToBrowser) -> Result<(), IpcError> {
        let data = FramedMessage::new(message)?.into_bytes();
        self.sender.send(data).await?;
        debug!("Sent RendererToBrowser message");
        Ok(())
    }

    pub async fn send_net_command(&self, command: &NetProcessCommand) -> Result<(), IpcError> {
        let data = FramedMessage::new(command)?.into_bytes();
        self.sender.send(data).await?;
        debug!("Sent NetProcessCommand");
        Ok(())
    }

    pub async fn send_net_event(&self, event: &NetProcessEvent) -> Result<(), IpcError> {
        let data = FramedMessage::new(event)?.into_bytes();
        self.sender.send(data).await?;
        debug!("Sent NetProcessEvent");
        Ok(())
    }

    pub async fn send_raw(&self, data: Vec<u8>) -> Result<(), IpcError> {
        self.sender.send(data).await?;
        Ok(())
    }
}

impl MessageReceiver {
    pub async fn recv_browser_message(&self) -> Result<BrowserToRenderer, IpcError> {
        let bytes = self.recv_raw().await?;
        BincodeCodec::decode(&bytes).map_err(IpcError::Codec)
    }

    pub async fn recv_renderer_message(&self) -> Result<RendererToBrowser, IpcError> {
        let bytes = self.recv_raw().await?;
        BincodeCodec::decode(&bytes).map_err(IpcError::Codec)
    }

    pub async fn recv_net_command(&self) -> Result<NetProcessCommand, IpcError> {
        let bytes = self.recv_raw().await?;
        BincodeCodec::decode(&bytes).map_err(IpcError::Codec)
    }

    pub async fn recv_net_event(&self) -> Result<NetProcessEvent, IpcError> {
        let bytes = self.recv_raw().await?;
        BincodeCodec::decode(&bytes).map_err(IpcError::Codec)
    }

    pub async fn recv_raw(&self) -> Result<Vec<u8>, IpcError> {
        let mut buffer = self.buffer.lock().await;
        let mut receiver = self.receiver.lock().await;

        loop {
            if let Ok(Some((_, _))) = BincodeCodec::decode_message::<Vec<u8>>(&mut buffer) {
                let (message, _) = BincodeCodec::decode_message::<Vec<u8>>(&mut buffer)
                    .map_err(IpcError::Codec)?
                    .ok_or(IpcError::Protocol(
                        "Failed to extract framed message".to_owned(),
                    ))?;
                return Ok(message);
            }

            let chunk = receiver.recv().await.ok_or(IpcError::ChannelClosed)?;

            buffer.extend_from_slice(&chunk);
        }
    }

    pub async fn recv_raw_timeout(&self, timeout: Duration) -> Result<Vec<u8>, IpcError> {
        tokio::time::timeout(timeout, self.recv_raw())
            .await
            .map_err(|_| IpcError::Timeout)?
    }
}
