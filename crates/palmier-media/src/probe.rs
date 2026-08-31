//! Reading what a media file actually is, via `ffprobe`.

use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use crate::{MediaError, require_tool};

/// What a probe can tell us about an asset.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub duration_seconds: f64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub fps: Option<f64>,
    pub has_audio: bool,
    pub has_video: bool,
}

impl MediaInfo {
    /// Duration in whole frames at the timeline's rate, rounded to the nearest frame.
    pub fn duration_frames(&self, fps: i64) -> i64 {
        if fps <= 0 {
            return 0;
        }
        (self.duration_seconds * fps as f64).round() as i64
    }
}

#[derive(Deserialize)]
struct Probe {
    format: Format,
    #[serde(default)]
    streams: Vec<Stream>,
}

#[derive(Deserialize)]
struct Format {
    #[serde(default)]
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: String,
    #[serde(default)]
    width: Option<i64>,
    #[serde(default)]
    height: Option<i64>,
    #[serde(default)]
    r_frame_rate: Option<String>,
}

/// `30000/1001` and friends. Returns `None` rather than a poisoned rate.
fn parse_rate(raw: &str) -> Option<f64> {
    let (num, den) = raw.split_once('/')?;
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    if den == 0.0 || !num.is_finite() || !den.is_finite() {
        return None;
    }
    let rate = num / den;
    rate.is_finite().then_some(rate)
}

pub fn probe(path: &Path) -> Result<MediaInfo, MediaError> {
    require_tool("ffprobe")?;
    if !path.is_file() {
        return Err(MediaError::Io {
            path: path.display().to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not a file"),
        });
    }

    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|_| MediaError::ToolMissing { tool: "ffprobe" })?;

    if !output.status.success() {
        return Err(MediaError::ToolFailed {
            tool: "ffprobe",
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    let parsed: Probe =
        serde_json::from_slice(&output.stdout).map_err(|e| MediaError::ToolFailed {
            tool: "ffprobe",
            message: format!("unreadable output: {e}"),
        })?;

    let duration_seconds = parsed
        .format
        .duration
        .as_deref()
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|d| d.is_finite() && *d >= 0.0)
        .ok_or_else(|| {
            MediaError::Unsupported(format!("{} has no readable duration", path.display()))
        })?;

    let video = parsed.streams.iter().find(|s| s.codec_type == "video");
    Ok(MediaInfo {
        duration_seconds,
        width: video.and_then(|s| s.width),
        height: video.and_then(|s| s.height),
        fps: video
            .and_then(|s| s.r_frame_rate.as_deref())
            .and_then(parse_rate),
        has_audio: parsed.streams.iter().any(|s| s.codec_type == "audio"),
        has_video: video.is_some(),
    })
}
