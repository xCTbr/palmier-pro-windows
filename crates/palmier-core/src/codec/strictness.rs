//! The three strictness levels of the `.palmier` format, named for the Swift
//! construct each one reproduces. See `specs/001-project-model/research.md`.
//!
//! Choosing the wrong helper is the likeliest way this crate diverges from the
//! original, and it is silent on well-formed input.

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::Object;
use super::error::{DecodeError, PathStack};

/// An explicit `null` is indistinguishable from an absent key anywhere in this
/// format (research.md T007), so both are treated as absent.
fn take_present(object: &mut Object, key: &str) -> Option<Value> {
    match object.remove(key) {
        None | Some(Value::Null) => None,
        Some(value) => Some(value),
    }
}

/// Reproduces `try c.decode(T.self, forKey:)`.
/// Missing key: error. Wrong type: error.
pub fn take_required<T: DeserializeOwned>(
    object: &mut Object,
    key: &'static str,
    expected: &'static str,
    path: &mut PathStack,
) -> Result<T, DecodeError> {
    let Some(value) = take_present(object, key) else {
        return Err(DecodeError::missing(path, key));
    };
    path.in_key(key, |p| {
        serde_json::from_value(value.clone())
            .map_err(|_| DecodeError::wrong_type(p, expected, &value))
    })
}

/// Reproduces `try c.decodeIfPresent(T.self, forKey:) ?? default`.
/// Missing key: default. Wrong type: **error**.
pub fn take_or_default<T: DeserializeOwned>(
    object: &mut Object,
    key: &'static str,
    expected: &'static str,
    default: T,
    path: &mut PathStack,
) -> Result<T, DecodeError> {
    let Some(value) = take_present(object, key) else {
        return Ok(default);
    };
    path.in_key(key, |p| {
        serde_json::from_value(value.clone())
            .map_err(|_| DecodeError::wrong_type(p, expected, &value))
    })
}

/// Reproduces `(try? c.decode(T.self, forKey:)) ?? default`.
/// Missing key: default. Wrong type: **default, silently**.
pub fn take_lenient<T: DeserializeOwned>(object: &mut Object, key: &str, default: T) -> T {
    match take_present(object, key) {
        Some(value) => serde_json::from_value(value).unwrap_or(default),
        None => default,
    }
}

/// `take_lenient` for an optional field: absent, null, or malformed all yield `None`.
pub fn take_lenient_opt<T: DeserializeOwned>(object: &mut Object, key: &str) -> Option<T> {
    take_present(object, key).and_then(|value| serde_json::from_value(value).ok())
}
