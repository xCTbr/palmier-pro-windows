//! The commands themselves. Each resolves and validates fully before writing anything.

use std::collections::BTreeSet;

use super::link;
use super::overwrite::{OverwriteAction, advance_trim, compute_overwrite};
use super::patch::{InversePatch, Undo, remove_clip_anywhere, sort_all};
use super::ripple;
use super::{
    ClipMove, ClipProperties, Plan, Receipt, RefusalReason, SplitPoint, TrackProperties, TrimEdge,
    resolve_targets,
};
use crate::frames::FrameRange;
use crate::timeline::{Clip, ClipType, Timeline, Track};

fn timeline_id(timeline: &Timeline) -> String {
    timeline.id.clone().unwrap_or_default()
}

fn track_index(timeline: &Timeline, track_id: &str) -> Result<usize, RefusalReason> {
    timeline
        .tracks
        .iter()
        .position(|t| t.id.as_deref() == Some(track_id))
        .ok_or_else(|| RefusalReason::UnknownTrack(track_id.to_string()))
}

fn track_id_of_clip(timeline: &Timeline, clip_id: &str) -> Option<String> {
    timeline
        .tracks
        .iter()
        .find(|t| t.clips.iter().any(|c| c.id.as_deref() == Some(clip_id)))
        .and_then(|t| t.id.clone())
}

fn find_clip<'a>(timeline: &'a Timeline, clip_id: &str) -> Option<&'a Clip> {
    timeline
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .find(|c| c.id.as_deref() == Some(clip_id))
}

fn type_name(t: ClipType) -> &'static str {
    match t {
        ClipType::Video => "video",
        ClipType::Audio => "audio",
        ClipType::Image => "image",
        ClipType::Text => "text",
        ClipType::Lottie => "lottie",
        ClipType::Sequence => "sequence",
        ClipType::Subtitle => "subtitle",
    }
}

/// Audio belongs on audio tracks; everything visual shares the video lanes.
fn compatible(source: ClipType, destination: ClipType) -> bool {
    match (source, destination) {
        (ClipType::Audio, ClipType::Audio) => true,
        (ClipType::Audio, _) | (_, ClipType::Audio) => false,
        _ => true,
    }
}

/// Frame arithmetic that refuses instead of panicking.
///
/// Constitution principle IV: validate before computing. A debug build panics on
/// overflow and a release build wraps silently — both are defects when the operand came
/// from an agent.
fn checked(label: &str, a: i64, b: i64) -> Result<i64, RefusalReason> {
    a.checked_add(b)
        .ok_or_else(|| RefusalReason::FrameOverflow(label.to_string()))
}

fn validate_frame(label: &str, frame: i64, duration: i64) -> Result<(), RefusalReason> {
    if frame < 0 {
        return Err(RefusalReason::NegativeFrame(label.to_string()));
    }
    FrameRange::from_duration(frame, duration)
        .map(|_| ())
        .map_err(|_| RefusalReason::FrameOverflow(label.to_string()))
}

