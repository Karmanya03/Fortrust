//! DOM Event System — EventTarget, Event dispatch with capture/bubble phases.
//!
//! Implements the W3C DOM Events model:
//! - `EventListener` stores per-node listeners (callback_id + phase)
//! - `DomEvent` carries event state (type, target, propagation flags)
//! - `dispatch_event` walks capture → target → bubble phases

use std::collections::HashMap;
use std::sync::Mutex;

/// Identifies a JS callback registered via `addEventListener`.
pub type CallbackId = u64;

/// Which phase(s) a listener should fire in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerPhase {
    /// Only fire during the bubble phase (default for `addEventListener`).
    Bubble,
    /// Only fire during the capture phase.
    Capture,
    /// Fire in both phases (uncommon but spec-compliant).
    Both,
}

/// A single event listener attached to a node.
#[derive(Debug, Clone)]
pub struct EventListener {
    /// The event type this listener responds to (e.g. "click", "input").
    pub event_type: String,
    /// Identifier for the JS callback function (maps to a closure in the JS runtime).
    pub callback_id: CallbackId,
    /// Which phase(s) to fire in.
    pub phase: ListenerPhase,
    /// If true, the listener is automatically removed after firing once.
    pub once: bool,
}

/// Storage for all event listeners on a single node.
#[derive(Debug)]
pub struct EventListenerSet {
    listeners: Mutex<Vec<EventListener>>,
}

impl Default for EventListenerSet {
    fn default() -> Self {
        Self::new()
    }
}

impl EventListenerSet {
    pub fn new() -> Self {
        Self {
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// Add a listener. Duplicates (same event_type + callback_id + phase) are ignored.
    pub fn add(&self, listener: EventListener) {
        let mut list = self.listeners.lock().unwrap();
        let is_dup = list.iter().any(|existing| {
            existing.event_type == listener.event_type
                && existing.callback_id == listener.callback_id
                && existing.phase == listener.phase
        });
        if !is_dup {
            list.push(listener);
        }
    }

    /// Remove a listener matching the given event_type + callback_id + phase.
    pub fn remove(&self, event_type: &str, callback_id: CallbackId, phase: ListenerPhase) {
        self.listeners.lock().unwrap().retain(|l| {
            !(l.event_type == event_type && l.callback_id == callback_id && l.phase == phase)
        });
    }

    /// Remove all listeners for a given event type.
    pub fn remove_all_for_type(&self, event_type: &str) {
        self.listeners.lock().unwrap().retain(|l| l.event_type != event_type);
    }

    /// Get all listeners matching a specific event type and phase.
    pub fn get_matching(&self, event_type: &str, phase: ListenerPhase) -> Vec<EventListener> {
        self.listeners.lock().unwrap()
            .iter()
            .filter(|l| {
                l.event_type == event_type
                    && (l.phase == phase
                        || l.phase == ListenerPhase::Both
                        || phase == ListenerPhase::Both)
            })
            .cloned()
            .collect()
    }

    /// Get all listeners (for inspection/debugging).
    pub fn all(&self) -> Vec<EventListener> {
        self.listeners.lock().unwrap().clone()
    }

    /// Count of registered listeners.
    pub fn len(&self) -> usize {
        self.listeners.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.listeners.lock().unwrap().is_empty()
    }

    /// Remove `once` listeners that have been marked for removal.
    pub fn remove_once_fired(&self, event_type: &str, callback_ids: &[CallbackId]) {
        self.listeners.lock().unwrap().retain(|l| {
            !(l.once
                && l.event_type == event_type
                && callback_ids.contains(&l.callback_id))
        });
    }
}

/// The current phase of event propagation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPhase {
    /// Not currently dispatching.
    None,
    /// Capturing from root down to target's parent.
    Capturing,
    /// At the target node.
    AtTarget,
    /// Bubbling from target's parent up to root.
    Bubbling,
}

/// A DOM event object passed to event handlers.
#[derive(Debug, Clone)]
pub struct DomEvent {
    /// The event type (e.g. "click", "input", "keydown").
    pub event_type: String,
    /// Whether the event bubbles up through the DOM.
    pub bubbles: bool,
    /// Whether the default action can be prevented.
    pub cancelable: bool,
    /// Current propagation phase.
    pub phase: EventPhase,
    /// Whether `preventDefault()` was called.
    pub default_prevented: bool,
    /// Whether `stopPropagation()` was called.
    pub propagation_stopped: bool,
    /// Whether `stopImmediatePropagation()` was called.
    pub immediate_stopped: bool,
    /// Pointer to the target node (as usize for lifetime flexibility).
    pub target_ptr: usize,
    /// Pointer to the currentTarget node (changes during dispatch).
    pub current_target_ptr: usize,
    /// Event-specific detail value (e.g. click count, key code).
    pub detail: EventDetail,
}

/// Additional event-specific data.
#[derive(Debug, Clone, PartialEq)]
pub enum EventDetail {
    /// No extra data (generic events).
    None,
    /// Mouse/pointer events: (client_x, client_y).
    Mouse { client_x: f32, client_y: f32 },
    /// Keyboard events: key string and key code.
    Keyboard { key: String, code: String },
    /// Input events: input value.
    Input { value: String },
    /// Custom numeric detail.
    Custom(i32),
}

impl DomEvent {
    /// Create a new event with the given type.
    pub fn new(event_type: impl Into<String>, bubbles: bool, cancelable: bool) -> Self {
        Self {
            event_type: event_type.into(),
            bubbles,
            cancelable,
            phase: EventPhase::None,
            default_prevented: false,
            propagation_stopped: false,
            immediate_stopped: false,
            target_ptr: 0,
            current_target_ptr: 0,
            detail: EventDetail::None,
        }
    }

