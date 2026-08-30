//! Per-property animation.

use serde::{Deserialize, Serialize};

use crate::codec::Extra;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Interpolation {
    #[default]
    Linear,
    Hold,
    Smooth,
}

/// A two-component animated value (position, scale). Both components required.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnimPair {
    pub a: f64,
    pub b: f64,
}

/// `frame`, `value`, and `interpolationOut` are all required — synthesized decoding
/// does not apply the declaration default on `interpolationOut`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Keyframe<T> {
    pub frame: i64,
    pub value: T,
    pub interpolation_out: Interpolation,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyframeTrack<T> {
    pub keyframes: Vec<Keyframe<T>>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl<T> KeyframeTrack<T> {
    /// Sort by frame, keeping the first of any duplicate-frame run.
    ///
    /// Deliberately NOT called during decoding. The original never sorts on load — it
    /// maintains order on insertion and trusts the file — so normalizing here would
    /// reorder keyframes through a round trip and break SC-002. Callers that evaluate
    /// a track opt in.
    pub fn normalized(mut self) -> Self {
        self.keyframes.sort_by_key(|k| k.frame);
        self.keyframes.dedup_by_key(|k| k.frame);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.keyframes.is_empty()
    }
}