/// Clear `[start, end)` on a track, recording how to put it back.
fn clear_region(
    timeline: &mut Timeline,
    track_idx: usize,
    start: i64,
    end: i64,
    patch: &mut InversePatch,
    receipt: &mut Receipt,
) {
    let track_id = timeline.tracks[track_idx].id.clone().unwrap_or_default();
    let actions = compute_overwrite(&timeline.tracks[track_idx].clips, start, end);
    // Removals are collected and applied in one `retain` below. Removing inside the
    // loop scans and memmoves the whole track per clip, which is quadratic in a struct
    // this size and dominated a 10,000-clip ripple.
    let mut doomed: std::collections::HashSet<String> = std::collections::HashSet::new();

    for action in actions {
        match action {
            OverwriteAction::Remove { clip_id } => {
                doomed.insert(clip_id.clone());
                receipt.removed_clip_ids.push(clip_id);
            }
            OverwriteAction::TrimEnd {
                clip_id,
                new_duration,
            } => {
                if let Some(slot) = track_slot(&mut timeline.tracks[track_idx], &clip_id) {
                    patch.push(Undo::RestoreClipState {
                        clip: Box::new(slot.clone()),
                    });
                    slot.duration_frames = new_duration;
                    receipt.changed_clip_ids.push(clip_id);
                }
            }
            OverwriteAction::TrimStart {
                clip_id,
                new_start_frame,
                new_trim_start,
                new_duration,
            } => {
                if let Some(slot) = track_slot(&mut timeline.tracks[track_idx], &clip_id) {
                    patch.push(Undo::RestoreClipState {
                        clip: Box::new(slot.clone()),
                    });
                    slot.start_frame = new_start_frame;
                    slot.trim_start_frame = new_trim_start;
                    slot.duration_frames = new_duration;
                    receipt.changed_clip_ids.push(clip_id);
                }
            }
            OverwriteAction::Split {
                clip_id,
                left_duration,
                right_start_frame,
                right_trim_start,
                right_duration,
            } => {
                let Some(slot) = track_slot(&mut timeline.tracks[track_idx], &clip_id) else {
                    continue;
                };
                patch.push(Undo::RestoreClipState {
                    clip: Box::new(slot.clone()),
                });
                let mut right = slot.clone();
                slot.duration_frames = left_duration;
                receipt.changed_clip_ids.push(clip_id);

                right.id = Some(uuid::Uuid::new_v4().to_string());
                right.start_frame = right_start_frame;
                right.trim_start_frame = right_trim_start;
                right.duration_frames = right_duration;
                let right_id = right.id.clone().unwrap_or_default();
                patch.push(Undo::DeleteClip {
                    clip_id: right_id.clone(),
                });
                timeline.tracks[track_idx].clips.push(right);
                receipt.created_clip_ids.push(right_id);
            }
        }
    }

    if !doomed.is_empty() {
        let track = &mut timeline.tracks[track_idx];
        for clip in &track.clips {
            if clip.id.as_deref().is_some_and(|id| doomed.contains(id)) {
                patch.push(Undo::RestoreClip {
                    track_id: track_id.clone(),
                    clip: Box::new(clip.clone()),
                });
            }
        }
        track
            .clips
            .retain(|c| !c.id.as_deref().is_some_and(|id| doomed.contains(id)));
    }
}

/// Look a clip up inside one track. O(track) instead of O(project) — the difference
/// between linear and quadratic when applying thousands of shifts.
fn track_slot<'a>(track: &'a mut Track, clip_id: &str) -> Option<&'a mut Clip> {
    track
        .clips
        .iter_mut()
        .find(|c| c.id.as_deref() == Some(clip_id))
}

/// Apply a batch of shifts to one track in a single pass, recording the prior state.
fn apply_shifts(
    track: &mut Track,
    shifts: &[super::ripple::ClipShift],
    patch: &mut InversePatch,
    receipt: &mut Receipt,
) -> Result<(), RefusalReason> {
    if shifts.is_empty() {
        return Ok(());
    }
    let by_id: std::collections::HashMap<&str, i64> = shifts
        .iter()
        .map(|s| (s.clip_id.as_str(), s.new_start_frame))
        .collect();
    for clip in &mut track.clips {
        let Some(id) = clip.id.as_deref() else {
            continue;
        };
        let Some(&new_start) = by_id.get(id) else {
            continue;
        };
        if new_start < 0 {
            return Err(RefusalReason::NegativeFrame(id.to_string()));
        }
        patch.push(Undo::RestoreClipStart {
            clip_id: id.to_string(),
            start_frame: clip.start_frame,
        });
        clip.start_frame = new_start;
        receipt.changed_clip_ids.push(id.to_string());
    }
    Ok(())
}

fn clip_slot<'a>(timeline: &'a mut Timeline, clip_id: &str) -> Option<&'a mut Clip> {
    timeline
        .tracks
        .iter_mut()
        .flat_map(|t| t.clips.iter_mut())
        .find(|c| c.id.as_deref() == Some(clip_id))
}

// ---------------------------------------------------------------- add / insert