    /// Create a bubbling, cancelable event (common default).
    pub fn bubbling(event_type: impl Into<String>) -> Self {
        Self::new(event_type, true, true)
    }

    /// Create a click event with coordinates.
    pub fn click(client_x: f32, client_y: f32) -> Self {
        Self {
            event_type: "click".into(),
            bubbles: true,
            cancelable: true,
            phase: EventPhase::None,
            default_prevented: false,
            propagation_stopped: false,
            immediate_stopped: false,
            target_ptr: 0,
            current_target_ptr: 0,
            detail: EventDetail::Mouse {
                client_x,
                client_y,
            },
        }
    }

    /// Create a keyboard event.
    pub fn keyboard(event_type: impl Into<String>, key: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            bubbles: true,
            cancelable: true,
            phase: EventPhase::None,
            default_prevented: false,
            propagation_stopped: false,
            immediate_stopped: false,
            target_ptr: 0,
            current_target_ptr: 0,
            detail: EventDetail::Keyboard {
                key: key.into(),
                code: code.into(),
            },
        }
    }

    /// Create an input event.
    pub fn input(value: impl Into<String>) -> Self {
        Self {
            event_type: "input".into(),
            bubbles: true,
            cancelable: false,
            phase: EventPhase::None,
            default_prevented: false,
            propagation_stopped: false,
            immediate_stopped: false,
            target_ptr: 0,
            current_target_ptr: 0,
            detail: EventDetail::Input {
                value: value.into(),
            },
        }
    }

    /// Call `preventDefault()` — marks the event so the default action is skipped.
    pub fn prevent_default(&mut self) {
        if self.cancelable {
            self.default_prevented = true;
        }
    }

    /// Call `stopPropagation()` — stops the event from propagating to further nodes.
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    /// Call `stopImmediatePropagation()` — stops propagation AND prevents other
    /// listeners on the same node from firing.
    pub fn stop_immediate_propagation(&mut self) {
        self.propagation_stopped = true;
        self.immediate_stopped = true;
    }
}

/// Result of dispatching an event.
#[derive(Debug, Clone)]
pub struct DispatchResult {
    /// Whether `preventDefault()` was called by any handler.
    pub default_prevented: bool,
    /// IDs of callbacks that were actually invoked.
    pub fired_callbacks: Vec<CallbackId>,
}

/// Trait for invoking a JS callback during event dispatch.
///
/// The implementation bridges to the JS runtime to call the actual listener closure.
/// Returns `true` if the callback called `stopPropagation` or `stopImmediatePropagation`.
pub trait EventCallbackInvoker {
    /// Invoke the callback identified by `callback_id` with the given event.
    /// Returns a `CallbackResult` indicating what the callback requested.
    fn invoke(&mut self, callback_id: CallbackId, event: &DomEvent) -> CallbackResult;
}

