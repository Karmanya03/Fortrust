use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use boa_engine::{
    Context, JsError as BoaError, JsValue as BoaValue, Source, js_string,
    native_function::NativeFunction, object::FunctionObjectBuilder,
};
use thiserror::Error;
use tracing::{debug, warn};

use crate::bindings;
use crate::event_loop::{EventLoop, TimerHandle};
use fortrust_core::FingerprintGuard;
use fortrust_dom::Document;
use fortrust_storage::{IndexedDbStore, LocalStorageStore};

pub type JsValue = BoaValue;

#[derive(Debug, Error)]
pub enum JsError {
    #[error("JS syntax error: {0}")]
    Syntax(String),
    #[error("JS runtime error: {0}")]
    Runtime(String),
    #[error("Type error: {0}")]
    TypeError(String),
    #[error("Reference error: {0}")]
    ReferenceError(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<BoaError> for JsError {
    fn from(error: BoaError) -> Self {
        let msg = error.to_string();
        if msg.contains("SyntaxError") || msg.contains("Unexpected token") {
            Self::Syntax(msg)
        } else if msg.contains("TypeError") {
            Self::TypeError(msg)
        } else if msg.contains("ReferenceError") {
            Self::ReferenceError(msg)
        } else {
            Self::Runtime(msg)
        }
    }
}

static SCRIPT_ID: AtomicU64 = AtomicU64::new(1);

pub struct WebApiRegistry {
    console_enabled: bool,
    timers_enabled: bool,
    fetch_enabled: bool,
    storage_enabled: bool,
    indexed_db_enabled: bool,
    dom_bridge_enabled: bool,
    websocket_enabled: bool,
    allowed_origins: Vec<String>,
    max_heap_bytes: usize,
    max_execution_ms: u64,
}

impl WebApiRegistry {
    pub fn new() -> Self {
        Self {
            console_enabled: true,
            timers_enabled: true,
            fetch_enabled: true,
            storage_enabled: true,
            indexed_db_enabled: true,
            dom_bridge_enabled: true,
            websocket_enabled: true,
            allowed_origins: Vec::new(),
            max_heap_bytes: 64 * 1024 * 1024,
            max_execution_ms: 10_000,
        }
    }

    pub fn with_console(mut self, enabled: bool) -> Self {
        self.console_enabled = enabled;
        self
    }

    pub fn with_timers(mut self, enabled: bool) -> Self {
        self.timers_enabled = enabled;
        self
    }

    pub fn with_fetch(mut self, enabled: bool) -> Self {
        self.fetch_enabled = enabled;
        self
    }

    pub fn with_storage(mut self, enabled: bool) -> Self {
        self.storage_enabled = enabled;
        self
    }

    pub fn with_indexed_db(mut self, enabled: bool) -> Self {
        self.indexed_db_enabled = enabled;
        self
    }

    pub fn with_dom_bridge(mut self, enabled: bool) -> Self {
        self.dom_bridge_enabled = enabled;
        self
    }

    pub fn with_websocket(mut self, enabled: bool) -> Self {
        self.websocket_enabled = enabled;
        self
    }

    pub fn with_max_heap(mut self, bytes: usize) -> Self {
        self.max_heap_bytes = bytes;
        self
    }

    pub fn with_max_execution_time(mut self, ms: u64) -> Self {
        self.max_execution_ms = ms;
        self
    }

    pub fn with_allowed_origin(mut self, origin: impl Into<String>) -> Self {
        self.allowed_origins.push(origin.into());
        self
    }

    pub fn max_heap(&self) -> usize {
        self.max_heap_bytes
    }

    pub fn max_execution_ms(&self) -> u64 {
        self.max_execution_ms
    }
}

impl Default for WebApiRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct JsRuntime {
    context: Context,
    next_timer_id: u64,
    active_timeouts: HashMap<u64, TimerHandle>,
    active_intervals: HashMap<u64, TimerHandle>,
    registry: WebApiRegistry,
    origin: String,
    network: Option<fortrust_net::NetworkClient>,
    arena: Option<&'static fortrust_dom::DomArena>,
    local_storage: Option<LocalStorageStore>,
    indexed_db: Option<IndexedDbStore>,
    /// Per-workspace fingerprint guard for spoofed navigator/screen/canvas values.
    fingerprint_guard: Option<FingerprintGuard>,
}

impl JsRuntime {
    pub fn new() -> Self {
        let context = Context::default();
        Self {
            context,
            next_timer_id: 1,
            active_timeouts: HashMap::new(),
            active_intervals: HashMap::new(),
            registry: WebApiRegistry::new(),
            origin: String::new(),
            network: None,
            arena: None,
            local_storage: None,
            indexed_db: None,
            fingerprint_guard: None,
        }
    }

    pub fn with_origin(mut self, origin: impl Into<String>) -> Self {
        self.origin = origin.into();
        self
    }

    pub fn with_registry(mut self, registry: WebApiRegistry) -> Self {
        self.registry = registry;
        self
    }

    pub fn with_network(mut self, network: fortrust_net::NetworkClient) -> Self {
        self.network = Some(network);
        self
    }

    pub fn with_arena(mut self, arena: &'static fortrust_dom::DomArena) -> Self {
        self.arena = Some(arena);
        self
    }

    pub fn with_local_storage(mut self, store: LocalStorageStore) -> Self {
        self.local_storage = Some(store);
        self
    }

    pub fn with_indexed_db_store(mut self, store: IndexedDbStore) -> Self {
        self.indexed_db = Some(store);
        self
    }

    pub fn with_fingerprint_guard(mut self, guard: FingerprintGuard) -> Self {
        self.fingerprint_guard = Some(guard);
        self
    }

    pub fn fingerprint_guard(&self) -> Option<&FingerprintGuard> {
        self.fingerprint_guard.as_ref()
    }

    pub fn arena(&self) -> Option<&'static fortrust_dom::DomArena> {
        self.arena
    }

    pub fn attach_document(&mut self, document: &Document<'static>) -> Result<(), JsError> {
        if self.registry.dom_bridge_enabled {
            bindings::dom_api::register(&mut self.context, document, self.arena)?;
        }
        // Expose `document.title` as a proper accessor that delegates to getTitle/setTitle.
        let _ = self.eval(
            "Object.defineProperty(document, 'title', { get() { return this.getTitle(); }, set(v) { this.setTitle(v); }, configurable: true, enumerable: true });",
        );
        Ok(())
    }

    pub fn initialize(&mut self, event_loop: &mut EventLoop) -> Result<(), JsError> {
        let origin = self.origin.clone();
        let registry = &self.registry;

        if registry.console_enabled {
            bindings::console::register(&mut self.context)?;
        }

        if registry.timers_enabled {
            bindings::timers::register(
                &mut self.context,
                &mut self.next_timer_id,
                &mut self.active_timeouts,
                &mut self.active_intervals,
                event_loop,
            )?;
        }

        if registry.fetch_enabled {
            bindings::fetch::register(
                &mut self.context,
                origin.clone(),
                event_loop,
                self.network.clone(),
            )?;
        }

        // XMLHttpRequest — legacy HTTP request API
        bindings::xhr::register(
            &mut self.context,
            origin.clone(),
            event_loop,
            self.network.clone(),
        )?;

        bindings::navigator::register(&mut self.context, self.fingerprint_guard.as_ref())?;
        bindings::screen::register(&mut self.context, self.fingerprint_guard.as_ref())?;

        bindings::location::register(&mut self.context, &origin)?;

        if registry.storage_enabled {
            bindings::storage::register_with_backend(
                &mut self.context,
                self.local_storage.clone(),
            )?;
        }

        if registry.websocket_enabled {
            bindings::websocket::register(&mut self.context)?;
        }

        if registry.indexed_db_enabled {
            if let Some(ref store) = self.indexed_db {
                bindings::indexed_db::initialize(std::sync::Arc::new(store.clone()));
            }
            bindings::indexed_db::register(&mut self.context)?;
        }

        bindings::crypto::register(&mut self.context)?;
        bindings::performance::register(&mut self.context)?;
        bindings::base64::register(&mut self.context)?;
        bindings::microtask::initialize(event_loop.task_queue());
        bindings::microtask::register(&mut self.context)?;

        debug!("JS runtime initialized for origin: {}", origin);
        Ok(())
    }

    pub fn eval(&mut self, source: &str) -> Result<JsValue, JsError> {
        let id = SCRIPT_ID.fetch_add(1, Ordering::Relaxed);
        debug!(script_id = id, "Evaluating JS script");

        let source = Source::from_bytes(source.as_bytes());
        match self.context.eval(source) {
            Ok(value) => {
                debug!(script_id = id, "JS evaluation succeeded");
                Ok(value)
            }
            Err(error) => {
                warn!(script_id = id, error = %error, "JS evaluation failed");
                Err(JsError::from(error))
            }
        }
    }

    pub fn execute_module(&mut self, source: &str, module_name: &str) -> Result<JsValue, JsError> {
        let id = SCRIPT_ID.fetch_add(1, Ordering::Relaxed);
        debug!(script_id = id, module = module_name, "Executing JS module");

        let source = Source::from_bytes(source.as_bytes()).with_path(Path::new(module_name));
        match self.context.eval(source) {
            Ok(value) => {
                debug!(script_id = id, "JS module execution succeeded");
                Ok(value)
            }
            Err(error) => {
                warn!(script_id = id, error = %error, "JS module execution failed");
                Err(JsError::from(error))
            }
        }
    }

    pub fn call_function(
        &mut self,
        function: &JsValue,
        this: &JsValue,
        args: &[JsValue],
    ) -> Result<JsValue, JsError> {
        let obj = function
            .as_object()
            .ok_or_else(|| JsError::TypeError("value is not callable".into()))?;
        obj.call(this, args, &mut self.context)
            .map_err(JsError::from)
    }

    pub fn register_global_property(&mut self, name: &str, value: JsValue) -> Result<(), JsError> {
        let global = self.context.global_object();
        global
            .set(js_string!(name), value, false, &mut self.context)
            .map_err(|e| JsError::Internal(format!("Failed to register global {name}: {e}")))
            .map(|_| ())
    }

    pub fn register_global_function(
        &mut self,
        name: &str,
        _arity: usize,
        function: NativeFunction,
    ) -> Result<(), JsError> {
        let global = self.context.global_object();
        let js_fn: BoaValue = FunctionObjectBuilder::new(self.context.realm(), function)
            .build()
            .into();
        global
            .set(js_string!(name), js_fn, false, &mut self.context)
            .map_err(|e| {
                JsError::Internal(format!("Failed to register global function {name}: {e}"))
            })
            .map(|_| ())
    }

    pub fn context(&mut self) -> &mut Context {
        &mut self.context
    }

    /// Execute all pending requestAnimationFrame callbacks from the given
    /// event loop, passing a DOMHighResTimeStamp (the number of milliseconds
    /// elapsed since origin) as the argument to each callback.
    pub fn execute_pending_raf(&mut self, event_loop: &EventLoop) {
        let callbacks = event_loop.drain_animation_frame_callbacks();
        if callbacks.is_empty() {
            return;
        }
        // Use wall-clock time from origin as timestamp (same as performance.now()).
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as f64;
        let ts = BoaValue::from(now);
        for (_id, handler) in callbacks {
            if let Err(e) = self.call_function(&handler, &BoaValue::undefined(), std::slice::from_ref(&ts)) {
                warn!(error = %e, raf_id = _id, "requestAnimationFrame callback failed");
            }
        }
    }

    pub fn set_origin(&mut self, origin: impl Into<String>) {
        self.origin = origin.into();
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn registry(&self) -> &WebApiRegistry {
        &self.registry
    }

    /// Poll all WebSocket connections for pending events and dispatch
    /// them to their JS event listeners (onopen, onmessage, onclose,
    /// onerror, and addEventListener-registered callbacks).
    /// Must be called once per event loop tick to ensure timely delivery.
    pub fn execute_pending_websocket(&mut self) {
        crate::bindings::websocket::poll_websocket_events(&mut self.context);
    }
}

impl Default for JsRuntime {
    fn default() -> Self {
        Self::new()
    }
}
