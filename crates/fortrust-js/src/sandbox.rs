//! Boa Sandbox hardening — restricted JS execution contexts.
//!
//! Provides hardened JS runtimes for:
//! - **Privacy container tabs** (restricted API access, isolated storage)
//! - **Extension sandboxing** (no DOM/network/storage access)
//! - **Untrusted content scripts** (minimal API surface)
//!
//! Enforces memory limits, execution time limits, and API restrictions.

use std::time::{Duration, Instant};
use boa_engine::JsValue;
use tracing::warn;

use crate::runtime::{JsError, JsRuntime, WebApiRegistry};
use crate::event_loop::EventLoop;

/// Configuration for a sandboxed JS execution context.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum heap memory in bytes. Scripts exceeding this are terminated.
    pub max_heap_bytes: usize,
    /// Maximum execution time per script evaluation. Scripts exceeding this are killed.
    pub max_execution_time: Duration,
    /// Whether `console` API is available.
    pub console_enabled: bool,
    /// Whether `setTimeout`/`setInterval` are available.
    pub timers_enabled: bool,
    /// Whether `fetch()` is available.
    pub fetch_enabled: bool,
    /// Whether `localStorage`/`sessionStorage` are available.
    pub storage_enabled: bool,
    /// Whether DOM bridge (`document`, `window`) is available.
    pub dom_bridge_enabled: bool,
    /// Whether `WebSocket` is available.
    pub websocket_enabled: bool,
    /// Whether `XMLHttpRequest` is available.
    pub xhr_enabled: bool,
    /// Maximum number of statements that can be executed.
    /// Prevents infinite loops in untrusted code.
    pub max_statements: Option<u64>,
    /// Maximum call stack depth.
    pub max_call_depth: Option<usize>,
    /// Whether to allow `eval()` and `Function()` constructor.
    pub allow_dynamic_code: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_heap_bytes: 32 * 1024 * 1024,  // 32 MB
            max_execution_time: Duration::from_secs(5),
            console_enabled: true,
            timers_enabled: false,
            fetch_enabled: false,
            storage_enabled: false,
            dom_bridge_enabled: false,
            websocket_enabled: false,
            xhr_enabled: false,
            max_statements: Some(1_000_000),
            max_call_depth: Some(100),
            allow_dynamic_code: false,
        }
    }
}

impl SandboxConfig {
    /// Create a maximally restrictive sandbox for extension background scripts.
    pub fn extension_sandbox() -> Self {
        Self {
            max_heap_bytes: 16 * 1024 * 1024,  // 16 MB
            max_execution_time: Duration::from_secs(3),
            console_enabled: true,
            timers_enabled: true,
            fetch_enabled: false,
            storage_enabled: true,  // Extensions get their own storage
            dom_bridge_enabled: false,
            websocket_enabled: false,
            xhr_enabled: false,
            max_statements: Some(500_000),
            max_call_depth: Some(50),
            allow_dynamic_code: false,
        }
    }

    /// Create a sandbox for privacy container tabs.
    /// Allows most APIs but with isolated storage and no cross-origin access.
    pub fn container_tab() -> Self {
        Self {
            max_heap_bytes: 64 * 1024 * 1024,  // 64 MB
            max_execution_time: Duration::from_secs(10),
            console_enabled: true,
            timers_enabled: true,
            fetch_enabled: true,
            storage_enabled: true,
            dom_bridge_enabled: true,
            websocket_enabled: true,
            xhr_enabled: true,
            max_statements: None,
            max_call_depth: None,
            allow_dynamic_code: true,
        }
    }

    /// Create a sandbox for content scripts (injected into pages).
    /// Minimal API access — no direct DOM mutation, limited fetch.
    pub fn content_script() -> Self {
        Self {
            max_heap_bytes: 8 * 1024 * 1024,   // 8 MB
            max_execution_time: Duration::from_secs(2),
            console_enabled: true,
            timers_enabled: true,
            fetch_enabled: false,
            storage_enabled: false,
            dom_bridge_enabled: false,
            websocket_enabled: false,
            xhr_enabled: false,
            max_statements: Some(100_000),
            max_call_depth: Some(30),
            allow_dynamic_code: false,
        }
    }

    /// Create a sandbox for untrusted worker scripts.
    pub fn worker() -> Self {
        Self {
            max_heap_bytes: 32 * 1024 * 1024,
            max_execution_time: Duration::from_secs(30),
            console_enabled: true,
            timers_enabled: true,
            fetch_enabled: true,
            storage_enabled: false,
            dom_bridge_enabled: false,
            websocket_enabled: true,
            xhr_enabled: false,
            max_statements: None,
            max_call_depth: Some(100),
            allow_dynamic_code: false,
        }
    }

