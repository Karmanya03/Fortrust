use boa_engine::{
    Context, JsResult, JsString, JsValue, js_string, object::ObjectInitializer, property::Attribute,
};

pub fn register(context: &mut Context, origin: &str) -> JsResult<()> {
    let url = url::Url::parse(origin).unwrap_or_else(|_| url::Url::parse("about:blank").unwrap());

    let location = ObjectInitializer::new(context)
        .property(
            js_string!("href"),
            JsString::from(url.as_str()),
            Attribute::all(),
        )
        .property(
            js_string!("protocol"),
            JsString::from(url.scheme()),
            Attribute::all(),
        )
        .property(
            js_string!("hostname"),
            JsString::from(url.host_str().unwrap_or("")),
            Attribute::all(),
        )
        .property(
            js_string!("host"),
            JsString::from(url.host_str().unwrap_or("")),
            Attribute::all(),
        )
        .property(
            js_string!("port"),
            JsString::from(url.port().map(|p| p.to_string()).unwrap_or_default()),
            Attribute::all(),
        )
        .property(
            js_string!("pathname"),
            JsString::from(url.path()),
            Attribute::all(),
        )
        .property(
            js_string!("search"),
            JsString::from(url.query().map(|q| format!("?{q}")).unwrap_or_default()),
            Attribute::all(),
        )
        .property(
            js_string!("hash"),
            JsString::from(url.fragment().map(|f| format!("#{f}")).unwrap_or_default()),
            Attribute::all(),
        )
        .property(
            js_string!("origin"),
            JsString::from(url.origin().ascii_serialization()),
            Attribute::all(),
        )
        .build();

    let global = context.global_object();
    global.set(
        js_string!("location"),
        JsValue::from(location),
        false,
        context,
    )?;

    Ok(())
}
