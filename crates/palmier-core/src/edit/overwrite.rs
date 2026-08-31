//! Overwrite: clearing a region of a track so something can be placed there.
//!
//! Ported from `Editor/OverwriteEngine.swift`, already pure.

use crate::timeline::Clip;

/// What must happen to one existing clip for a region to be clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverwriteAction {
    Remove {
        clip_id: String,
    },
    TrimEnd {
        clip_id: String,
        new_duration: i64,
    },
    TrimStart {
        clip_id: String,
        new_start_frame: i64,
        new_trim_start: i64,
        new_duration: i64,
    },
    Split {
        clip_id: String,
        left_duration: i64,
        right_start_frame: i64,
        right_trim_start: i64,
        right_duration: i64,
    },
}

/// Advance a source trim by a timeline span, scaled by clip speed.
///
/// This one line is the format's source-continuity rule (FR-011): trimming `span`
/// frames off the timeline consumes `span * speed` frames of source media.
pub fn advance_trim(trim_start: i64, span: i64, speed: f64) -> i64 {
    let scaled = (span as f64 * speed).round();
    // A non-finite or absurd speed must not produce a poisoned trim.
    if !scaled.is_finite() {
        return trim_start;
    }
    trim_start.saturating_add(scaled as i64)
}

/// Actions needed to clear `[region_start, region_end)` on a track.
pub fn compute_overwrite(
    clips: &[Clip],
    region_start: i64,
    region_end: i64,
) -> Vec<OverwriteAction> {
    if region_end <= region_start {
        return Vec::new();
    }
    let mut actions = Vec::new();

    for clip in clips {
        let Some(id) = clip.id.clone() else { continue };
        let cs = clip.start_frame;
        let Ok(range) = clip.range() else { continue };
        let ce = range.end();

        // Disjoint: untouched.
        if ce <= region_start || cs >= region_end {
            continue;
        }

        if cs >= region_start && ce <= region_end {
            actions.push(OverwriteAction::Remove { clip_id: id });
        } else if cs < region_start && ce > region_end {
            // The region is strictly inside the clip: split around it.
            actions.push(OverwriteAction::Split {
                clip_id: id,
                left_duration: region_start - cs,
                right_start_frame: region_end,
                right_trim_start: advance_trim(clip.trim_start_frame, region_end - cs, clip.speed),
                right_duration: ce - region_end,
            });
        } else if cs < region_start {
            actions.push(OverwriteAction::TrimEnd {
                clip_id: id,
                new_duration: region_start - cs,
            });
        } else {
            actions.push(OverwriteAction::TrimStart {
                clip_id: id,
                new_start_frame: region_end,
                new_trim_start: advance_trim(clip.trim_start_frame, region_end - cs, clip.speed),
                new_duration: ce - region_end,
            });
        }
    }

    actions
}
