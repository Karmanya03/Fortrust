use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use boa_engine::{
    Context, JsError as BoaError, JsValue as BoaValue, Source, js_string,
    native_function::NativeFunction,
    object::FunctionObjectBuilder,
};
use thiserror::Error;
use tracing::{debug, warn};

use crate::event_loop::{EventLoop, TimerHandle};
use crate::bindings;
use fortrust_dom::Document;

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
    dom_bridge_enabled: bool,
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
            dom_bridge_enabled: true,
            allowed_origins: Vec::new(),
            max_heap_bytes: 64 * 1024 * 1024,
            max_execution_ms: 10_000,
        }
    }

    pub fn with_console(mut self, enabled: bool) -> Self {
        self.console_enabled = enabled;
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

    pub fn attach_document(&mut self, document: &Document<'static>) -> Result<(), JsError> {
        if self.registry.dom_bridge_enabled {
            bindings::dom_api::register(&mut self.context, document)?;
        }
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
            bindings::fetch::register(&mut self.context, origin.clone(), event_loop, self.network.clone())?;
        }

        bindings::navigator::register(&mut self.context)?;

        bindings::location::register(&mut self.context, &origin)?;

        if registry.storage_enabled {
            bindings::storage::register(&mut self.context)?;
        }

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

    pub fn register_global_property(
        &mut self,
        name: &str,
        value: JsValue,
    ) -> Result<(), JsError> {
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
        let js_fn: BoaValue =
            FunctionObjectBuilder::new(self.context.realm(), function).build().into();
        global
            .set(js_string!(name), js_fn, false, &mut self.context)
            .map_err(|e| JsError::Internal(format!("Failed to register global function {name}: {e}")))
            .map(|_| ())
    }

    pub fn context(&mut self) -> &mut Context {
        &mut self.context
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
}

impl Default for JsRuntime {
    fn default() -> Self {
        Self::new()
    }
}
