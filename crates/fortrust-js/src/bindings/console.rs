use boa_engine::{
    Context, JsResult, JsValue, NativeFunction, js_string, object::ObjectInitializer,
};
use tracing::info;

pub fn register(context: &mut Context) -> JsResult<()> {
    let log_func = unsafe { NativeFunction::from_closure(console_log) };
    let warn_func = unsafe { NativeFunction::from_closure(console_warn) };
    let error_func = unsafe { NativeFunction::from_closure(console_error) };
    let info_func = unsafe { NativeFunction::from_closure(console_info) };

    let console = ObjectInitializer::new(context)
        .function(log_func, js_string!("log"), 1)
        .function(warn_func, js_string!("warn"), 1)
        .function(error_func, js_string!("error"), 1)
        .function(info_func, js_string!("info"), 1)
        .build();

    let global = context.global_object();
    let console_value = JsValue::from(console);
    global.set(js_string!("console"), console_value, false, context)?;

    Ok(())
}

fn format_args(args: &[JsValue], context: &mut Context) -> String {
    args.iter()
        .map(|arg| {
            arg.to_string(context)
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_else(|_| "[unprintable]".to_owned())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn console_log(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    info!("[JS console.log] {}", format_args(args, context));
    Ok(JsValue::undefined())
}

fn console_warn(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    info!("[JS console.warn] {}", format_args(args, context));
    Ok(JsValue::undefined())
}

fn console_error(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    info!("[JS console.error] {}", format_args(args, context));
    Ok(JsValue::undefined())
}

fn console_info(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    info!("[JS console.info] {}", format_args(args, context));
    Ok(JsValue::undefined())
}