pub(crate) fn add_clips(
    timeline: &mut Timeline,
    track_id: &str,
    clips: &[Clip],
    insert_at: Option<i64>,
) -> Result<Plan, RefusalReason> {
    if clips.is_empty() {
        return Err(RefusalReason::EmptyTargets);
    }
    let idx = track_index(timeline, track_id)?;
    let track_type = timeline.tracks[idx].track_type;

    for clip in clips {
        let label = clip.media_ref.clone();
        validate_frame(&label, clip.start_frame, clip.duration_frames)?;
        if clip.duration_frames < 0 {
            return Err(RefusalReason::Invalid(format!(
                "`{label}` has negative duration"
            )));
        }
        if !compatible(clip.media_type, track_type) {
            return Err(RefusalReason::IncompatibleTrackType {
                clip: label,
                from_type: type_name(clip.media_type),
                to_type: type_name(track_type),
            });
        }
    }

    let mut patch = InversePatch::new(timeline_id(timeline));
    let mut receipt = Receipt::default();

    if let Some(at_frame) = insert_at {
        let total: i64 = clips.iter().map(|c| c.duration_frames).sum();
        let shifts = ripple::push(&timeline.tracks[idx].clips, at_frame, total, &[]);
        apply_shifts(&mut timeline.tracks[idx], &shifts, &mut patch, &mut receipt)?;
        if total > 0 && !timeline.markers.is_empty() {
            patch.push(Undo::RestoreMarkers {
                markers: timeline.markers.clone(),
            });
            timeline.markers = ripple::ripple_markers_opening(&timeline.markers, at_frame, total);
            receipt.markers_changed = true;
        }
    }

    for clip in clips {
        let mut clip = clip.clone();
        if let Some(at_frame) = insert_at {
            clip.start_frame = at_frame;
        }
        if clip.id.is_none() {
            clip.id = Some(uuid::Uuid::new_v4().to_string());
        }
        let end = checked(&clip.media_ref, clip.start_frame, clip.duration_frames)?;
        if insert_at.is_none() {
            clear_region(
                timeline,
                idx,
                clip.start_frame,
                end,
                &mut patch,
                &mut receipt,
            );
        }
        let id = clip.id.clone().unwrap_or_default();
        patch.push(Undo::DeleteClip {
            clip_id: id.clone(),
        });
        timeline.tracks[idx].clips.push(clip);
        receipt.created_clip_ids.push(id);
    }

    sort_all(timeline);
    Ok(Plan { receipt, patch })
}

// ---------------------------------------------------------------------- move

pub(crate) fn move_clips(
    timeline: &mut Timeline,
    moves: &[ClipMove],
) -> Result<Plan, RefusalReason> {
    if moves.is_empty() {
        return Err(RefusalReason::EmptyTargets);
    }

    // D1: an unknown target refuses the whole command rather than being skipped.
    let requested: Vec<String> = moves.iter().map(|m| m.clip_id.clone()).collect();
    resolve_targets(timeline, &requested, false)?;

    struct Resolved {
        clip: Clip,
        from_track: String,
        to_track: String,
        to_frame: i64,
    }
    let mut resolved = Vec::with_capacity(moves.len());

    for mv in moves {
        let clip = find_clip(timeline, &mv.clip_id)
            .ok_or_else(|| RefusalReason::UnknownClip(mv.clip_id.clone()))?
            .clone();
        let to_idx = track_index(timeline, &mv.to_track_id)?;
        let destination = timeline.tracks[to_idx].track_type;
        if !compatible(clip.media_type, destination) {
            return Err(RefusalReason::IncompatibleTrackType {
                clip: mv.clip_id.clone(),
                from_type: type_name(clip.media_type),
                to_type: type_name(destination),
            });
        }
        // D2: refuse rather than clamping to frame 0.
        validate_frame(&mv.clip_id, mv.to_frame, clip.duration_frames)?;
        let from_track = track_id_of_clip(timeline, &mv.clip_id).unwrap_or_default();
        resolved.push(Resolved {
            clip,
            from_track,
            to_track: mv.to_track_id.clone(),
            to_frame: mv.to_frame,
        });
    }

    // Linked partners follow by the same delta, and are validated too — a partner that
    // would land before frame 0 refuses the command instead of silently desyncing.
    let explicit: BTreeSet<String> = requested.iter().cloned().collect();
    let mut partner_moves: Vec<Resolved> = Vec::new();
    for item in &resolved {
        let Some(id) = item.clip.id.as_deref() else {
            continue;
        };
        let delta = checked(id, item.to_frame, -item.clip.start_frame.max(i64::MIN + 1))?;
        if delta == 0 {
            continue;
        }
        for partner_id in link::partners_of(timeline, id) {
            if explicit.contains(&partner_id) {
                continue;
            }
            let partner = find_clip(timeline, &partner_id)
                .ok_or_else(|| RefusalReason::UnknownClip(partner_id.clone()))?
                .clone();
            let to_frame = checked(&partner_id, partner.start_frame, delta)?;
            validate_frame(&partner_id, to_frame, partner.duration_frames)?;
            let from_track = track_id_of_clip(timeline, &partner_id).unwrap_or_default();
            partner_moves.push(Resolved {
                clip: partner,
                to_track: from_track.clone(),
                from_track,
                to_frame,
            });
        }
    }
    resolved.extend(partner_moves);

    let unchanged = resolved
        .iter()
        .all(|r| r.to_frame == r.clip.start_frame && r.to_track == r.from_track);
    if unchanged {
        return Ok(Plan {
            receipt: Receipt::default(),
            patch: InversePatch::new(timeline_id(timeline)),
        });
    }

    let mut patch = InversePatch::new(timeline_id(timeline));
    let mut receipt = Receipt::default();

    // Lift every mover off its track first, so clearing a destination cannot hit one.
    for item in &resolved {
        patch.push(Undo::RestoreClip {
            track_id: item.from_track.clone(),
            clip: Box::new(item.clip.clone()),
        });
        if let Some(id) = item.clip.id.as_deref() {
            remove_clip_anywhere(timeline, id);
        }
    }

    for item in &resolved {
        let to_idx = track_index(timeline, &item.to_track)?;
        let end = item.to_frame + item.clip.duration_frames;
        clear_region(
            timeline,
            to_idx,
            item.to_frame,
            end,
            &mut patch,
            &mut receipt,
        );

        let mut clip = item.clip.clone();
        clip.start_frame = item.to_frame;
        let id = clip.id.clone().unwrap_or_default();
        timeline.tracks[to_idx].clips.push(clip);
        patch.push(Undo::DeleteClip {
            clip_id: id.clone(),
        });
        receipt.changed_clip_ids.push(id);
    }

    sort_all(timeline);
    Ok(Plan { receipt, patch })
}

