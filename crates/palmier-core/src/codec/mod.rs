//! The decoding kernel. Every model type routes through it.
//!
//! One pattern: collect the JSON object into a map, extract each known key at its
//! audited strictness, and keep whatever remains as `extra`. That single pass
//! satisfies the strictness contract, per-field defaults, and unknown-field
//! preservation together.

pub mod error;
pub mod ranges;
pub mod strictness;
pub mod writer;

pub use error::{DecodeError, ErrorKind, PathStack};
pub use ranges::{clamp_range, coerce_unit_interval};
pub use strictness::{take_lenient, take_lenient_opt, take_or_default, take_required};
pub use writer::{ObjectWriter, ToObject};

use serde_json::Value;

/// A JSON object under decode. `serde_json`'s `preserve_order` feature is enabled, so
/// unmatched keys keep their original position when re-emitted.
pub type Object = serde_json::Map<String, Value>;

/// Unmatched keys, preserved verbatim through a round trip (FR-003).
pub type Extra = Object;

/// A type decoded from a JSON object with unknown-field capture.
pub trait FromObject: Sized {
    fn from_object(object: Object, path: &mut PathStack) -> Result<Self, DecodeError>;
}

/// Decode a `Value` that must be an object.
pub fn object_from_value(value: Value, path: &PathStack) -> Result<Object, DecodeError> {
    match value {
        Value::Object(map) => Ok(map),
        other => Err(DecodeError::wrong_type(path, "object", &other)),
    }
}

/// Decode a required nested object field.
pub fn take_object<T: FromObject>(
    object: &mut Object,
    key: &'static str,
    path: &mut PathStack,
) -> Result<T, DecodeError> {
    let Some(value) = object.remove(key).filter(|v| !v.is_null()) else {
        return Err(DecodeError::missing(path, key));
    };
    path.in_key(key, |p| {
        let map = object_from_value(value, p)?;
        T::from_object(map, p)
    })
}

/// Decode a nested object field leniently: absent or malformed yields the default.
pub fn take_object_lenient<T: FromObject>(
    object: &mut Object,
    key: &'static str,
    default: T,
    path: &mut PathStack,
) -> T {
    match object.remove(key).filter(|v| !v.is_null()) {
        Some(Value::Object(map)) => path
            .in_key(key, |p| T::from_object(map, p))
            .unwrap_or(default),
        _ => default,
    }
}

/// Decode an optional nested object field: absent or malformed yields `None`.
pub fn take_object_opt<T: FromObject>(
    object: &mut Object,
    key: &'static str,
    path: &mut PathStack,
) -> Option<T> {
    match object.remove(key).filter(|v| !v.is_null()) {
        Some(Value::Object(map)) => path.in_key(key, |p| T::from_object(map, p)).ok(),
        _ => None,
    }
}

/// Decode a required array of objects, failing on the first bad element.
pub fn take_object_array<T: FromObject>(
    object: &mut Object,
    key: &'static str,
    path: &mut PathStack,
) -> Result<Vec<T>, DecodeError> {
    let Some(value) = object.remove(key).filter(|v| !v.is_null()) else {
        return Err(DecodeError::missing(path, key));
    };
    path.in_key(key, |p| decode_array(value, p))
}

/// Decode an array of objects leniently: absent or malformed yields an empty vector.
pub fn take_object_array_lenient<T: FromObject>(
    object: &mut Object,
    key: &'static str,
    path: &mut PathStack,
) -> Vec<T> {
    match object.remove(key).filter(|v| !v.is_null()) {
        Some(value) => path
            .in_key(key, |p| decode_array(value, p))
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

/// Decode an optional array of objects: absent or malformed yields `None`.
pub fn take_object_array_opt<T: FromObject>(
    object: &mut Object,
    key: &'static str,
    path: &mut PathStack,
) -> Option<Vec<T>> {
    let value = object.remove(key).filter(|v| !v.is_null())?;
    path.in_key(key, |p| decode_array(value, p)).ok()
}

fn decode_array<T: FromObject>(value: Value, path: &mut PathStack) -> Result<Vec<T>, DecodeError> {
    let Value::Array(items) = value else {
        return Err(DecodeError::wrong_type(path, "array", &value));
    };
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let decoded = path.in_index(index, |p| {
            let map = object_from_value(item, p)?;
            T::from_object(map, p)
        })?;
        out.push(decoded);
    }
    Ok(out)
}