/// Result returned by a single callback invocation.
#[derive(Debug, Clone, Default)]
pub struct CallbackResult {
    pub prevent_default: bool,
    pub stop_propagation: bool,
    pub stop_immediate: bool,
}

/// A no-op invoker for testing — always succeeds without side effects.
#[derive(Debug, Default)]
pub struct NoopInvoker;

impl EventCallbackInvoker for NoopInvoker {
    fn invoke(&mut self, _callback_id: CallbackId, _event: &DomEvent) -> CallbackResult {
        CallbackResult::default()
    }
}

/// Dispatch an event through the DOM tree rooted at the given ancestor chain.
///
/// `ancestor_chain` must be ordered from the root (document) down to and including
/// the target node. Typically built by walking from the target up to the root and
/// then reversing.
///
/// The dispatch algorithm:
/// 1. **Capture phase**: Walk from root to target's parent, firing capture listeners.
/// 2. **Target phase**: Fire all listeners (both capture and bubble) on the target.
/// 3. **Bubble phase**: Walk from target's parent up to root, firing bubble listeners
///    (only if `event.bubbles` is true).
pub fn dispatch_event_chain(
    ancestor_chain: &[(usize, &EventListenerSet)],
    target_idx: usize,
    event: &mut DomEvent,
    invoker: &mut dyn EventCallbackInvoker,
) -> DispatchResult {
    let mut fired = Vec::new();
    event.target_ptr = ancestor_chain
        .get(target_idx)
        .map(|(ptr, _)| *ptr)
        .unwrap_or(0);

    // Phase 1: Capture (root → parent of target)
    event.phase = EventPhase::Capturing;
    for &(node_ptr, listeners) in &ancestor_chain[..target_idx] {
        if event.propagation_stopped {
            break;
        }
        event.current_target_ptr = node_ptr;
        let matching = listeners.get_matching(&event.event_type, ListenerPhase::Capture);
        let once_ids: Vec<CallbackId> = matching.iter().filter(|l| l.once).map(|l| l.callback_id).collect();
        for listener in &matching {
            if event.immediate_stopped {
                break;
            }
            let result = invoker.invoke(listener.callback_id, event);
            fired.push(listener.callback_id);
            apply_callback_result(event, &result);
        }
        if !once_ids.is_empty() {
            listeners.remove_once_fired(&event.event_type, &once_ids);
        }
    }

    // Phase 2: At target — fire all listeners regardless of phase
    if !event.propagation_stopped
        && let Some((node_ptr, listeners)) = ancestor_chain.get(target_idx) {
            event.phase = EventPhase::AtTarget;
            event.current_target_ptr = *node_ptr;
            let matching = listeners.get_matching(&event.event_type, ListenerPhase::Both);
            let once_ids: Vec<CallbackId> = matching.iter().filter(|l| l.once).map(|l| l.callback_id).collect();
            for listener in &matching {
                if event.immediate_stopped {
                    break;
                }
                let result = invoker.invoke(listener.callback_id, event);
                fired.push(listener.callback_id);
                apply_callback_result(event, &result);
            }
            if !once_ids.is_empty() {
                listeners.remove_once_fired(&event.event_type, &once_ids);
            }
        }

    // Phase 3: Bubble (parent of target → root), only if event.bubbles
    if event.bubbles && !event.propagation_stopped {
        event.phase = EventPhase::Bubbling;
        for &(node_ptr, listeners) in ancestor_chain[..target_idx].iter().rev() {
            if event.propagation_stopped {
                break;
            }
            event.current_target_ptr = node_ptr;
            let matching = listeners.get_matching(&event.event_type, ListenerPhase::Bubble);
            let once_ids: Vec<CallbackId> = matching.iter().filter(|l| l.once).map(|l| l.callback_id).collect();
            for listener in &matching {
                if event.immediate_stopped {
                    break;
                }
                let result = invoker.invoke(listener.callback_id, event);
                fired.push(listener.callback_id);
                apply_callback_result(event, &result);
            }
            if !once_ids.is_empty() {
                listeners.remove_once_fired(&event.event_type, &once_ids);
            }
        }
    }

    event.phase = EventPhase::None;
    DispatchResult {
        default_prevented: event.default_prevented,
        fired_callbacks: fired,
    }
}

