use std::sync::{Arc, Mutex};

type TitleHandler = dyn Fn(String) + Send + Sync + 'static;
type EventHandler = dyn Fn(String, String) + Send + Sync + 'static;

lazy_static::lazy_static! {
    static ref TITLE_HANDLER: Mutex<Option<Arc<TitleHandler>>> = Mutex::new(None);
    static ref EVENT_HANDLER: Mutex<Option<Arc<EventHandler>>> = Mutex::new(None);
}

pub fn set_title_handler(cb: Option<Arc<TitleHandler>>) {
    let mut guard = TITLE_HANDLER.lock().unwrap();
    *guard = cb;
}

pub fn notify_title_changed(title: String) {
    if let Some(cb) = &*TITLE_HANDLER.lock().unwrap() {
        cb(title);
    }
}

pub fn set_event_handler(cb: Option<Arc<EventHandler>>) {
    let mut guard = EVENT_HANDLER.lock().unwrap();
    *guard = cb;
}

pub fn notify_event(name: String, detail: String) {
    if let Some(cb) = &*EVENT_HANDLER.lock().unwrap() {
        cb(name, detail);
    }
}
