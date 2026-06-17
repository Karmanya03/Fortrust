use boa_engine::{
    js_string, Context, JsError, JsNativeError, JsResult, JsValue,
    NativeFunction, object::ObjectInitializer, object::builtins::JsArrayBuffer,
};

pub fn register(context: &mut Context) -> JsResult<()> {
    let get_random_values = unsafe {
        NativeFunction::from_closure(|_this, args, ctx| {
            let buf = args.first().ok_or_else(|| {
                JsError::from_native(
                    JsNativeError::typ().with_message("getRandomValues: 1 argument required"),
                )
            })?;
            let obj = buf.as_object().ok_or_else(|| {
                JsError::from_native(
                    JsNativeError::typ()
                        .with_message("getRandomValues: argument must be an ArrayBuffer view"),
                )
            })?;

            let byte_length = obj
                .get(js_string!("byteLength"), ctx)
                .ok()
                .and_then(|v| v.as_number())
                .map(|n| n as usize)
                .unwrap_or(0);

            if byte_length > 65536 {
                return Err(JsError::from_native(
                    JsNativeError::typ()
                        .with_message("getRandomValues: byteLength exceeds 65536"),
                ));
            }

            if byte_length == 0 {
                return Ok(buf.clone());
            }

            let buffer_val = obj.get(js_string!("buffer"), ctx).map_err(|_| {
                JsError::from_native(
                    JsNativeError::typ().with_message("getRandomValues: no buffer property"),
                )
            })?;
            let buf_obj = buffer_val.as_object().ok_or_else(|| {
                JsError::from_native(
                    JsNativeError::typ()
                        .with_message("getRandomValues: buffer is not an object"),
                )
            })?;

            let byte_offset = obj
                .get(js_string!("byteOffset"), ctx)
                .ok()
                .and_then(|v| v.as_number())
                .map(|n| n as usize)
                .unwrap_or(0);

            let array_buffer = JsArrayBuffer::from_object(buf_obj.clone()).map_err(|_| {
                JsError::from_native(
                    JsNativeError::typ()
                        .with_message("getRandomValues: failed to get ArrayBuffer"),
                )
            })?;

            if let Some(mut data) = array_buffer.data_mut() {
                let slice = &mut data[byte_offset..byte_offset + byte_length];
                use rand::Rng;
                rand::rngs::OsRng.fill(slice);
            }

            Ok(buf.clone())
        })
    };

    let subtle = build_subtle_object(context)?;

    let crypto = ObjectInitializer::new(context)
        .function(get_random_values, js_string!("getRandomValues"), 1)
        .property(js_string!("subtle"), subtle, boa_engine::property::Attribute::READONLY)
        .build();

    context.register_global_property(
        js_string!("crypto"),
        JsValue::from(crypto),
        boa_engine::property::Attribute::all(),
    )?;

    Ok(())
}

fn build_subtle_object(context: &mut Context) -> JsResult<JsValue> {
    let digest_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let algorithm = args.first().ok_or_else(|| {
                JsError::from_native(
                    JsNativeError::typ().with_message("subtle.digest: 2 arguments required"),
                )
            })?;
            let data = args.get(1).ok_or_else(|| {
                JsError::from_native(
                    JsNativeError::typ().with_message("subtle.digest: 2 arguments required"),
                )
            })?;

            let alg_str = algorithm
                .to_string(ctx)
                .map(|s| s.to_std_string_escaped().to_uppercase())
                .unwrap_or_default();

            let data_bytes = extract_bytes(data, ctx)?;
            let hash = compute_digest(&alg_str, &data_bytes)?;

            let buffer = JsArrayBuffer::from_byte_block(hash, ctx)?;

            let promise = boa_engine::object::builtins::JsPromise::resolve(
                JsValue::from(buffer),
                ctx,
            );
            Ok(JsValue::from(promise))
        })
    };

    let subtle = ObjectInitializer::new(context)
        .function(digest_fn, js_string!("digest"), 2)
        .build();
    Ok(JsValue::from(subtle))
}

fn extract_bytes(value: &JsValue, context: &mut Context) -> JsResult<Vec<u8>> {
    if let Some(obj) = value.as_object() {
        if let Ok(ab) = JsArrayBuffer::from_object(obj.clone()) {
            if let Some(data) = ab.data() {
                return Ok(data.to_vec());
            }
        }

        if let Ok(buffer_val) = obj.get(js_string!("buffer"), context) {
            if let Some(buf_obj) = buffer_val.as_object() {
                if let Ok(ab) = JsArrayBuffer::from_object(buf_obj.clone()) {
                    let byte_offset = obj
                        .get(js_string!("byteOffset"), context)
                        .ok()
                        .and_then(|v| v.as_number())
                        .map(|n| n as usize)
                        .unwrap_or(0);
                    let byte_length = obj
                        .get(js_string!("byteLength"), context)
                        .ok()
                        .and_then(|v| v.as_number())
                        .map(|n| n as usize)
                        .unwrap_or(0);
                    if let Some(data) = ab.data() {
                        if byte_offset + byte_length <= data.len() {
                            return Ok(data[byte_offset..byte_offset + byte_length].to_vec());
                        }
                    }
                }
            }
        }
    }

    let s = value.to_string(context).map(|s| s.to_std_string_escaped())?;
    Ok(s.into_bytes())
}

fn compute_digest(algorithm: &str, data: &[u8]) -> JsResult<Vec<u8>> {
    match algorithm {
        "SHA-256" | "SHA256" => {
            Ok(ring::digest::digest(&ring::digest::SHA256, data).as_ref().to_vec())
        }
        "SHA-384" | "SHA384" => {
            Ok(ring::digest::digest(&ring::digest::SHA384, data).as_ref().to_vec())
        }
        "SHA-512" | "SHA512" => {
            Ok(ring::digest::digest(&ring::digest::SHA512, data).as_ref().to_vec())
        }
        "SHA-1" | "SHA1" => {
            Ok(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, data).as_ref().to_vec())
        }
        other => Err(JsError::from_native(
            JsNativeError::typ()
                .with_message(format!("Unsupported algorithm: {other}")),
        )),
    }
}