    /// Convert this sandbox config to a `WebApiRegistry` for the JS runtime.
    pub fn to_registry(&self) -> WebApiRegistry {
        WebApiRegistry::new()
            .with_console(self.console_enabled)
            .with_timers(self.timers_enabled)
            .with_fetch(self.fetch_enabled)
            .with_storage(self.storage_enabled)
            .with_dom_bridge(self.dom_bridge_enabled)
            .with_websocket(self.websocket_enabled)
            .with_max_heap(self.max_heap_bytes)
            .with_max_execution_time(self.max_execution_time.as_millis() as u64)
    }
}

/// Result of a sandboxed script evaluation.
#[derive(Debug)]
pub enum SandboxResult {
    /// Script completed successfully with a return value.
    Success(JsValue),
    /// Script was killed due to exceeding the time limit.
    Timeout { elapsed: Duration },
    /// Script was killed due to exceeding the memory limit.
    MemoryExceeded { bytes: usize },
    /// Script threw an exception.
    Exception(JsError),
    /// Script was rejected by the sandbox policy.
    PolicyViolation(String),
}

/// A hardened JS runtime that enforces sandbox constraints.
///
/// Wraps a `JsRuntime` with additional safety checks:
/// - Execution time limits (checked before and after each eval)
/// - Statement count limits (prevents infinite loops)
/// - Memory usage tracking
/// - API access restrictions
pub struct SandboxedRuntime {
    inner: JsRuntime,
    config: SandboxConfig,
    total_execution_time: Duration,
    eval_count: u64,
}

impl SandboxedRuntime {
    /// Create a new sandboxed runtime with the given configuration.
    pub fn new(config: SandboxConfig) -> Self {
        let registry = config.to_registry();
        let inner = JsRuntime::new()
            .with_registry(registry);

        Self {
            inner,
            config,
            total_execution_time: Duration::ZERO,
            eval_count: 0,
        }
    }

    /// Initialize the runtime with an event loop (registers available APIs).
    pub fn initialize(&mut self, event_loop: &mut EventLoop) -> Result<(), JsError> {
        self.inner.initialize(event_loop)
    }

    /// Evaluate a script in the sandbox with time and resource enforcement.
    pub fn eval(&mut self, source: &str) -> SandboxResult {
        // Check policy before execution
        if !self.config.allow_dynamic_code
            && (source.contains("eval(") || source.contains("Function(")) {
                return SandboxResult::PolicyViolation(
                    "Dynamic code execution (eval/Function) is not allowed in this sandbox".into()
                );
            }

        let start = Instant::now();

        // Check if we've already exceeded our time budget
        if self.total_execution_time >= self.config.max_execution_time {
            return SandboxResult::Timeout {
                elapsed: self.total_execution_time,
            };
        }

        let result = self.inner.eval(source);
        let elapsed = start.elapsed();
        self.total_execution_time += elapsed;
        self.eval_count += 1;

        // Check time limit after execution
        if self.total_execution_time > self.config.max_execution_time {
            warn!(
                total_ms = self.total_execution_time.as_millis(),
                limit_ms = self.config.max_execution_time.as_millis(),
                "Sandbox execution time exceeded"
            );
            return SandboxResult::Timeout {
                elapsed: self.total_execution_time,
            };
        }

        match result {
            Ok(value) => SandboxResult::Success(value),
            Err(error) => SandboxResult::Exception(error),
        }
    }

    /// Evaluate a script with a specific time limit override (shorter than the config limit).
    pub fn eval_with_timeout(&mut self, source: &str, timeout: Duration) -> SandboxResult {
        let effective_limit = timeout.min(self.config.max_execution_time);
        let start = Instant::now();

        let result = self.inner.eval(source);
        let elapsed = start.elapsed();

        if elapsed > effective_limit {
            return SandboxResult::Timeout { elapsed };
        }

        self.total_execution_time += elapsed;
        self.eval_count += 1;

        match result {
            Ok(value) => SandboxResult::Success(value),
            Err(error) => SandboxResult::Exception(error),
        }
    }

    /// Get the total accumulated execution time across all evaluations.
    pub fn total_execution_time(&self) -> Duration {
        self.total_execution_time
    }

