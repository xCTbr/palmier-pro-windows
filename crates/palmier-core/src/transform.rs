//! Geometry. `Transform` is the only type in the format with a custom encoder and the
//! only one carrying legacy keys.

use serde::{Deserialize, Serialize};

use crate::codec::{DecodeError, Extra, FromObject, Object, PathStack, take_or_default};
use crate::codec::{ObjectWriter, ToObject};

/// Strict on wrong types: the original uses `decodeIfPresent`, not `try?`.
#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    pub center_x: f64,
    pub center_y: f64,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub extra: Extra,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            center_x: 0.5,
            center_y: 0.5,
            width: 1.0,
            height: 1.0,
            rotation: 0.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            flip_horizontal: false,
            flip_vertical: false,
            extra: Extra::new(),
        }
    }
}

impl FromObject for Transform {
    fn from_object(mut o: Object, p: &mut PathStack) -> Result<Self, DecodeError> {
        let width = take_or_default(&mut o, "width", "number", 1.0, p)?;
        let height = take_or_default(&mut o, "height", "number", 1.0, p)?;

        // Legacy files stored a top-left origin under `x`/`y`. Reproduce the original's
        // migration formula exactly; it is the format's history, not a bug to fix.
        let legacy_x: Option<f64> = take_or_default(&mut o, "x", "number", None, p)?;
        let legacy_y: Option<f64> = take_or_default(&mut o, "y", "number", None, p)?;
        let center_x = match take_or_default::<Option<f64>>(&mut o, "centerX", "number", None, p)? {
            Some(v) => v,
            None => legacy_x.map_or(0.5, |x| x + width - 0.5),
        };
        let center_y = match take_or_default::<Option<f64>>(&mut o, "centerY", "number", None, p)? {
            Some(v) => v,
            None => legacy_y.map_or(0.5, |y| y + height - 0.5),
        };

        Ok(Self {
            center_x,
            center_y,
            width,
            height,
            rotation: take_or_default(&mut o, "rotation", "number", 0.0, p)?,
            rotation_x: take_or_default(&mut o, "rotationX", "number", 0.0, p)?,
            rotation_y: take_or_default(&mut o, "rotationY", "number", 0.0, p)?,
            flip_horizontal: take_or_default(&mut o, "flipHorizontal", "boolean", false, p)?,
            flip_vertical: take_or_default(&mut o, "flipVertical", "boolean", false, p)?,
            extra: o,
        })
    }
}

/// All four edges are required: `Crop` uses synthesized decoding, so its declaration
/// defaults are not applied to missing keys (research.md T006).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Crop {
    fn default() -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            extra: Extra::new(),
        }
    }
}

impl Crop {
    pub fn is_identity(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }
}

impl ToObject for Transform {
    /// All nine modern keys, never the legacy `x`/`y` — the original's only custom
    /// encoder behaves exactly this way.
    fn to_object(&self) -> Object {
        let mut w = ObjectWriter::new();
        w.put("centerX", &self.center_x)
            .put("centerY", &self.center_y)
            .put("width", &self.width)
            .put("height", &self.height)
            .put("rotation", &self.rotation)
            .put("rotationX", &self.rotation_x)
            .put("rotationY", &self.rotation_y)
            .put("flipHorizontal", &self.flip_horizontal)
            .put("flipVertical", &self.flip_vertical)
            .extras(&self.extra);
        let mut object = w.finish();
        // Migration is one-way: a re-encoded document never carries the legacy pair,
        // even if the source document did.
        object.remove("x");
        object.remove("y");
        object
    }
}