// -------------------------------------------------------------------- remove

pub(crate) fn remove_clips(
    timeline: &mut Timeline,
    clip_ids: &[String],
    ripple_after: bool,
) -> Result<Plan, RefusalReason> {
    let targets = resolve_targets(timeline, clip_ids, true)?;
    let mut patch = InversePatch::new(timeline_id(timeline));
    let mut receipt = Receipt::default();

    let mut per_track_ranges: Vec<Vec<FrameRange>> = Vec::new();

    for track_idx in 0..timeline.tracks.len() {
        let track_id = timeline.tracks[track_idx].id.clone().unwrap_or_default();
        let doomed: Vec<Clip> = timeline.tracks[track_idx]
            .clips
            .iter()
            .filter(|c| c.id.as_deref().is_some_and(|id| targets.contains(id)))
            .cloned()
            .collect();
        if doomed.is_empty() {
            continue;
        }
        let ranges: Vec<FrameRange> = doomed.iter().filter_map(|c| c.range().ok()).collect();
        for clip in &doomed {
            patch.push(Undo::RestoreClip {
                track_id: track_id.clone(),
                clip: Box::new(clip.clone()),
            });
            receipt
                .removed_clip_ids
                .push(clip.id.clone().unwrap_or_default());
        }
        timeline.tracks[track_idx]
            .clips
            .retain(|c| !c.id.as_deref().is_some_and(|id| targets.contains(id)));

        if ripple_after {
            let shifts = ripple::shifts_for_ranges(&timeline.tracks[track_idx].clips, &ranges);
            apply_shifts(
                &mut timeline.tracks[track_idx],
                &shifts,
                &mut patch,
                &mut receipt,
            )?;
            per_track_ranges.push(ranges);
        }
    }

    if ripple_after && !per_track_ranges.is_empty() && !timeline.markers.is_empty() {
        patch.push(Undo::RestoreMarkers {
            markers: timeline.markers.clone(),
        });
        timeline.markers = ripple::ripple_markers(&timeline.markers, &per_track_ranges);
        receipt.markers_changed = true;
    }

    sort_all(timeline);
    Ok(Plan { receipt, patch })
}