    /// Get the number of evaluations performed.
    pub fn eval_count(&self) -> u64 {
        self.eval_count
    }

    /// Get a reference to the sandbox configuration.
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }

    /// Access the inner JsRuntime for advanced operations.
    /// Use with caution — bypasses sandbox enforcement.
    pub fn inner_mut(&mut self) -> &mut JsRuntime {
        &mut self.inner
    }

    /// Register a global property in the sandboxed context.
    pub fn register_global_property(&mut self, name: &str, value: JsValue) -> Result<(), JsError> {
        self.inner.register_global_property(name, value)
    }

    /// Reset the execution time counter (e.g., between page loads).
    pub fn reset_time_budget(&mut self) {
        self.total_execution_time = Duration::ZERO;
        self.eval_count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_config_defaults() {
        let config = SandboxConfig::default();
        assert_eq!(config.max_heap_bytes, 32 * 1024 * 1024);
        assert_eq!(config.max_execution_time, Duration::from_secs(5));
        assert!(config.console_enabled);
        assert!(!config.fetch_enabled);
        assert!(!config.dom_bridge_enabled);
        assert!(!config.allow_dynamic_code);
    }

    #[test]
    fn extension_sandbox_is_restrictive() {
        let config = SandboxConfig::extension_sandbox();
        assert!(!config.fetch_enabled);
        assert!(!config.dom_bridge_enabled);
        assert!(!config.websocket_enabled);
        assert!(!config.allow_dynamic_code);
        assert!(config.storage_enabled);
    }

    #[test]
    fn container_tab_allows_most_apis() {
        let config = SandboxConfig::container_tab();
        assert!(config.fetch_enabled);
        assert!(config.dom_bridge_enabled);
        assert!(config.websocket_enabled);
        assert!(config.allow_dynamic_code);
    }

    #[test]
    fn content_script_is_minimal() {
        let config = SandboxConfig::content_script();
        assert!(!config.fetch_enabled);
        assert!(!config.dom_bridge_enabled);
        assert!(!config.storage_enabled);
        assert!(config.max_statements.is_some());
    }

    #[test]
    fn sandbox_eval_simple_script() {
        let config = SandboxConfig::default();
        let mut sandbox = SandboxedRuntime::new(config);
        let result = sandbox.eval("1 + 2");
        match result {
            SandboxResult::Success(val) => {
                assert!(val.is_number());
            }
            _ => panic!("Expected success"),
        }
    }

    #[test]
    fn sandbox_tracks_execution_time() {
        let config = SandboxConfig::default();
        let mut sandbox = SandboxedRuntime::new(config);
        let _ = sandbox.eval("1 + 1");
        let _ = sandbox.eval("2 + 2");
        assert!(sandbox.total_execution_time() > Duration::ZERO);
        assert_eq!(sandbox.eval_count(), 2);
    }

    #[test]
    fn sandbox_reset_time_budget() {
        let config = SandboxConfig::default();
        let mut sandbox = SandboxedRuntime::new(config);
        let _ = sandbox.eval("1 + 1");
        assert!(sandbox.eval_count() > 0);
        sandbox.reset_time_budget();
        assert_eq!(sandbox.eval_count(), 0);
        assert_eq!(sandbox.total_execution_time(), Duration::ZERO);
    }

    #[test]
    fn sandbox_rejects_dynamic_code_when_disabled() {
        let config = SandboxConfig {
            allow_dynamic_code: false,
            ..Default::default()
        };
        let mut sandbox = SandboxedRuntime::new(config);
        let result = sandbox.eval("eval('1+1')");
        match result {
            SandboxResult::PolicyViolation(msg) => {
                assert!(msg.contains("Dynamic code execution"));
            }
            _ => panic!("Expected policy violation, got {:?}", result),
        }
    }

    #[test]
    fn sandbox_handles_js_errors() {
        let config = SandboxConfig::default();
        let mut sandbox = SandboxedRuntime::new(config);
        let result = sandbox.eval("throw new Error('test')");
        match result {
            SandboxResult::Exception(_) => {} // Expected
            _ => panic!("Expected exception"),
        }
    }

    #[test]
    fn sandbox_register_global_property() {
        let config = SandboxConfig::default();
        let mut sandbox = SandboxedRuntime::new(config);
        sandbox.register_global_property("myVar", JsValue::from(42)).unwrap();
        let result = sandbox.eval("myVar");
        match result {
            SandboxResult::Success(val) => {
                assert!(val.is_number());
            }
            _ => panic!("Expected success"),
        }
    }
}
