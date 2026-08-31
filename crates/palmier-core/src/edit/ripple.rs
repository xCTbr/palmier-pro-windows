//! Ripple: how clips and markers shift when spans of timeline open or close.
//!
//! Ported from `Editor/RippleEngine.swift`, which is already pure. The subtle rules
//! live here — see specs/002-edit-commands/research.md.

use crate::frames::FrameRange;
use crate::marker::TimelineMarker;
use crate::timeline::Clip;

/// A proposed new start frame for one clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipShift {
    pub clip_id: String,
    pub new_start_frame: i64,
}

/// Sort by start and merge overlapping *or adjacent* ranges. Adjacency merges because
/// the original's condition is `range.start <= last.end`, not `<`.
pub fn merge_ranges(ranges: &[FrameRange]) -> Vec<FrameRange> {
    let mut sorted: Vec<FrameRange> = ranges.to_vec();
    sorted.sort_by_key(|r| r.start());
    let mut merged: Vec<FrameRange> = Vec::with_capacity(sorted.len());
    for range in sorted {
        match merged.last_mut() {
            Some(last) if range.start() <= last.end() => {
                let end = last.end().max(range.end());
                *last = FrameRange::from_duration(last.start(), end - last.start())
                    .expect("merging two valid ranges cannot overflow");
            }
            _ => merged.push(range),
        }
    }
    merged
}

/// Shift clips left to close `removed_ranges`.
///
/// A clip shifts by the total length of every merged range that ends at or before its
/// start. A clip that *overlaps* a removed range does not shift at all — only clips
/// entirely after a range move.
pub fn shifts_for_ranges(clips: &[Clip], removed_ranges: &[FrameRange]) -> Vec<ClipShift> {
    let merged = merge_ranges(removed_ranges);
    if merged.is_empty() {
        return Vec::new();
    }
    let mut ordered: Vec<&Clip> = clips.iter().collect();
    ordered.sort_by_key(|c| c.start_frame);

    let mut shifts = Vec::new();
    for clip in ordered {
        let shift: i64 = merged
            .iter()
            .filter(|r| r.end() <= clip.start_frame)
            .map(|r| r.duration())
            .sum();
        if shift > 0 {
            shifts.push(ClipShift {
                clip_id: clip.id.clone().unwrap_or_default(),
                new_start_frame: clip.start_frame - shift,
            });
        }
    }
    shifts
}

/// Shift the clips that remain after `removed_ids` are taken out of `clips`.
pub fn shifts_for_removed(clips: &[Clip], removed_ids: &[String]) -> Vec<ClipShift> {
    let is_removed = |c: &Clip| {
        c.id.as_ref()
            .is_some_and(|id| removed_ids.iter().any(|r| r == id))
    };
    let removed_ranges: Vec<FrameRange> = clips
        .iter()
        .filter(|c| is_removed(c))
        .filter_map(|c| c.range().ok())
        .collect();
    let remaining: Vec<Clip> = clips.iter().filter(|c| !is_removed(c)).cloned().collect();
    shifts_for_ranges(&remaining, &removed_ranges)
}

/// Push every clip starting at or after `insert_frame` forward by `push_amount`.
pub fn push(
    clips: &[Clip],
    insert_frame: i64,
    push_amount: i64,
    exclude: &[String],
) -> Vec<ClipShift> {
    clips
        .iter()
        .filter(|c| {
            c.start_frame >= insert_frame
                && !c
                    .id
                    .as_ref()
                    .is_some_and(|id| exclude.iter().any(|e| e == id))
        })
        .map(|c| ClipShift {
            clip_id: c.id.clone().unwrap_or_default(),
            new_start_frame: c.start_frame + push_amount,
        })
        .collect()
}

/// Map one frame through a set of closing ranges.
///
/// A range entirely before the frame subtracts its whole length; a range straddling the
/// frame collapses the frame onto that range's start. Floored at zero.
pub fn map_frame(frame: i64, closing: &[FrameRange]) -> i64 {
    let mut mapped = frame;
    for range in closing {
        if range.end() <= frame {
            mapped -= range.duration();
        } else if range.start() < frame {
            mapped -= frame - range.start();
        }
    }
    mapped.max(0)
}

/// Remap markers across closing spans.
///
/// `track_ranges` carries one range list per shifting track. A marker survives if it
/// survives in **at least one** track's mapping, and lands at the **minimum** surviving
/// position. Point markers inside a removed range are dropped, as are range markers
/// that would collapse to zero length.
pub fn ripple_markers(
    markers: &[TimelineMarker],
    track_ranges: &[Vec<FrameRange>],
) -> Vec<TimelineMarker> {
    let merged_tracks: Vec<Vec<FrameRange>> = track_ranges
        .iter()
        .map(|ranges| {
            let non_empty: Vec<FrameRange> =
                ranges.iter().copied().filter(|r| !r.is_empty()).collect();
            merge_ranges(&non_empty)
        })
        .filter(|m| !m.is_empty())
        .collect();
    if merged_tracks.is_empty() {
        return markers.to_vec();
    }

    let mut out = Vec::with_capacity(markers.len());
    for marker in markers {
        let is_range = marker.is_range();
        let marker_end = marker.start_frame + marker.duration_frames;

        let mut surviving: Vec<(i64, i64)> = Vec::new();
        for ranges in &merged_tracks {
            let start = map_frame(marker.start_frame, ranges);
            if !is_range {
                let removed = ranges.iter().any(|r| r.contains(marker.start_frame));
                if !removed {
                    surviving.push((start, start));
                }
                continue;
            }
            let end = map_frame(marker_end, ranges);
            if end > start {
                surviving.push((start, end));
            }
        }

        let Some(_) = surviving.first() else { continue };
        let new_start = surviving.iter().map(|s| s.0).min().unwrap_or_default();
        let mut next = marker.clone();
        next.start_frame = new_start;
        if is_range {
            let new_end = surviving.iter().map(|s| s.1).min().unwrap_or_default();
            if new_end <= new_start {
                continue;
            }
            next.duration_frames = new_end - new_start;
        }
        out.push(next);
    }
    out
}

/// Open a gap at `insert_frame`. A negative `push_amount` closes the span behind it.
pub fn ripple_markers_opening(
    markers: &[TimelineMarker],
    insert_frame: i64,
    push_amount: i64,
) -> Vec<TimelineMarker> {
    if push_amount == 0 {
        return markers.to_vec();
    }
    if push_amount < 0 {
        let Ok(range) = FrameRange::from_duration(insert_frame + push_amount, -push_amount) else {
            return markers.to_vec();
        };
        return ripple_markers(markers, &[vec![range]]);
    }
    markers
        .iter()
        .map(|marker| {
            let mut next = marker.clone();
            if next.start_frame >= insert_frame {
                next.start_frame += push_amount;
            } else if next.is_range() && next.start_frame + next.duration_frames > insert_frame {
                next.duration_frames += push_amount;
            }
            next
        })
        .collect()
}
