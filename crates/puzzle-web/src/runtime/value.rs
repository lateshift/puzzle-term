//! JS arguments become bounded plain data before touching core state (R8).
//!
//! Reflect preserves native objects without linking a JSON parser. Depth and
//! node limits also reject cyclic objects without recursion or stack overflow.
//!
//! JavaScript objects are external input, including objects supplied by a
//! page that normally trusts its own settings. Getters can throw, arrays can
//! contain themselves and numeric values can be NaN or Infinity. Each of
//! these cases must return a reportable error rather than trap in wasm.
//!
//! The node budget is shared by siblings. A shallow object with thousands
//! of fields is therefore bounded just like a deeply nested object. The
//! depth budget stops a self-reference even though the converter does not
//! retain a second table of visited JS identities.
//!
//! No stringification occurs here. Strings remain strings, booleans remain
//! booleans and maps retain their keys for the settings schema's validation.
//! The schema decides unknown keys, numeric clamps and platform defaults;
//! this converter only establishes a finite, plain-data representation.
//!
//! Byte output is deliberately a separate conversion. An ArrayBuffer or
//! an arbitrary numeric array is not silently treated as a Uint8Array.
//! This prevents surprising coercion of application output and keeps the
//! public write contract to its documented two input types.
use js_sys::{Array, Object, Reflect};
use puzzle_term::bus::Value;
use std::collections::BTreeMap;
use wasm_bindgen::{JsCast, JsValue};

/// Errors carry the stable code expected by embedding applications (§12.1).
pub(crate) fn error(code: &str, message: &str) -> JsValue {
    let error = js_sys::Error::new(message);
    let _ = Reflect::set(&error, &"name".into(), &"PuzzleError".into());
    let _ = Reflect::set(&error, &"code".into(), &code.into());
    error.into()
}

/// Property access propagates throwing getters instead of trapping the wasm.
pub(crate) fn get(object: &JsValue, key: &str) -> Result<JsValue, JsValue> {
    Reflect::get(object, &key.into())
}

/// Accepts only strings or actual byte arrays; no implicit object coercion.
pub(crate) fn bytes(data: JsValue) -> Result<Vec<u8>, JsValue> {
    if let Some(text) = data.as_string() {
        return Ok(text.into_bytes());
    }
    if data.is_instance_of::<js_sys::Uint8Array>() {
        return Ok(js_sys::Uint8Array::new(&data).to_vec());
    }
    Err(error(
        "InvalidArgument",
        "write needs a string or Uint8Array",
    ))
}

/// Caps user objects at sixteen levels and 4096 values, including array entries.
pub(crate) fn plain(value: &JsValue, depth: usize, left: &mut usize) -> Result<Value, JsValue> {
    if depth > 16 || *left == 0 {
        return Err(error("InvalidArgument", "options exceed plain-data limits"));
    }
    *left -= 1;
    if value.is_null() || value.is_undefined() {
        return Ok(Value::Null);
    }
    if let Some(value) = value.as_bool() {
        return Ok(Value::Bool(value));
    }
    if let Some(value) = value.as_string() {
        return Ok(Value::String(value));
    }
    if let Some(value) = value.as_f64() {
        return if value.is_finite() {
            Ok(Value::Number(value))
        } else {
            Err(error("InvalidArgument", "number must be finite"))
        };
    }
    if Array::is_array(value) {
        let array = Array::from(value);
        let mut values = Vec::new();
        for value in array.iter() {
            values.push(plain(&value, depth + 1, left)?);
        }
        return Ok(Value::List(values));
    }
    if !value.is_object() {
        return Err(error("InvalidArgument", "options must contain plain data"));
    }
    let object: &Object = value.unchecked_ref();
    let mut values = BTreeMap::new();
    for key in Object::keys(object).iter() {
        let name = key
            .as_string()
            .ok_or_else(|| error("InvalidArgument", "invalid property key"))?;
        values.insert(name.clone(), plain(&get(value, &name)?, depth + 1, left)?);
    }
    Ok(Value::Map(values))
}
