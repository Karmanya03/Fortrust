use boa_engine::{
    Context, JsResult, JsValue, js_string,
    object::ObjectInitializer,
    object::builtins::JsArray,
    property::Attribute,
};

pub fn register(context: &mut Context) -> JsResult<()> {
    let lang_arr = JsArray::new(context);
    lang_arr.push(JsValue::from(js_string!("en-US")), context)?;
    lang_arr.push(JsValue::from(js_string!("en")), context)?;

    let navigator = ObjectInitializer::new(context)
        .property(js_string!("userAgent"), js_string!("Fortrust/0.1 (Windows; x86_64) TrustEngine"), Attribute::all())
        .property(js_string!("platform"), js_string!("Win32"), Attribute::all())
        .property(js_string!("language"), js_string!("en-US"), Attribute::all())
        .property(js_string!("languages"), JsValue::from(lang_arr), Attribute::all())
        .property(js_string!("cookieEnabled"), true, Attribute::all())
        .property(js_string!("doNotTrack"), js_string!("1"), Attribute::all())
        .property(js_string!("hardwareConcurrency"), 4, Attribute::all())
        .property(js_string!("maxTouchPoints"), 0, Attribute::all())
        .build();

    let global = context.global_object();
    global.set(js_string!("navigator"), JsValue::from(navigator), false, context)?;

    Ok(())
}
