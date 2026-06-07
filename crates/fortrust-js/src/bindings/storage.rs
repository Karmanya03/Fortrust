use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use boa_engine::{
    Context, JsResult, JsString, JsValue, NativeFunction, js_string, object::ObjectInitializer,
    property::Attribute,
};
use fortrust_storage::LocalStorageStore;

type StorageMap = Arc<Mutex<HashMap<String, String>>>;

thread_local! {
    static SESSION_STORAGE: StorageMap = Arc::new(Mutex::new(HashMap::new()));
}

fn build_storage_object(
    context: &mut Context,
    persistent: Option<LocalStorageStore>,
) -> JsResult<JsValue> {
    let get_item_fn = if let Some(store) = persistent.clone() {
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let key = args
                    .first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))?;
                match store.get(&key) {
                    Ok(Some(value)) => Ok(JsValue::from(JsString::from(value.as_str()))),
                    _ => Ok(JsValue::null()),
                }
            })
        }
    } else {
        let map: StorageMap = Arc::new(Mutex::new(HashMap::new()));
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let key = args
                    .first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))?;
                let map = map.lock().unwrap();
                match map.get(&key) {
                    Some(value) => Ok(JsValue::from(JsString::from(value.as_str()))),
                    None => Ok(JsValue::null()),
                }
            })
        }
    };

    let set_item_fn = if let Some(store) = persistent.clone() {
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let key = args
                    .first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))?;
                let value = args
                    .get(1)
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))?;
                let _ = store.set(&key, &value);
                Ok(JsValue::undefined())
            })
        }
    } else {
        let map: StorageMap = Arc::new(Mutex::new(HashMap::new()));
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let key = args
                    .first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))?;
                let value = args
                    .get(1)
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))?;
                let mut map = map.lock().unwrap();
                map.insert(key, value);
                Ok(JsValue::undefined())
            })
        }
    };

    let remove_item_fn = if let Some(store) = persistent.clone() {
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let key = args
                    .first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))?;
                let _ = store.remove(&key);
                Ok(JsValue::undefined())
            })
        }
    } else {
        let map: StorageMap = Arc::new(Mutex::new(HashMap::new()));
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let key = args
                    .first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))?;
                let mut map = map.lock().unwrap();
                map.remove(&key);
                Ok(JsValue::undefined())
            })
        }
    };

    let clear_fn = if let Some(store) = persistent.clone() {
        unsafe {
            NativeFunction::from_closure(move |_this, _args, _ctx| {
                let _ = store.clear();
                Ok(JsValue::undefined())
            })
        }
    } else {
        let map: StorageMap = Arc::new(Mutex::new(HashMap::new()));
        unsafe {
            NativeFunction::from_closure(move |_this, _args, _ctx| {
                let mut map = map.lock().unwrap();
                map.clear();
                Ok(JsValue::undefined())
            })
        }
    };

    let length_fn = if let Some(store) = persistent.clone() {
        unsafe {
            NativeFunction::from_closure(move |_this, _args, _ctx| {
                let len = store.len().unwrap_or(0);
                Ok(JsValue::from(len as i32))
            })
        }
    } else {
        let map: StorageMap = Arc::new(Mutex::new(HashMap::new()));
        unsafe {
            NativeFunction::from_closure(move |_this, _args, _ctx| {
                let map = map.lock().unwrap();
                Ok(JsValue::from(map.len() as i32))
            })
        }
    };

    let key_fn = if let Some(store) = persistent.clone() {
        unsafe {
            NativeFunction::from_closure(move |_this, args, _ctx| {
                let index = args
                    .first()
                    .and_then(|v| v.as_number())
                    .map(|n| n as usize)
                    .unwrap_or(0);
                match store.key_at(index) {
                    Ok(Some(key)) => Ok(JsValue::from(JsString::from(key.as_str()))),
                    _ => Ok(JsValue::null()),
                }
            })
        }
    } else {
        let map: StorageMap = Arc::new(Mutex::new(HashMap::new()));
        unsafe {
            NativeFunction::from_closure(move |_this, args, _ctx| {
                let index = args
                    .first()
                    .and_then(|v| v.as_number())
                    .map(|n| n as usize)
                    .unwrap_or(0);
                let map = map.lock().unwrap();
                match map.keys().nth(index) {
                    Some(key) => Ok(JsValue::from(JsString::from(key.as_str()))),
                    None => Ok(JsValue::null()),
                }
            })
        }
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

pub fn register_with_backend(
    context: &mut Context,
    local_storage: Option<LocalStorageStore>,
) -> JsResult<()> {
    let local = build_storage_object(context, local_storage)?;
    context.register_global_property(js_string!("localStorage"), local, Attribute::all())?;

    let session = build_storage_object(context, None)?;
    context.register_global_property(js_string!("sessionStorage"), session, Attribute::all())?;

    Ok(())
}

pub fn register(context: &mut Context) -> JsResult<()> {
    register_with_backend(context, None)
}
