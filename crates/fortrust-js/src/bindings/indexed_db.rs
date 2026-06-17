use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use boa_engine::{
    js_string, Context, JsError, JsNativeError, JsResult, JsValue,
    NativeFunction, object::ObjectInitializer,
};
use fortrust_storage::{IndexedDbStore, StorageError};

struct IdbState {
    store: Arc<IndexedDbStore>,
    open_dbs: Mutex<HashMap<String, bool>>,
}

impl Clone for IdbState {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            open_dbs: Mutex::new(HashMap::new()),
        }
    }
}

thread_local! {
    static IDB: std::cell::RefCell<Option<IdbState>> = const { std::cell::RefCell::new(None) };
}

pub fn initialize(store: Arc<IndexedDbStore>) {
    IDB.with(|cell| {
        *cell.borrow_mut() = Some(IdbState {
            store,
            open_dbs: Mutex::new(HashMap::new()),
        });
    });
}

fn with_store<R>(f: impl FnOnce(&IndexedDbStore) -> Result<R, StorageError>) -> Result<R, String> {
    IDB.with(|cell| {
        let guard = cell.borrow();
        let state = guard.as_ref().ok_or_else(|| "IndexedDB not initialized".to_owned())?;
        f(&state.store).map_err(|e| e.to_string())
    })
}

fn to_js_err(msg: String) -> JsError {
    JsError::from_native(JsNativeError::typ().with_message(msg))
}

fn store_result<T>(r: Result<T, String>) -> JsResult<T> {
    r.map_err(to_js_err)
}

fn open_db(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let db_name = args
        .first()
        .map(|v| v.to_string(context).map(|s| s.to_std_string_escaped()))
        .unwrap_or(Ok("default".to_owned()))
        .map_err(|e| to_js_err(e.to_string()))?;
    let version = args
        .get(1)
        .and_then(|v| v.as_number())
        .map(|n| n as u32)
        .unwrap_or(1);

    let existing = store_result(with_store(|store| store.get_database(&db_name)))?;
    let needs_upgrade = existing.as_ref().map(|d| d.version != version).unwrap_or(true);

    if existing.is_none() || needs_upgrade {
        let db = fortrust_storage::indexed_db::IdbDatabase {
            name: db_name.clone(),
            version,
            store_names: existing
                .as_ref()
                .map(|d| d.store_names.clone())
                .unwrap_or_default(),
        };
        store_result(with_store(|store| store.put_database(&db)))?;
    }

    build_database_object(context, &db_name, version)
}

fn build_database_object(
    context: &mut Context,
    db_name: &str,
    version: u32,
) -> JsResult<JsValue> {
    let name = db_name.to_owned();
    let close_fn = {
        let name = name.clone();
        unsafe {
            NativeFunction::from_closure(move |_this, _args, _ctx| {
                IDB.with(|cell| {
                    if let Some(ref state) = *cell.borrow() {
                        if let Ok(mut dbs) = state.open_dbs.lock() {
                            let _ = dbs.remove(&name);
                        }
                    }
                });
                Ok(JsValue::undefined())
            })
        }
    };

    let create_store_fn = {
        let name = name.clone();
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let store_name = args
                    .first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))
                    .map_err(|e| to_js_err(e.to_string()))?;
                let mut db = store_result(with_store(|store| store.get_database(&name)))?
                    .ok_or_else(|| to_js_err("Database not found".to_owned()))?;
                if !db.store_names.contains(&store_name) {
                    db.store_names.push(store_name.clone());
                    store_result(with_store(|store| store.put_database(&db)))?;
                }
                build_object_store_object(ctx, &name, &store_name)
            })
        }
    };

    let transaction_fn = {
        let name = name.clone();
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let store_names_val = args
                    .first()
                    .ok_or_else(|| to_js_err("storeNames required".to_owned()))?;
                let store_names = if let Some(arr) = store_names_val.as_object() {
                    let mut names = Vec::new();
                    let len = arr.get(js_string!("length"), ctx)
                        .ok()
                        .and_then(|v| v.as_number())
                        .unwrap_or(0.0) as i32;
                    for i in 0..len {
                        if let Ok(elem) = arr.get(i, ctx) {
                            if let Some(s) = elem.as_string() {
                                names.push(s.to_std_string_escaped());
                            }
                        }
                    }
                    names
                } else if let Some(s) = store_names_val.as_string() {
                    vec![s.to_std_string_escaped()]
                } else {
                    return Err(to_js_err("Invalid storeNames".to_owned()));
                };
                build_transaction_object(ctx, &name, &store_names)
            })
        }
    };

    let obj = ObjectInitializer::new(context)
        .property(
            js_string!("name"),
            JsValue::from(js_string!(name.as_str())),
            boa_engine::property::Attribute::READONLY,
        )
        .property(
            js_string!("version"),
            JsValue::from(version as i32),
            boa_engine::property::Attribute::READONLY,
        )
        .function(close_fn, js_string!("close"), 0)
        .function(create_store_fn, js_string!("createObjectStore"), 2)
        .function(transaction_fn, js_string!("transaction"), 2)
        .build();
    Ok(JsValue::from(obj))
}

