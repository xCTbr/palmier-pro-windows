//! Review notes on a timeline. The strictest type in the format.

use crate::codec::{
    DecodeError, Extra, FromObject, Object, PathStack, take_lenient, take_or_default, take_required,
};
use crate::frames::{FrameError, FrameRange};
use crate::text::Rgba;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarkerStatus {
    #[default]
    Open,
    Resolved,
}

/// Everything is required except `status`. A malformed `color` fails the whole load
/// here, unlike the same type nested inside a lenient parent.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineMarker {
    pub id: String,
    pub name: String,
    pub start_frame: i64,
    pub duration_frames: i64,
    pub color: Rgba,
    pub comment: String,
    pub status: MarkerStatus,
    pub extra: Extra,
}

impl FromObject for TimelineMarker {
    fn from_object(mut o: Object, p: &mut PathStack) -> Result<Self, DecodeError> {
        Ok(Self {
            id: take_required(&mut o, "id", "string", p)?,
            name: take_required(&mut o, "name", "string", p)?,
            start_frame: take_required(&mut o, "startFrame", "integer", p)?,
            duration_frames: take_required(&mut o, "durationFrames", "integer", p)?,
            color: take_required(&mut o, "color", "object", p)?,
            comment: take_required(&mut o, "comment", "string", p)?,
            status: take_or_default(&mut o, "status", "string", MarkerStatus::Open, p)?,
            extra: o,
        })
    }
}

impl TimelineMarker {
    /// A point marker has `durationFrames == 0`; a range marker spans `[start, end)`.
    pub fn range(&self) -> Result<FrameRange, FrameError> {
        FrameRange::from_duration(self.start_frame, self.duration_frames)
    }

    pub fn is_range(&self) -> bool {
        self.duration_frames > 0
    }
}

/// Reserved for the caption speaker registry; nothing in this feature reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerRegistryEntry {
    pub id: Option<String>,
    pub name: Option<String>,
    pub extra: Extra,
}

impl FromObject for SpeakerRegistryEntry {
    fn from_object(mut o: Object, _p: &mut PathStack) -> Result<Self, DecodeError> {
        Ok(Self {
            id: take_lenient(&mut o, "id", None),
            name: take_lenient(&mut o, "name", None),
            extra: o,
        })
    }
}
