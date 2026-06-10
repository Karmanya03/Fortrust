use boa_engine::{
    Context, JsResult, JsValue, js_string, object::ObjectInitializer, object::builtins::JsArray,
    property::Attribute,
};
use fortrust_core::FingerprintGuard;

pub fn register(context: &mut Context, fingerprint_guard: Option<&FingerprintGuard>) -> JsResult<()> {
    let lang_arr = JsArray::new(context);
    lang_arr.push(JsValue::from(js_string!("en-US")), context)?;
    lang_arr.push(JsValue::from(js_string!("en")), context)?;

    let (user_agent, platform, hw_concurrency, device_mem) = if let Some(guard) = fingerprint_guard {
        let ua = guard
            .get_noisy_navigator_property("userAgent")
            .unwrap_or_else(|| "Fortrust/0.1 (Windows; x86_64) TrustEngine".to_owned());
        let plat = guard
            .platform_override
            .clone()
            .unwrap_or_else(|| "Win32".to_owned());
        (ua, plat, guard.hardware_concurrency as i32, guard.device_memory as i32)
    } else {
        (
            "Fortrust/0.1 (Windows; x86_64) TrustEngine".to_owned(),
            "Win32".to_owned(),
            4,
            8,
        )
    };

    let navigator = ObjectInitializer::new(context)
        .property(js_string!("userAgent"), JsValue::from(js_string!(user_agent.as_str())), Attribute::all())
        .property(js_string!("platform"), JsValue::from(js_string!(platform.as_str())), Attribute::all())
        .property(js_string!("language"), js_string!("en-US"), Attribute::all())
        .property(js_string!("languages"), JsValue::from(lang_arr), Attribute::all())
        .property(js_string!("cookieEnabled"), true, Attribute::all())
        .property(js_string!("doNotTrack"), js_string!("1"), Attribute::all())
        .property(js_string!("hardwareConcurrency"), hw_concurrency, Attribute::all())
        .property(js_string!("maxTouchPoints"), 0, Attribute::all())
        .property(js_string!("deviceMemory"), device_mem, Attribute::all())
        .build();

    let global = context.global_object();
    global.set(
        js_string!("navigator"),
        JsValue::from(navigator),
        false,
        context,
    )?;

    Ok(())
}