fn build_object_store_object(
    context: &mut Context,
    db_name: &str,
    store_name: &str,
) -> JsResult<JsValue> {
    let db_name = db_name.to_owned();
    let store_name = store_name.to_owned();

    let put_fn = {
        let db_name = db_name.clone();
        let store_name = store_name.clone();
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let value = args.first().ok_or_else(|| {
                    to_js_err("value required".to_owned())
                })?;
                let key = args.get(1).map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))
                    .map_err(|e| to_js_err(e.to_string()))?;
                let value_json = value.to_json(ctx)
                    .map_err(|e| to_js_err(format!("serialize error: {e}")))?;
                let value_str = serde_json::to_string(&value_json)
                    .map_err(|e| to_js_err(format!("serialize error: {e}")))?;
                let record_key = if key.is_empty() { uuid_key() } else { key };
                store_result(with_store(|store| {
                    store.put_record(&db_name, &store_name, &record_key, &value_str)
                }))?;
                Ok(JsValue::from(js_string!(record_key.as_str())))
            })
        }
    };

    let get_fn = {
        let db_name = db_name.clone();
        let store_name = store_name.clone();
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let key = args
                    .first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))
                    .map_err(|e| to_js_err(e.to_string()))?;
                let val: Option<String> = store_result(with_store(|store| {
                    store.get_record(&db_name, &store_name, &key)
                }))?;
                match val {
                    Some(val_str) => {
                        let parsed: serde_json::Value = serde_json::from_str(&val_str)
                            .map_err(|e| to_js_err(format!("parse error: {e}")))?;
                        JsValue::from_json(&parsed, ctx)
                    }
                    None => Ok(JsValue::undefined()),
                }
            })
        }
    };

    let delete_fn = {
        let db_name = db_name.clone();
        let store_name = store_name.clone();
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let key = args
                    .first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))
                    .map_err(|e| to_js_err(e.to_string()))?;
                store_result(with_store(|store| {
                    store.delete_record(&db_name, &store_name, &key)
                }))?;
                Ok(JsValue::undefined())
            })
        }
    };

    let clear_fn = {
        let db_name = db_name.clone();
        let store_name = store_name.clone();
        unsafe {
            NativeFunction::from_closure(move |_this, _args, _ctx| {
                store_result(with_store(|store| {
                    store.clear_store(&db_name, &store_name)
                }))?;
                Ok(JsValue::undefined())
            })
        }
    };

    let get_all_fn = {
        let db_name = db_name.clone();
        let store_name = store_name.clone();
        unsafe {
            NativeFunction::from_closure(move |_this, _args, ctx| {
                let records = store_result(with_store(|store| {
                    store.get_all_records(&db_name, &store_name)
                }))?;
                let arr = boa_engine::object::builtins::JsArray::new(ctx);
                for rec in &records {
                    let val: serde_json::Value = serde_json::from_str(&rec.value)
                        .unwrap_or(serde_json::Value::String(rec.value.clone()));
                    if let Ok(js_val) = JsValue::from_json(&val, ctx) {
                        let _ = arr.push(js_val, ctx);
                    }
                }
                Ok(JsValue::from(arr))
            })
        }
    };

    let count_fn = unsafe {
        let db_name = db_name.clone();
        let store_name = store_name.clone();
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let records = store_result(with_store(|store| {
                store.get_all_records(&db_name, &store_name)
            }))?;
            Ok(JsValue::from(records.len() as i32))
        })
    };

    let obj = ObjectInitializer::new(context)
        .property(
            js_string!("name"),
            JsValue::from(js_string!(store_name.as_str())),
            boa_engine::property::Attribute::READONLY,
        )
        .function(put_fn, js_string!("put"), 2)
        .function(get_fn, js_string!("get"), 1)
        .function(delete_fn, js_string!("delete"), 1)
        .function(clear_fn, js_string!("clear"), 0)
        .function(get_all_fn, js_string!("getAll"), 0)
        .function(count_fn, js_string!("count"), 0)
        .build();
    Ok(JsValue::from(obj))
}

fn build_transaction_object(
    context: &mut Context,
    db_name: &str,
    store_names: &[String],
) -> JsResult<JsValue> {
    let db_name = db_name.to_owned();
    let store_names = store_names.to_vec();

    let object_store_fn = {
        let db_name = db_name.clone();
        let store_names = store_names.clone();
        unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let name = args
                    .first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))
                    .map_err(|e| to_js_err(e.to_string()))?;
                if !store_names.contains(&name) {
                    return Err(to_js_err(format!("Object store '{name}' not in transaction")));
                }
                build_object_store_object(ctx, &db_name, &name)
            })
        }
    };

    let obj = ObjectInitializer::new(context)
        .function(object_store_fn, js_string!("objectStore"), 1)
        .build();
    Ok(JsValue::from(obj))
}

fn uuid_key() -> String {
    use std::sync::atomic::AtomicU64;
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let count = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("idb_{:x}_{:x}", nanos, count)
}

pub fn register(context: &mut Context) -> JsResult<()> {
    let open_fn = NativeFunction::from_fn_ptr(open_db);

    let delete_db_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let db_name = args
                .first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))
                .map_err(|e| to_js_err(e.to_string()))?;
            store_result(with_store(|store| store.delete_database(&db_name)))?;
            Ok(JsValue::undefined())
        })
    };

    let databases_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let names = store_result(with_store(|store| store.list_databases()))?;
            let arr = boa_engine::object::builtins::JsArray::new(ctx);
            for name in &names {
                let _ = arr.push(JsValue::from(js_string!(name.as_str())), ctx);
            }
            Ok(JsValue::from(arr))
        })
    };

    let idb_obj = ObjectInitializer::new(context)
        .function(open_fn, js_string!("open"), 2)
        .function(delete_db_fn, js_string!("deleteDatabase"), 1)
        .function(databases_fn, js_string!("databases"), 0)
        .build();

    context.register_global_property(
        js_string!("indexedDB"),
        JsValue::from(idb_obj),
        boa_engine::property::Attribute::all(),
    )?;

    Ok(())
}
