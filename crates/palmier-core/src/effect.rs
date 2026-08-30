//! Effects, including colour grades and curves.

use serde::{Deserialize, Serialize};

use crate::codec::{
    DecodeError, Extra, FromObject, Object, PathStack, take_lenient, take_required,
};

/// A single effect parameter. Every field is optional.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectParam {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<crate::keyframe::KeyframeTrack<f64>>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `type` is required; everything else is lenient.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    /// `None` when the document omitted it; filled by `materialize_ids`.
    pub id: Option<String>,
    pub effect_type: String,
    pub enabled: bool,
    pub params: std::collections::BTreeMap<String, EffectParam>,
    pub extra: Extra,
}

impl FromObject for Effect {
    fn from_object(mut o: Object, p: &mut PathStack) -> Result<Self, DecodeError> {
        let effect_type = take_required(&mut o, "type", "string", p)?;
        Ok(Self {
            id: take_lenient(&mut o, "id", None),
            effect_type,
            enabled: take_lenient(&mut o, "enabled", true),
            params: take_lenient(&mut o, "params", Default::default()),
            extra: o,
        })
    }
}

/// A point on a grade curve. Both coordinates required.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub x: f64,
    pub y: f64,
}

/// All four channels required — synthesized decoding, declaration defaults not applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradeCurve {
    pub master: Vec<CurvePoint>,
    pub red: Vec<CurvePoint>,
    pub green: Vec<CurvePoint>,
    pub blue: Vec<CurvePoint>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// All three curves required, same reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HueCurves {
    pub hue_vs_hue: Vec<CurvePoint>,
    pub hue_vs_sat: Vec<CurvePoint>,
    pub hue_vs_lum: Vec<CurvePoint>,
    #[serde(flatten)]
    pub extra: Extra,
}