fn apply_callback_result(event: &mut DomEvent, result: &CallbackResult) {
    if result.prevent_default {
        event.prevent_default();
    }
    if result.stop_propagation {
        event.stop_propagation();
    }
    if result.stop_immediate {
        event.stop_immediate_propagation();
    }
}

/// Global callback map: maps callback_id → (event_type, node_ptr).
#[derive(Debug)]
pub struct EventCallbackRegistry {
    next_id: Mutex<CallbackId>,
    callbacks: Mutex<HashMap<CallbackId, CallbackInfo>>,
}

impl Default for EventCallbackRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a registered callback.
#[derive(Debug, Clone)]
pub struct CallbackInfo {
    pub event_type: String,
    pub node_ptr: usize,
}

impl EventCallbackRegistry {
    pub fn new() -> Self {
        Self {
            next_id: Mutex::new(1),
            callbacks: Mutex::new(HashMap::new()),
        }
    }

    /// Register a new callback and return its unique ID.
    pub fn register(&self, event_type: String, node_ptr: usize) -> CallbackId {
        let mut next = self.next_id.lock().unwrap();
        let id = *next;
        *next += 1;
        self.callbacks.lock().unwrap().insert(
            id,
            CallbackInfo {
                event_type,
                node_ptr,
            },
        );
        id
    }

    /// Unregister a callback by ID.
    pub fn unregister(&self, id: CallbackId) -> Option<CallbackInfo> {
        self.callbacks.lock().unwrap().remove(&id)
    }

    /// Look up callback info.
    pub fn get(&self, id: CallbackId) -> Option<CallbackInfo> {
        self.callbacks.lock().unwrap().get(&id).cloned()
    }

    /// Count of registered callbacks.
    pub fn len(&self) -> usize {
        self.callbacks.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.callbacks.lock().unwrap().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_listener_set_add_and_get() {
        let set = EventListenerSet::new();
        set.add(EventListener {
            event_type: "click".into(),
            callback_id: 1,
            phase: ListenerPhase::Bubble,
            once: false,
        });
        set.add(EventListener {
            event_type: "click".into(),
            callback_id: 2,
            phase: ListenerPhase::Capture,
            once: false,
        });

        let bubble = set.get_matching("click", ListenerPhase::Bubble);
        assert_eq!(bubble.len(), 1);
        assert_eq!(bubble[0].callback_id, 1);

        let capture = set.get_matching("click", ListenerPhase::Capture);
        assert_eq!(capture.len(), 1);
        assert_eq!(capture[0].callback_id, 2);
    }

    #[test]
    fn event_listener_set_deduplicates() {
        let set = EventListenerSet::new();
        let listener = EventListener {
            event_type: "click".into(),
            callback_id: 1,
            phase: ListenerPhase::Bubble,
            once: false,
        };
        set.add(listener.clone());
        set.add(listener);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn event_listener_set_remove() {
        let set = EventListenerSet::new();
        set.add(EventListener {
            event_type: "click".into(),
            callback_id: 1,
            phase: ListenerPhase::Bubble,
            once: false,
        });
        set.remove("click", 1, ListenerPhase::Bubble);
        assert!(set.is_empty());
    }

    #[test]
    fn event_prevent_default_only_when_cancelable() {
        let mut event = DomEvent::new("click", true, true);
        event.prevent_default();
        assert!(event.default_prevented);

        let mut event2 = DomEvent::new("scroll", true, false);
        event2.prevent_default();
        assert!(!event2.default_prevented);
    }

    #[test]
    fn dispatch_fires_capture_then_bubble() {
        // Build a chain: root(0) → parent(1) → target(2)
        let root_set = EventListenerSet::new();
        let parent_set = EventListenerSet::new();
        let target_set = EventListenerSet::new();

        let mut call_order = Vec::new();

        // Root: capture listener (id=1), bubble listener (id=2)
        root_set.add(EventListener {
            event_type: "click".into(),
            callback_id: 1,
            phase: ListenerPhase::Capture,
            once: false,
        });
        root_set.add(EventListener {
            event_type: "click".into(),
            callback_id: 2,
            phase: ListenerPhase::Bubble,
            once: false,
        });

        // Parent: capture (id=3), bubble (id=4)
        parent_set.add(EventListener {
            event_type: "click".into(),
            callback_id: 3,
            phase: ListenerPhase::Capture,
            once: false,
        });
        parent_set.add(EventListener {
            event_type: "click".into(),
            callback_id: 4,
            phase: ListenerPhase::Bubble,
            once: false,
        });

        // Target: bubble listener (id=5) — fires during AtTarget phase
        target_set.add(EventListener {
            event_type: "click".into(),
            callback_id: 5,
            phase: ListenerPhase::Bubble,
            once: false,
        });

        struct OrderTracker<'a> {
            order: &'a mut Vec<CallbackId>,
        }
        impl EventCallbackInvoker for OrderTracker<'_> {
            fn invoke(&mut self, callback_id: CallbackId, _event: &DomEvent) -> CallbackResult {
                self.order.push(callback_id);
                CallbackResult::default()
            }
        }

        let chain: Vec<(usize, &EventListenerSet)> = vec![
            (100, &root_set),
            (200, &parent_set),
            (300, &target_set),
        ];

        let mut event = DomEvent::click(10.0, 20.0);
        let mut tracker = OrderTracker {
            order: &mut call_order,
        };
        let result = dispatch_event_chain(&chain, 2, &mut event, &mut tracker);

        // Expected: capture(1) → capture(3) → at_target(5) → bubble(4) → bubble(2)
        assert_eq!(result.fired_callbacks, vec![1, 3, 5, 4, 2]);
        assert!(!result.default_prevented);
    }

