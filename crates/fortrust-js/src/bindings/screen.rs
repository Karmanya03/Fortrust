use boa_engine::{
    Context, JsResult, JsValue, js_string, object::ObjectInitializer,
    property::Attribute,
};
use fortrust_core::FingerprintGuard;

pub fn register(context: &mut Context, fingerprint_guard: Option<&FingerprintGuard>) -> JsResult<()> {
    let (width, height) = fingerprint_guard
        .map(|g| g.get_screen_resolution())
        .unwrap_or((1920, 1080));
    let avail_height = height.saturating_sub(40);

    let screen = ObjectInitializer::new(context)
        .property(js_string!("width"), width as i32, Attribute::all())
        .property(js_string!("height"), height as i32, Attribute::all())
        .property(js_string!("availWidth"), width as i32, Attribute::all())
        .property(js_string!("availHeight"), avail_height as i32, Attribute::all())
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
