pub mod bindings;
mod event_loop;
mod runtime;
mod hooks;

pub use event_loop::{EventLoop, TaskQueue, TimerHandle, TimerKind};
pub use runtime::{JsError, JsRuntime, JsValue, WebApiRegistry};

use std::cell::RefCell;
use std::rc::Rc;

pub type SharedJsRuntime = Rc<RefCell<JsRuntime>>;

pub fn new_shared_runtime() -> (SharedJsRuntime, EventLoop) {
    let runtime = JsRuntime::new();
    let event_loop = EventLoop::new();
    (Rc::new(RefCell::new(runtime)), event_loop)
}
