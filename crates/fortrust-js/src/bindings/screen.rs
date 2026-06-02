use boa_engine::{
    Context, JsResult, JsValue, js_string, object::ObjectInitializer,
    property::Attribute,
};

pub fn register(context: &mut Context) -> JsResult<()> {
    // Spoofed screen dimensions for anti-fingerprinting
    let screen = ObjectInitializer::new(context)
        .property(js_string!("width"), 1920, Attribute::all())
        .property(js_string!("height"), 1080, Attribute::all())
        .property(js_string!("availWidth"), 1920, Attribute::all())
        .property(js_string!("availHeight"), 1040, Attribute::all())
        .property(js_string!("colorDepth"), 24, Attribute::all())
        .property(js_string!("pixelDepth"), 24, Attribute::all())
        .build();

    let global = context.global_object();
    global.set(
        js_string!("screen"),
        JsValue::from(screen),
        false,
        context,
    )?;

    Ok(())
}
