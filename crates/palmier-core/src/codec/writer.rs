//! Encoding, mirroring the decoding kernel.
//!
//! Absent optionals are omitted rather than written as null, and captured unknown
//! keys are re-emitted. Key order within a JSON object is not semantic, so extras land
//! after the keys this crate models rather than at their original offset.

use serde::Serialize;
use serde_json::Value;

use super::{Extra, Object};

/// Builds a JSON object field by field, the encode-side counterpart of the extraction
/// helpers.
#[derive(Default)]
pub struct ObjectWriter {
    map: Object,
}

impl ObjectWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Always written — the original's synthesized encoder emits every non-optional key.
    pub fn put<T: Serialize>(&mut self, key: &str, value: &T) -> &mut Self {
        self.map.insert(
            key.to_string(),
            serde_json::to_value(value).unwrap_or(Value::Null),
        );
        self
    }

    /// Written only when present, reproducing `encodeIfPresent` (FR-005).
    pub fn put_opt<T: Serialize>(&mut self, key: &str, value: &Option<T>) -> &mut Self {
        if let Some(value) = value {
            self.put(key, value);
        }
        self
    }

    /// Written only when present, using the type's own object encoder.
    pub fn put_object_opt<T: ToObject>(&mut self, key: &str, value: &Option<T>) -> &mut Self {
        if let Some(value) = value {
            self.map
                .insert(key.to_string(), Value::Object(value.to_object()));
        }
        self
    }

    pub fn put_object<T: ToObject>(&mut self, key: &str, value: &T) -> &mut Self {
        self.map
            .insert(key.to_string(), Value::Object(value.to_object()));
        self
    }

    pub fn put_object_array<T: ToObject>(&mut self, key: &str, values: &[T]) -> &mut Self {
        let items: Vec<Value> = values
            .iter()
            .map(|v| Value::Object(v.to_object()))
            .collect();
        self.map.insert(key.to_string(), Value::Array(items));
        self
    }

    pub fn put_object_array_opt<T: ToObject>(
        &mut self,
        key: &str,
        values: &Option<Vec<T>>,
    ) -> &mut Self {
        if let Some(values) = values {
            self.put_object_array(key, values);
        }
        self
    }

    /// Re-emit captured unknown keys (FR-003). Never overwrites a modeled key.
    pub fn extras(&mut self, extra: &Extra) -> &mut Self {
        for (key, value) in extra {
            if !self.map.contains_key(key) {
                self.map.insert(key.clone(), value.clone());
            }
        }
        self
    }

    pub fn finish(&mut self) -> Object {
        std::mem::take(&mut self.map)
    }
}

/// A type encoded to a JSON object. The encode-side counterpart of `FromObject`.
pub trait ToObject {
    fn to_object(&self) -> Object;
}
