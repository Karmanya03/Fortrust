use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::rc::Rc;
use std::time::Duration;

use boa_engine::{
    Context, JsResult, JsValue, NativeFunction, js_string,
    object::FunctionObjectBuilder,
};
use tracing::debug;

use crate::event_loop::{EventLoop, TimerHandle, TimerKind};

thread_local! {
    static TIMER_STATE: std::cell::RefCell<Option<Rc<TimerState>>> = const { std::cell::RefCell::new(None) };
}

static NEXT_TIMER_ID: AtomicU64 = AtomicU64::new(1);

struct TimerState {
    event_loop: EventLoop,
    active_timeouts: std::cell::RefCell<HashMap<u64, TimerHandle>>,
    active_intervals: std::cell::RefCell<HashMap<u64, TimerHandle>>,
}

pub fn register(
    context: &mut Context,
    _next_id: &mut u64,
    _active_timeouts: &mut HashMap<u64, TimerHandle>,
    _active_intervals: &mut HashMap<u64, TimerHandle>,
    event_loop: &mut EventLoop,
) -> JsResult<()> {
    let state = Rc::new(TimerState {
        event_loop: event_loop.clone(),
        active_timeouts: std::cell::RefCell::new(HashMap::new()),
        active_intervals: std::cell::RefCell::new(HashMap::new()),
    });

    TIMER_STATE.with(|ts| {
        *ts.borrow_mut() = Some(state);
    });

    let set_timeout_fn = unsafe { NativeFunction::from_closure(|_this, args, _ctx| {
        let handler = args.first().cloned().unwrap_or(JsValue::undefined());
        let delay_ms = args
            .get(1)
            .and_then(|v| v.as_number())
            .unwrap_or(0.0)
            .max(0.0) as u64;

        if !handler.is_callable() {
            return Ok(JsValue::from(0));
        }

        let id = NEXT_TIMER_ID.fetch_add(1, Ordering::Relaxed);
        TIMER_STATE.with(|ts| {
            let state = ts.borrow();
            let state = state.as_ref().unwrap();
            let handle = state.event_loop.schedule_timer(
                id,
                Duration::from_millis(delay_ms),
                TimerKind::Timeout,
                handler,
            );
            state.active_timeouts.borrow_mut().insert(id, handle);
        });
        debug!(timer_id = id, delay_ms = delay_ms, "setTimeout registered");
        Ok(JsValue::from(id as f64))
    }) };

    let set_interval_fn = unsafe { NativeFunction::from_closure(|_this, args, _ctx| {
        let handler = args.first().cloned().unwrap_or(JsValue::undefined());
        let interval_ms = args
            .get(1)
            .and_then(|v| v.as_number())
            .unwrap_or(0.0)
            .max(0.0) as u64;

        if !handler.is_callable() {
            return Ok(JsValue::from(0));
        }

        let id = NEXT_TIMER_ID.fetch_add(1, Ordering::Relaxed);
        TIMER_STATE.with(|ts| {
            let state = ts.borrow();
            let state = state.as_ref().unwrap();
            let handle = state.event_loop.schedule_timer(
                id,
                Duration::from_millis(interval_ms),
                TimerKind::Interval,
                handler,
            );
            state.active_intervals.borrow_mut().insert(id, handle);
        });
        debug!(timer_id = id, interval_ms = interval_ms, "setInterval registered");
        Ok(JsValue::from(id as f64))
    }) };

    let clear_timeout_fn = unsafe { NativeFunction::from_closure(|_this, args, _ctx| {
        if let Some(id_val) = args.first().and_then(|v| v.as_number()) {
            let id = id_val as u64;
            TIMER_STATE.with(|ts| {
                let state = ts.borrow();
                let state = state.as_ref().unwrap();
                if let Some(handle) = state.active_timeouts.borrow_mut().remove(&id) {
                    state.event_loop.cancel_timer(handle);
                }
            });
            debug!(timer_id = id, "clearTimeout executed");
        }
        Ok(JsValue::undefined())
    }) };

    let clear_interval_fn = unsafe { NativeFunction::from_closure(|_this, args, _ctx| {
        if let Some(id_val) = args.first().and_then(|v| v.as_number()) {
            let id = id_val as u64;
            TIMER_STATE.with(|ts| {
                let state = ts.borrow();
                let state = state.as_ref().unwrap();
                if let Some(handle) = state.active_intervals.borrow_mut().remove(&id) {
                    state.event_loop.cancel_timer(handle);
                }
            });
            debug!(timer_id = id, "clearInterval executed");
        }
        Ok(JsValue::undefined())
    }) };

    let global = context.global_object();
    let set_timeout_val: JsValue = FunctionObjectBuilder::new(context.realm(), set_timeout_fn).build().into();
    let set_interval_val: JsValue = FunctionObjectBuilder::new(context.realm(), set_interval_fn).build().into();
    let clear_timeout_val: JsValue = FunctionObjectBuilder::new(context.realm(), clear_timeout_fn).build().into();
    let clear_interval_val: JsValue = FunctionObjectBuilder::new(context.realm(), clear_interval_fn).build().into();
    global.set(js_string!("setTimeout"), set_timeout_val, false, context)?;
    global.set(js_string!("setInterval"), set_interval_val, false, context)?;
    global.set(js_string!("clearTimeout"), clear_timeout_val, false, context)?;
    global.set(js_string!("clearInterval"), clear_interval_val, false, context)?;

    debug!("Timer Web API bindings registered");
    Ok(())
}