/// Close `ranges` across every sync-locked track, remapping markers.
pub(crate) fn ripple_delete(
    timeline: &mut Timeline,
    ranges: &[(i64, i64)],
) -> Result<Plan, RefusalReason> {
    if ranges.is_empty() {
        return Err(RefusalReason::EmptyTargets);
    }
    let mut frame_ranges = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        if *start < 0 {
            return Err(RefusalReason::NegativeFrame(format!("[{start}, {end})")));
        }
        if end < start {
            return Err(RefusalReason::Invalid(format!(
                "range [{start}, {end}) ends before it starts"
            )));
        }
        frame_ranges.push(
            FrameRange::from_duration(*start, end - start)
                .map_err(|_| RefusalReason::FrameOverflow(format!("[{start}, {end})")))?,
        );
    }
    let merged = ripple::merge_ranges(&frame_ranges);
    if merged.iter().all(|r| r.is_empty()) {
        return Ok(Plan {
            receipt: Receipt::default(),
            patch: InversePatch::new(timeline_id(timeline)),
        });
    }

    let mut patch = InversePatch::new(timeline_id(timeline));
    let mut receipt = Receipt::default();
    let mut per_track_ranges: Vec<Vec<FrameRange>> = Vec::new();

    for track_idx in 0..timeline.tracks.len() {
        // Only sync-locked tracks participate (FR-008).
        if !timeline.tracks[track_idx].sync_locked {
            continue;
        }
        let track_id = timeline.tracks[track_idx].id.clone().unwrap_or_default();

        for range in &merged {
            clear_region(
                timeline,
                track_idx,
                range.start(),
                range.end(),
                &mut patch,
                &mut receipt,
            );
        }
        let _ = track_id;

        let shifts = ripple::shifts_for_ranges(&timeline.tracks[track_idx].clips, &merged);
        apply_shifts(
            &mut timeline.tracks[track_idx],
            &shifts,
            &mut patch,
            &mut receipt,
        )?;
        per_track_ranges.push(merged.clone());
    }

    if !per_track_ranges.is_empty() && !timeline.markers.is_empty() {
        patch.push(Undo::RestoreMarkers {
            markers: timeline.markers.clone(),
        });
        timeline.markers = ripple::ripple_markers(&timeline.markers, &per_track_ranges);
        receipt.markers_changed = true;
    }

    sort_all(timeline);
    Ok(Plan { receipt, patch })
}

// --------------------------------------------------------------------- split

pub(crate) fn split_clips(
    timeline: &mut Timeline,
    points: &[SplitPoint],
) -> Result<Plan, RefusalReason> {
    if points.is_empty() {
        return Err(RefusalReason::EmptyTargets);
    }
    for point in points {
        track_index(timeline, &point.track_id)?;
        if point.at_frame < 0 {
            return Err(RefusalReason::NegativeFrame(point.track_id.clone()));
        }
    }

    let mut patch = InversePatch::new(timeline_id(timeline));
    let mut receipt = Receipt::default();

    for point in points {
        let idx = track_index(timeline, &point.track_id)?;
        // Strictly inside: a split at either boundary is a no-op, matching the original.
        let Some(target) = timeline.tracks[idx]
            .clips
            .iter()
            .find(|c| {
                c.start_frame < point.at_frame
                    && c.range().map(|r| point.at_frame < r.end()).unwrap_or(false)
            })
            .cloned()
        else {
            receipt.skip(
                point.track_id.clone(),
                format!("no clip spans frame {}", point.at_frame),
            );
            continue;
        };
        let Some(target_id) = target.id.clone() else {
            continue;
        };

        let group: Vec<String> = if target.link_group_id.is_some() {
            let mut all = vec![target_id.clone()];
            all.extend(link::partners_of(timeline, &target_id));
            all
        } else {
            vec![target_id.clone()]
        };

        let mut right_ids = Vec::new();
        for member_id in &group {
            let Some(slot) = clip_slot(timeline, member_id) else {
                continue;
            };
            if slot.start_frame >= point.at_frame {
                continue;
            }
            let Ok(range) = slot.range() else { continue };
            if point.at_frame >= range.end() {
                continue;
            }
            let left_duration = point.at_frame - slot.start_frame;
            let right_duration = range.end() - point.at_frame;

            patch.push(Undo::RestoreClipState {
                clip: Box::new(slot.clone()),
            });
            let mut right = slot.clone();
            slot.duration_frames = left_duration;
            receipt.changed_clip_ids.push(member_id.clone());

            right.id = Some(uuid::Uuid::new_v4().to_string());
            right.start_frame = point.at_frame;
            right.trim_start_frame =
                advance_trim(right.trim_start_frame, left_duration, right.speed);
            right.duration_frames = right_duration;
            let right_id = right.id.clone().unwrap_or_default();

            let track_id = track_id_of_clip(timeline, member_id).unwrap_or_default();
            let track_idx = track_index(timeline, &track_id)?;
            patch.push(Undo::DeleteClip {
                clip_id: right_id.clone(),
            });
            timeline.tracks[track_idx].clips.push(right);
            receipt.created_clip_ids.push(right_id.clone());
            right_ids.push(right_id);
        }

        // The right halves become their own link group, as in the original.
        if group.len() > 1 && right_ids.len() > 1 {
            let new_group = uuid::Uuid::new_v4().to_string();
            for id in &right_ids {
                if let Some(slot) = clip_slot(timeline, id) {
                    slot.link_group_id = Some(new_group.clone());
                }
            }
        }
    }

    sort_all(timeline);
    Ok(Plan { receipt, patch })
}

