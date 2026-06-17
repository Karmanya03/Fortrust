use std::time::{SystemTime, UNIX_EPOCH};

use boa_engine::{
    js_string, Context, JsResult, JsValue, NativeFunction, object::ObjectInitializer,
};

pub fn register(context: &mut Context) -> JsResult<()> {
    let now_fn = unsafe {
        NativeFunction::from_closure(|_this, _args, _ctx| {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64() * 1000.0;
            Ok(JsValue::from(now))
        })
    };

    let timing = ObjectInitializer::new(context)
        .function(now_fn, js_string!("now"), 0)
        .build();

    context.register_global_property(
        js_string!("performance"),
        JsValue::from(timing),
        boa_engine::property::Attribute::all(),
    )?;

    Ok(())
}