    #[test]
    fn dispatch_stop_propagation_halts_bubbling() {
        let root_set = EventListenerSet::new();
        let target_set = EventListenerSet::new();

        root_set.add(EventListener {
            event_type: "click".into(),
            callback_id: 1,
            phase: ListenerPhase::Bubble,
            once: false,
        });

        target_set.add(EventListener {
            event_type: "click".into(),
            callback_id: 2,
            phase: ListenerPhase::Bubble,
            once: false,
        });

        struct StopInvoker;
        impl EventCallbackInvoker for StopInvoker {
            fn invoke(&mut self, callback_id: CallbackId, _event: &DomEvent) -> CallbackResult {
                if callback_id == 2 {
                    CallbackResult {
                        stop_propagation: true,
                        ..Default::default()
                    }
                } else {
                    CallbackResult::default()
                }
            }
        }

        let chain: Vec<(usize, &EventListenerSet)> =
            vec![(100, &root_set), (200, &target_set)];

        let mut event = DomEvent::click(0.0, 0.0);
        let result = dispatch_event_chain(&chain, 1, &mut event, &mut StopInvoker);

        // Only target listener should have fired; root bubble should be skipped
        assert_eq!(result.fired_callbacks, vec![2]);
    }

    #[test]
    fn dispatch_once_listener_is_removed() {
        let set = EventListenerSet::new();
        set.add(EventListener {
            event_type: "click".into(),
            callback_id: 42,
            phase: ListenerPhase::Bubble,
            once: true,
        });

        let chain: Vec<(usize, &EventListenerSet)> = vec![(100, &set)];
        let mut event = DomEvent::click(0.0, 0.0);
        let _ = dispatch_event_chain(&chain, 0, &mut event, &mut NoopInvoker);

        // After dispatch, the once listener should be gone
        assert!(set.is_empty());
    }

    #[test]
    fn non_bubbling_event_does_not_bubble() {
        let root_set = EventListenerSet::new();
        let target_set = EventListenerSet::new();

        root_set.add(EventListener {
            event_type: "focus".into(),
            callback_id: 1,
            phase: ListenerPhase::Bubble,
            once: false,
        });

        target_set.add(EventListener {
            event_type: "focus".into(),
            callback_id: 2,
            phase: ListenerPhase::Bubble,
            once: false,
        });

        let chain: Vec<(usize, &EventListenerSet)> =
            vec![(100, &root_set), (200, &target_set)];

        let mut event = DomEvent::new("focus", false, true); // non-bubbling
        let result = dispatch_event_chain(&chain, 1, &mut event, &mut NoopInvoker);

        // Only target listener fires; root bubble listener should NOT fire
        assert_eq!(result.fired_callbacks, vec![2]);
    }

    #[test]
    fn callback_registry_register_and_get() {
        let reg = EventCallbackRegistry::new();
        let id = reg.register("click".into(), 12345);
        assert_eq!(id, 1);
        let info = reg.get(id).unwrap();
        assert_eq!(info.event_type, "click");
        assert_eq!(info.node_ptr, 12345);
    }
}
