use std::cell::RefCell;
use std::rc::Rc;

use boa_engine::{
    js_string, Context, JsError, JsNativeError, JsResult, JsValue,
    NativeFunction, object::FunctionObjectBuilder,
};

use crate::event_loop::TaskQueue;

thread_local! {
    static MICROTASK_QUEUE: RefCell<Option<Rc<RefCell<TaskQueue>>>> = const { RefCell::new(None) };
}

pub fn initialize(queue: Rc<RefCell<TaskQueue>>) {
    MICROTASK_QUEUE.with(|cell| {
        *cell.borrow_mut() = Some(queue);
    });
}

pub fn register(context: &mut Context) -> JsResult<()> {
    let queue_microtask_fn = unsafe {
        NativeFunction::from_closure(|_this, args, _ctx| {
            let callback = args.first().ok_or_else(|| {
                JsError::from_native(
                    JsNativeError::typ()
                        .with_message("queueMicrotask: 1 argument required"),
                )
            })?;

            if !callback.is_callable() {
                return Err(JsError::from_native(
                    JsNativeError::typ()
                        .with_message("queueMicrotask: argument must be a function"),
                ));
            }

            MICROTASK_QUEUE.with(|cell| {
                if let Some(ref queue) = *cell.borrow() {
                    if let Ok(mut q) = queue.try_borrow_mut() {
                        q.enqueue_microtask(callback.clone());
                    }
                }
            });

            Ok(JsValue::undefined())
        })
    };

    let global = context.global_object();
    let val: JsValue = FunctionObjectBuilder::new(context.realm(), queue_microtask_fn)
        .build()
        .into();
    global.set(js_string!("queueMicrotask"), val, false, context)?;

    Ok(())
}
