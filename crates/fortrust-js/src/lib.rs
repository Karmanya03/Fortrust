pub mod bindings;
mod event_loop;
mod runtime;
mod hooks;
pub mod sandbox;
pub use hooks::{set_title_handler, set_event_handler};

pub use event_loop::{EventLoop, TaskQueue, TimerHandle, TimerKind};
pub use runtime::{JsError, JsRuntime, JsValue, WebApiRegistry};
pub use sandbox::{SandboxConfig, SandboxedRuntime, SandboxResult};

use std::cell::RefCell;
use std::rc::Rc;

pub type SharedJsRuntime = Rc<RefCell<JsRuntime>>;

pub fn new_shared_runtime() -> (SharedJsRuntime, EventLoop) {
    let runtime = JsRuntime::new();
    let event_loop = EventLoop::new();
    (Rc::new(RefCell::new(runtime)), event_loop)
}