// ---------------------------------------------------------------------- trim

pub(crate) fn trim_clip(
    timeline: &mut Timeline,
    clip_id: &str,
    edge: TrimEdge,
    delta_frames: i64,
) -> Result<Plan, RefusalReason> {
    let clip = find_clip(timeline, clip_id)
        .ok_or_else(|| RefusalReason::UnknownClip(clip_id.to_string()))?
        .clone();
    if delta_frames == 0 {
        return Ok(Plan {
            receipt: Receipt::default(),
            patch: InversePatch::new(timeline_id(timeline)),
        });
    }

    let (new_start, new_duration, new_trim_start) = match edge {
        TrimEdge::Left => (
            checked(clip_id, clip.start_frame, delta_frames)?,
            checked(
                clip_id,
                clip.duration_frames,
                -delta_frames.max(i64::MIN + 1),
            )?,
            advance_trim(clip.trim_start_frame, delta_frames, clip.speed),
        ),
        TrimEdge::Right => (
            clip.start_frame,
            checked(clip_id, clip.duration_frames, delta_frames)?,
            clip.trim_start_frame,
        ),
    };
    if new_duration <= 0 {
        return Err(RefusalReason::Invalid(format!(
            "trimming `{clip_id}` would leave a duration of {new_duration}"
        )));
    }
    if new_trim_start < 0 {
        return Err(RefusalReason::Invalid(format!(
            "trimming `{clip_id}` past the start of its media"
        )));
    }
    validate_frame(clip_id, new_start, new_duration)?;

    let mut patch = InversePatch::new(timeline_id(timeline));
    let mut receipt = Receipt::default();
    if let Some(slot) = clip_slot(timeline, clip_id) {
        patch.push(Undo::RestoreClipState {
            clip: Box::new(slot.clone()),
        });
        slot.start_frame = new_start;
        slot.duration_frames = new_duration;
        slot.trim_start_frame = new_trim_start;
        receipt.changed_clip_ids.push(clip_id.to_string());
    }
    sort_all(timeline);
    Ok(Plan { receipt, patch })
}

// ---------------------------------------------------------------- properties

pub(crate) fn set_clip_properties(
    timeline: &mut Timeline,
    clip_ids: &[String],
    properties: &ClipProperties,
) -> Result<Plan, RefusalReason> {
    let targets = resolve_targets(timeline, clip_ids, false)?;

    if let Some(speed) = properties.speed {
        if !speed.is_finite() || speed <= 0.0 {
            return Err(RefusalReason::Invalid(format!(
                "speed must be finite and positive, got {speed}"
            )));
        }
        for id in &targets {
            if let Some(clip) = find_clip(timeline, id)
                && clip.source_clip_type == ClipType::Sequence
            {
                return Err(RefusalReason::SequenceNotRetimable(id.clone()));
            }
        }
    }
    for value in [properties.opacity, properties.volume]
        .into_iter()
        .flatten()
    {
        if !value.is_finite() {
            return Err(RefusalReason::Invalid(
                "opacity and volume must be finite".into(),
            ));
        }
    }

    let mut patch = InversePatch::new(timeline_id(timeline));
    let mut receipt = Receipt::default();

    for id in &targets {
        let Some(slot) = clip_slot(timeline, id) else {
            continue;
        };
        let before = slot.clone();
        if let Some(v) = properties.opacity {
            slot.opacity = v;
        }
        if let Some(v) = properties.volume {
            slot.volume = v;
        }
        if let Some(v) = properties.speed {
            slot.speed = v;
        }
        if let Some(v) = properties.fade_in_frames {
            slot.fade_in_frames = v;
        }
        if let Some(v) = properties.fade_out_frames {
            slot.fade_out_frames = v;
        }
        if *slot != before {
            patch.push(Undo::RestoreClipState {
                clip: Box::new(before),
            });
            receipt.changed_clip_ids.push(id.clone());
        }
    }
    Ok(Plan { receipt, patch })
}

