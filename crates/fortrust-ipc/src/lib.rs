mod channel;
mod codec;
mod messages;

pub use channel::{IpcChannel, IpcError, MessageReceiver, MessageSender};
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
