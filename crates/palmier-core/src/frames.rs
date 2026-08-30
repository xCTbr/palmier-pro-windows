//! Integer frame arithmetic. Constitution principle IV: frames are `i64` end to end,
//! ranges are half-open `[start, end)`, and every operation validates before
//! computing rather than panicking or wrapping.

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FrameError {
    #[error("frame arithmetic overflowed i64: {start} + {duration}")]
    Overflow { start: i64, duration: i64 },
    #[error("duration must not be negative: {0}")]
    NegativeDuration(i64),
    #[error("fps must be positive: {0}")]
    NonPositiveFps(i64),
}

/// A half-open frame range `[start, end)`. `duration = end - start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameRange {
    start: i64,
    end: i64,
}

impl FrameRange {
    /// Build from a start and a duration, rejecting negative durations and overflow.
    pub fn from_duration(start: i64, duration: i64) -> Result<Self, FrameError> {
        if duration < 0 {
            return Err(FrameError::NegativeDuration(duration));
        }
        let end = start
            .checked_add(duration)
            .ok_or(FrameError::Overflow { start, duration })?;
        Ok(Self { start, end })
    }

    pub fn start(self) -> i64 {
        self.start
    }

    pub fn end(self) -> i64 {
        self.end
    }

    pub fn duration(self) -> i64 {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.end == self.start
    }

    /// Half-open containment: `start <= frame < end`. An empty range contains nothing.
    pub fn contains(self, frame: i64) -> bool {
        frame >= self.start && frame < self.end
    }

    /// Two half-open ranges overlap when each starts before the other ends.
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// Format a frame count as `HH:MM:SS:FF` at the given rate.
pub fn timecode(frame: i64, fps: i64) -> Result<String, FrameError> {
    if fps <= 0 {
        return Err(FrameError::NonPositiveFps(fps));
    }
    let negative = frame < 0;
    let magnitude = frame.unsigned_abs();
    let fps = fps as u64;
    let total_seconds = magnitude / fps;
    let frames = magnitude % fps;
    let seconds = total_seconds % 60;
    let minutes = (total_seconds / 60) % 60;
    let hours = total_seconds / 3600;
    let sign = if negative { "-" } else { "" };
    Ok(format!(
        "{sign}{hours:02}:{minutes:02}:{seconds:02}:{frames:02}"
    ))
}
