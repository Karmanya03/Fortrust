use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use boa_engine::{
    Context, JsResult, JsString, JsValue, NativeFunction, js_string, object::ObjectInitializer,
    property::Attribute,
};

type StorageMap = Arc<Mutex<HashMap<String, String>>>;

thread_local! {
    static LOCAL_STORAGE: StorageMap = Arc::new(Mutex::new(HashMap::new()));
    static SESSION_STORAGE: StorageMap = Arc::new(Mutex::new(HashMap::new()));
}

pub fn register(context: &mut Context) -> JsResult<()> {
    let local = LOCAL_STORAGE.with(|s| build_storage_object(context, "localStorage", s))?;
    context.register_global_property(js_string!("localStorage"), local, Attribute::all())?;

    let session = SESSION_STORAGE.with(|s| build_storage_object(context, "sessionStorage", s))?;
    context.register_global_property(js_string!("sessionStorage"), session, Attribute::all())?;

    Ok(())
}

fn build_storage_object(
    context: &mut Context,
    _name: &str,
    storage: &StorageMap,
) -> JsResult<JsValue> {
    let storage_clone = storage.clone();
    let get_item_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let key = args
                .first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            let map = storage_clone.lock().unwrap();
            match map.get(&key) {
                Some(value) => Ok(JsValue::from(JsString::from(value.as_str()))),
                None => Ok(JsValue::null()),
            }
        })
    };

    let storage_clone2 = storage.clone();
    let set_item_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let key = args
                .first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            let value = args
                .get(1)
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            let mut map = storage_clone2.lock().unwrap();
            map.insert(key, value);
            Ok(JsValue::undefined())
        })
    };

    let storage_clone3 = storage.clone();
    let remove_item_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let key = args
                .first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            let mut map = storage_clone3.lock().unwrap();
            map.remove(&key);
            Ok(JsValue::undefined())
        })
    };

    let storage_clone4 = storage.clone();
    let clear_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let mut map = storage_clone4.lock().unwrap();
            map.clear();
            Ok(JsValue::undefined())
        })
    };

    let storage_clone5 = storage.clone();
    let length_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = storage_clone5.lock().unwrap();
            Ok(JsValue::from(map.len() as i32))
        })
    };

    let storage_clone6 = storage.clone();
    let key_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let index = args
                .first()
                .and_then(|v| v.as_number())
                .map(|n| n as usize)
                .unwrap_or(0);
            let map = storage_clone6.lock().unwrap();
            match map.keys().nth(index) {
                Some(key) => Ok(JsValue::from(JsString::from(key.as_str()))),
                None => Ok(JsValue::null()),
            }
        })
    };

    let obj = ObjectInitializer::new(context)
        .function(get_item_fn, js_string!("getItem"), 1)
        .function(set_item_fn, js_string!("setItem"), 2)
        .function(remove_item_fn, js_string!("removeItem"), 1)
        .function(clear_fn, js_string!("clear"), 0)
        .function(length_fn, js_string!("length"), 0)
        .function(key_fn, js_string!("key"), 1)
        .build();

    Ok(JsValue::from(obj))
}
