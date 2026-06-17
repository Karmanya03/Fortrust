use base64::Engine as _;
use boa_engine::{
    js_string, Context, JsError, JsNativeError, JsResult, JsValue,
    NativeFunction, object::FunctionObjectBuilder,
};

pub fn register(context: &mut Context) -> JsResult<()> {
    let atob_fn = unsafe {
        NativeFunction::from_closure(|_this, args, ctx| {
            let input = args
                .first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))
                .map_err(|e| {
                    JsError::from_native(
                        JsNativeError::typ().with_message(format!("atob: {e}")),
                    )
                })?;

            let bytes = base64::engine::general_purpose::STANDARD
                .decode(input.trim())
                .map_err(|e| {
                    JsError::from_native(
                        JsNativeError::typ()
                            .with_message(format!("atob: {e}")),
                    )
                })?;

            let result = String::from_utf8(bytes).map_err(|_| {
                JsError::from_native(
                    JsNativeError::typ()
                        .with_message("atob: decoded data is not valid UTF-8"),
                )
            })?;

            Ok(JsValue::from(js_string!(result.as_str())))
        })
    };

    let btoa_fn = unsafe {
        NativeFunction::from_closure(|_this, args, ctx| {
            let input = args
                .first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))
                .map_err(|e| {
                    JsError::from_native(
                        JsNativeError::typ().with_message(format!("btoa: {e}")),
                    )
                })?;

            let result = base64::engine::general_purpose::STANDARD.encode(input.as_bytes());

            Ok(JsValue::from(js_string!(result.as_str())))
        })
    };

    let global = context.global_object();
    let atob_val: JsValue = FunctionObjectBuilder::new(context.realm(), atob_fn)
        .build()
        .into();
    global.set(js_string!("atob"), atob_val, false, context)?;

    let btoa_val: JsValue = FunctionObjectBuilder::new(context.realm(), btoa_fn)
        .build()
        .into();
    global.set(js_string!("btoa"), btoa_val, false, context)?;

    Ok(())
}
