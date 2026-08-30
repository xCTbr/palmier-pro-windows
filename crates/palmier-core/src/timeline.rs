//! The timeline: clips on tracks.

use serde::{Deserialize, Serialize};

use crate::codec::{
    DecodeError, Extra, FromObject, Object, PathStack, clamp_range, coerce_unit_interval,
    take_lenient, take_lenient_opt, take_object_array, take_object_array_lenient,
    take_object_array_opt, take_object_lenient, take_object_opt, take_required,
};
use crate::effect::Effect;
use crate::frames::{FrameError, FrameRange};
use crate::keyframe::{AnimPair, Interpolation, KeyframeTrack};
use crate::marker::TimelineMarker;
use crate::text::{TextAnimation, TextFillMode, TextStyle, WordTiming};
use crate::transform::{Crop, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipType {
    #[default]
    Video,
    Audio,
    Image,
    Text,
    Lottie,
    Sequence,
    Subtitle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlendMode {
    Normal,
    Darken,
    Multiply,
    ColorBurn,
    Lighten,
    Screen,
    ColorDodge,
    Overlay,
    SoftLight,
    HardLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

pub mod track_size {
    pub const MIN_HEIGHT: f64 = 28.0;
    pub const MAX_HEIGHT: f64 = 240.0;
    pub const DEFAULT_HEIGHT: f64 = 64.0;
}

/// Track name rules from `TrackName.normalized`: control characters, newlines, and
/// names over 80 characters are rejected — and rejection yields `None`, not an error.
pub const TRACK_NAME_MAX: usize = 80;

fn normalize_track_name(raw: Option<String>) -> Option<String> {
    let raw = raw?;
    if raw.chars().any(|c| c.is_control()) || raw.chars().count() > TRACK_NAME_MAX {
        return None;
    }
    let trimmed = raw.trim_matches(' ');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Only `mediaRef`, `startFrame`, and `durationFrames` are required (research.md Q1).
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// `None` when the document omitted it; filled by `materialize_ids`.
    pub id: Option<String>,
    pub media_ref: String,
    pub media_type: ClipType,
    pub source_clip_type: ClipType,
    pub start_frame: i64,
    pub duration_frames: i64,
    pub trim_start_frame: i64,
    pub trim_end_frame: i64,
    pub speed: f64,
    pub volume: f64,
    pub fade_in_frames: i64,
    pub fade_out_frames: i64,
    pub fade_in_interpolation: Interpolation,
    pub fade_out_interpolation: Interpolation,
    pub opacity: f64,
    pub transform: Transform,
    pub crop: Crop,
    pub edge_rounding: f64,
    pub edge_softness: f64,
    pub link_group_id: Option<String>,
    pub caption_group_id: Option<String>,
    pub multicam_group_id: Option<String>,
    pub text_content: Option<String>,
    pub text_style: Option<TextStyle>,
    pub text_animation: Option<TextAnimation>,
    pub word_timings: Option<Vec<WordTiming>>,
    pub text_fill_mode: Option<TextFillMode>,
    pub opacity_track: Option<KeyframeTrack<f64>>,
    pub position_track: Option<KeyframeTrack<AnimPair>>,
    pub scale_track: Option<KeyframeTrack<AnimPair>>,
    pub rotation_track: Option<KeyframeTrack<f64>>,
    pub crop_track: Option<KeyframeTrack<Crop>>,
    pub volume_track: Option<KeyframeTrack<f64>>,
    pub effects: Option<Vec<Effect>>,
    pub blend_mode: Option<BlendMode>,
    pub extra: Extra,
}

impl FromObject for Clip {
    fn from_object(mut o: Object, p: &mut PathStack) -> Result<Self, DecodeError> {
        let media_ref = take_required(&mut o, "mediaRef", "string", p)?;
        let start_frame = take_required(&mut o, "startFrame", "integer", p)?;
        let duration_frames = take_required(&mut o, "durationFrames", "integer", p)?;
        Ok(Self {
            id: take_lenient(&mut o, "id", None),
            media_ref,
            media_type: take_lenient(&mut o, "mediaType", ClipType::Video),
            source_clip_type: take_lenient(&mut o, "sourceClipType", ClipType::Video),
            start_frame,
            duration_frames,
            trim_start_frame: take_lenient(&mut o, "trimStartFrame", 0),
            trim_end_frame: take_lenient(&mut o, "trimEndFrame", 0),
            speed: take_lenient(&mut o, "speed", 1.0),
            volume: take_lenient(&mut o, "volume", 1.0),
            fade_in_frames: take_lenient(&mut o, "fadeInFrames", 0),
            fade_out_frames: take_lenient(&mut o, "fadeOutFrames", 0),
            fade_in_interpolation: take_lenient(
                &mut o,
                "fadeInInterpolation",
                Interpolation::Linear,
            ),
            fade_out_interpolation: take_lenient(
                &mut o,
                "fadeOutInterpolation",
                Interpolation::Linear,
            ),
            opacity: take_lenient(&mut o, "opacity", 1.0),
            transform: take_object_lenient(&mut o, "transform", Transform::default(), p),
            crop: take_lenient(&mut o, "crop", Crop::default()),
            // Coerced, not clamped — see codec::ranges.
            edge_rounding: coerce_unit_interval(take_lenient(&mut o, "edgeRounding", 0.0)),
            edge_softness: coerce_unit_interval(take_lenient(&mut o, "edgeSoftness", 0.0)),
            link_group_id: take_lenient_opt(&mut o, "linkGroupId"),
            caption_group_id: take_lenient_opt(&mut o, "captionGroupId"),
            multicam_group_id: take_lenient_opt(&mut o, "multicamGroupId"),
            text_content: take_lenient_opt(&mut o, "textContent"),
            text_style: take_object_opt(&mut o, "textStyle", p),
            text_animation: take_object_opt(&mut o, "textAnimation", p),
            word_timings: take_lenient_opt(&mut o, "wordTimings"),
            text_fill_mode: take_lenient_opt(&mut o, "textFillMode"),
            opacity_track: take_lenient_opt(&mut o, "opacityTrack"),
            position_track: take_lenient_opt(&mut o, "positionTrack"),
            scale_track: take_lenient_opt(&mut o, "scaleTrack"),
            rotation_track: take_lenient_opt(&mut o, "rotationTrack"),
            crop_track: take_lenient_opt(&mut o, "cropTrack"),
            volume_track: take_lenient_opt(&mut o, "volumeTrack"),
            effects: take_object_array_opt(&mut o, "effects", p),
            blend_mode: take_lenient_opt(&mut o, "blendMode"),
            extra: o,
        })
    }
}

impl Clip {
    /// Half-open `[startFrame, startFrame + durationFrames)`.
    pub fn range(&self) -> Result<FrameRange, FrameError> {
        FrameRange::from_duration(self.start_frame, self.duration_frames)
    }

    pub fn end_frame(&self) -> Result<i64, FrameError> {
        Ok(self.range()?.end())
    }
}

/// Only `type` is required. Note `syncLocked` defaults to `true`.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: Option<String>,
    pub track_type: ClipType,
    pub name: Option<String>,
    pub muted: bool,
    pub hidden: bool,
    pub sync_locked: bool,
    pub clips: Vec<Clip>,
    pub display_height: f64,
    pub extra: Extra,
}

impl FromObject for Track {
    fn from_object(mut o: Object, p: &mut PathStack) -> Result<Self, DecodeError> {
        let track_type = take_required(&mut o, "type", "string", p)?;
        Ok(Self {
            id: take_lenient(&mut o, "id", None),
            track_type,
            name: normalize_track_name(take_lenient_opt(&mut o, "name")),
            muted: take_lenient(&mut o, "muted", false),
            hidden: take_lenient(&mut o, "hidden", false),
            sync_locked: take_lenient(&mut o, "syncLocked", true),
            clips: take_object_array_lenient(&mut o, "clips", p),
            // Clamped, not coerced — the other half of the pair.
            display_height: clamp_range(
                take_lenient(&mut o, "displayHeight", track_size::DEFAULT_HEIGHT),
                track_size::MIN_HEIGHT,
                track_size::MAX_HEIGHT,
            ),
            extra: o,
        })
    }
}

impl Track {
    pub fn end_frame(&self) -> Result<i64, FrameError> {
        let mut max = 0;
        for clip in &self.clips {
            max = max.max(clip.end_frame()?);
        }
        Ok(max)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineViewState {
    pub playhead_frame: i64,
    pub zoom_scale: f64,
    pub scroll_offset_x: f64,
    pub extra: Extra,
}

impl FromObject for TimelineViewState {
    fn from_object(mut o: Object, p: &mut PathStack) -> Result<Self, DecodeError> {
        Ok(Self {
            playhead_frame: take_required(&mut o, "playheadFrame", "integer", p)?,
            zoom_scale: take_required(&mut o, "zoomScale", "number", p)?,
            scroll_offset_x: take_required(&mut o, "scrollOffsetX", "number", p)?,
            extra: o,
        })
    }
}

/// `fps`, `width`, `height`, and `tracks` are required.
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    pub id: Option<String>,
    pub name: String,
    pub fps: i64,
    pub width: i64,
    pub height: i64,
    pub settings_configured: bool,
    pub folder_id: Option<String>,
    pub tracks: Vec<Track>,
    pub markers: Vec<TimelineMarker>,
    pub extra: Extra,
}

impl FromObject for Timeline {
    fn from_object(mut o: Object, p: &mut PathStack) -> Result<Self, DecodeError> {
        let fps = take_required(&mut o, "fps", "integer", p)?;
        let width = take_required(&mut o, "width", "integer", p)?;
        let height = take_required(&mut o, "height", "integer", p)?;
        let tracks = take_object_array(&mut o, "tracks", p)?;
        Ok(Self {
            id: take_lenient(&mut o, "id", None),
            name: take_lenient(&mut o, "name", "Timeline 1".to_string()),
            fps,
            width,
            height,
            settings_configured: take_lenient(&mut o, "settingsConfigured", false),
            folder_id: take_lenient_opt(&mut o, "folderId"),
            tracks,
            markers: take_object_array_lenient(&mut o, "markers", p),
            extra: o,
        })
    }
}

impl Timeline {
    pub fn total_frames(&self) -> Result<i64, FrameError> {
        let mut max = 0;
        for track in &self.tracks {
            max = max.max(track.end_frame()?);
        }
        Ok(max)
    }

    pub fn has_audio_clips(&self) -> bool {
        self.tracks
            .iter()
            .any(|t| t.track_type == ClipType::Audio && !t.clips.is_empty())
    }
}
