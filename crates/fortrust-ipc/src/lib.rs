use serde::{Deserialize, Serialize};
use bincode::config;

/// Messages exchanged between processes.
#[derive(Debug, Clone, Serialize, Deserialize, bincode::Encode, bincode::Decode)]
pub enum IpcMessage {
    NavigateRequest { tab_id: u64, url: String },
    ResourceResponse { request_id: u64, status: u16, body: Vec<u8> },
    PaintFrame { tab_id: u64, frame_id: u64, payload: Vec<u8> },
    TabUpdate { tab_id: u64, title: Option<String>, url: Option<String> },
    SecurityInfo { tab_id: u64, secure: bool },
}

impl IpcMessage {
    /// Serialize to bytes using bincode.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let cfg = config::standard().with_variable_int_encoding();
        bincode::encode_to_vec(self, cfg).map_err(|e| e.to_string())
    }

    /// Deserialize from bytes using bincode.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let cfg = config::standard().with_variable_int_encoding();
        bincode::decode_from_slice(bytes, cfg).map(|(v, _)| v).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_bincode() {
        let msg = IpcMessage::NavigateRequest { tab_id: 1, url: "https://example.test/".into() };
        let b = msg.to_bytes().expect("serialize");
        let out = IpcMessage::from_bytes(&b).expect("deserialize");
        match out {
            IpcMessage::NavigateRequest { tab_id, url } => {
                assert_eq!(tab_id, 1);
                assert_eq!(url, "https://example.test/");
            }
            _ => panic!("unexpected message"),
        }
    }
}
mod channel;
mod codec;
mod messages;

pub use channel::{IpcChannel, IpcError, MessageReceiver, MessageSender};
pub use channel::create_tcp_endpoint;
pub use codec::{BincodeCodec, CodecError, FramedMessage};
pub use messages::{
    BrowserToRenderer, KeyEvent, LoadState, Modifiers, MouseEvent, NetProcessCommand,
    NetProcessEvent, PrivacyEvent, RendererToBrowser,
};

use std::sync::Arc;
use tokio::sync::Mutex;

pub type SharedIpcChannel = Arc<Mutex<IpcChannel>>;

pub fn create_ipc_pair() -> (IpcChannel, IpcChannel) {
    let (tx1, rx2) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let (tx2, rx1) = tokio::sync::mpsc::channel::<Vec<u8>>(64);

    let channel_a = IpcChannel::new(tx1, rx1);
    let channel_b = IpcChannel::new(tx2, rx2);

    (channel_a, channel_b)
}

pub fn create_shared_ipc_pair() -> (SharedIpcChannel, SharedIpcChannel) {
    let (a, b) = create_ipc_pair();
    (Arc::new(Mutex::new(a)), Arc::new(Mutex::new(b)))
}