// -------------------------------------------------------------------- tracks

pub(crate) fn add_track(
    timeline: &mut Timeline,
    track_type: ClipType,
    at_index: Option<usize>,
) -> Result<Plan, RefusalReason> {
    let id = uuid::Uuid::new_v4().to_string();
    let track = Track {
        id: Some(id.clone()),
        track_type,
        name: None,
        muted: false,
        hidden: false,
        sync_locked: true,
        clips: Vec::new(),
        display_height: crate::timeline::track_size::DEFAULT_HEIGHT,
        extra: Default::default(),
    };
    let index = at_index
        .unwrap_or(timeline.tracks.len())
        .min(timeline.tracks.len());

    let mut patch = InversePatch::new(timeline_id(timeline));
    patch.push(Undo::DeleteTrack {
        track_id: id.clone(),
    });
    timeline.tracks.insert(index, track);

    Ok(Plan {
        receipt: Receipt {
            created_track_ids: vec![id],
            ..Default::default()
        },
        patch,
    })
}

pub(crate) fn remove_track(timeline: &mut Timeline, track_id: &str) -> Result<Plan, RefusalReason> {
    let index = track_index(timeline, track_id)?;
    if !timeline.tracks[index].clips.is_empty() {
        return Err(RefusalReason::TrackNotEmpty(track_id.to_string()));
    }
    let mut patch = InversePatch::new(timeline_id(timeline));
    let track = timeline.tracks.remove(index);
    patch.push(Undo::RestoreTrack {
        index,
        track: Box::new(track),
    });

    Ok(Plan {
        receipt: Receipt {
            removed_track_ids: vec![track_id.to_string()],
            ..Default::default()
        },
        patch,
    })
}

pub(crate) fn set_track_properties(
    timeline: &mut Timeline,
    track_id: &str,
    properties: &TrackProperties,
) -> Result<Plan, RefusalReason> {
    let index = track_index(timeline, track_id)?;
    let before = timeline.tracks[index].clone();
    let id = timeline_id(timeline);
    {
        let track = &mut timeline.tracks[index];
        if let Some(name) = &properties.name {
            track.name = name.clone();
        }
        if let Some(v) = properties.muted {
            track.muted = v;
        }
        if let Some(v) = properties.hidden {
            track.hidden = v;
        }
        if let Some(v) = properties.sync_locked {
            track.sync_locked = v;
        }
    }

    let mut patch = InversePatch::new(id);
    let mut receipt = Receipt::default();
    if timeline.tracks[index] != before {
        patch.push(Undo::RestoreTrackState {
            track_id: track_id.to_string(),
            track: Box::new(before),
        });
        receipt.changed_track_ids.push(track_id.to_string());
    }
    Ok(Plan { receipt, patch })
}

// ---------------------------------------------------------------------- link

pub(crate) fn link_clips(
    timeline: &mut Timeline,
    clip_ids: &[String],
    link: bool,
) -> Result<Plan, RefusalReason> {
    let targets = resolve_targets(timeline, clip_ids, !link)?;
    if link && targets.len() < 2 {
        return Err(RefusalReason::Invalid(
            "linking needs at least two clips".into(),
        ));
    }

    let group = link.then(|| uuid::Uuid::new_v4().to_string());
    let mut patch = InversePatch::new(timeline_id(timeline));
    let mut receipt = Receipt::default();

    for id in &targets {
        let Some(slot) = clip_slot(timeline, id) else {
            continue;
        };
        if slot.link_group_id == group {
            continue;
        }
        patch.push(Undo::RestoreClipState {
            clip: Box::new(slot.clone()),
        });
        slot.link_group_id = group.clone();
        receipt.changed_clip_ids.push(id.clone());
    }
    Ok(Plan { receipt, patch })
}
