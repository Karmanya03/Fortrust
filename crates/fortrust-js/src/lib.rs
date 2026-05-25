mod runtime;
mod event_loop;
pub mod bindings;

pub use runtime::{JsRuntime, JsError, JsValue, WebApiRegistry};
pub use event_loop::{EventLoop, TaskQueue, TimerHandle, TimerKind};

use std::cell::RefCell;
use std::rc::Rc;

pub type SharedJsRuntime = Rc<RefCell<JsRuntime>>;

pub fn new_shared_runtime() -> (SharedJsRuntime, EventLoop) {
    let runtime = JsRuntime::new();
    let event_loop = EventLoop::new();
    (Rc::new(RefCell::new(runtime)), event_loop)
}
