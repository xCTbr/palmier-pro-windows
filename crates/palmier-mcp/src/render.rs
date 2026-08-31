//! Turning domain values into tool responses.
//!
//! One place, so no tool composes its own shape. Responses carry stable ids, integer
//! frames, and no localized text, UI label, or internal type name (FR-004, FR-007).

use palmier_core::edit::{Receipt, RefusalReason};
use palmier_core::timeline::{Clip, ClipType, Timeline};
use serde_json::{Map, Value, json};

pub fn clip_type(t: ClipType) -> &'static str {
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

/// A clip, with fields equal to their defaults omitted — the original's rule, kept
/// because it is what makes a large timeline readable to an agent.
pub fn clip(c: &Clip) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(c.id.clone().unwrap_or_default()));
    out.insert("mediaRef".into(), json!(c.media_ref));
    out.insert("startFrame".into(), json!(c.start_frame));
    out.insert("endFrame".into(), json!(c.start_frame + c.duration_frames));
    out.insert("durationFrames".into(), json!(c.duration_frames));

    if c.media_type != ClipType::Video {
        out.insert("mediaType".into(), json!(clip_type(c.media_type)));
    }
    if c.source_clip_type != c.media_type {
        out.insert(
            "sourceClipType".into(),
            json!(clip_type(c.source_clip_type)),
        );
    }
    if c.speed != 1.0 {
        out.insert("speed".into(), json!(c.speed));
    }
    if c.volume != 1.0 {
        out.insert("volume".into(), json!(c.volume));
    }
    if c.opacity != 1.0 {
        out.insert("opacity".into(), json!(c.opacity));
    }
    if c.trim_start_frame != 0 {
        out.insert("trimStartFrame".into(), json!(c.trim_start_frame));
    }
    if c.trim_end_frame != 0 {
        out.insert("trimEndFrame".into(), json!(c.trim_end_frame));
    }
    if c.fade_in_frames != 0 {
        out.insert("fadeInFrames".into(), json!(c.fade_in_frames));
    }
    if c.fade_out_frames != 0 {
        out.insert("fadeOutFrames".into(), json!(c.fade_out_frames));
    }
    if let Some(group) = &c.link_group_id {
        out.insert("linkGroupId".into(), json!(group));
    }
    if let Some(text) = &c.text_content {
        out.insert("textContent".into(), json!(text));
    }
    Value::Object(out)
}

/// Empty `[start, end)` spans on a track. Absent when the track is contiguous.
fn gaps(clips: &[Clip]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut cursor = 0i64;
    for c in clips {
        if c.start_frame > cursor {
            out.push(json!({ "startFrame": cursor, "endFrame": c.start_frame }));
        }
        cursor = cursor.max(c.start_frame + c.duration_frames);
    }
    out
}

pub fn timeline(t: &Timeline, window: Option<(i64, i64)>) -> Value {
    let total = t.total_frames().unwrap_or(0);
    let tracks: Vec<Value> = t
        .tracks
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let visible: Vec<&Clip> = track
                .clips
                .iter()
                .filter(|c| match window {
                    None => true,
                    Some((start, end)) => {
                        c.start_frame < end && (c.start_frame + c.duration_frames) > start
                    }
                })
                .collect();

            let mut row = Map::new();
            row.insert(
                "trackId".into(),
                json!(track.id.clone().unwrap_or_default()),
            );
            row.insert("trackIndex".into(), json!(index));
            row.insert("type".into(), json!(clip_type(track.track_type)));
            if let Some(name) = &track.name {
                row.insert("name".into(), json!(name));
            }
            if track.muted {
                row.insert("muted".into(), json!(true));
            }
            if track.hidden {
                row.insert("hidden".into(), json!(true));
            }
            if !track.sync_locked {
                row.insert("syncLocked".into(), json!(false));
            }
            row.insert(
                "clips".into(),
                json!(visible.iter().map(|c| clip(c)).collect::<Vec<_>>()),
            );
            if visible.len() != track.clips.len() {
                row.insert("totalClips".into(), json!(track.clips.len()));
            }
            let g = gaps(&track.clips);
            if !g.is_empty() {
                row.insert("gaps".into(), json!(g));
            }
            Value::Object(row)
        })
        .collect();

    let markers: Vec<Value> = t
        .markers
        .iter()
        .map(|m| {
            json!({
                "markerId": m.id,
                "name": m.name,
                "comment": m.comment,
                "startFrame": m.start_frame,
                "endFrame": m.start_frame + m.duration_frames,
                "durationFrames": m.duration_frames,
            })
        })
        .collect();

    let mut out = json!({
        "timelineId": t.id.clone().unwrap_or_default(),
        "name": t.name,
        "fps": t.fps,
        "width": t.width,
        "height": t.height,
        "totalFrames": total,
        "durationSeconds": if t.fps > 0 { total as f64 / t.fps as f64 } else { 0.0 },
        "tracks": tracks,
    });
    if !markers.is_empty() {
        out["markers"] = json!(markers);
    }
    out
}

/// A receipt, rendered so the agent can tell change from no-change without re-reading.
pub fn receipt(r: &Receipt) -> Value {
    if r.is_no_op() {
        let mut out = json!({ "status": "no_op", "detail": "nothing changed" });
        if !r.skipped.is_empty() {
            out["skipped"] = json!(skipped(r));
        }
        return out;
    }
    let mut out = Map::new();
    out.insert("status".into(), json!("applied"));
    for (key, ids) in [
        ("createdClipIds", &r.created_clip_ids),
        ("removedClipIds", &r.removed_clip_ids),
        ("changedClipIds", &r.changed_clip_ids),
        ("createdTrackIds", &r.created_track_ids),
        ("removedTrackIds", &r.removed_track_ids),
        ("changedTrackIds", &r.changed_track_ids),
    ] {
        if !ids.is_empty() {
            out.insert(key.into(), json!(ids));
        }
    }
    if r.markers_changed {
        out.insert("markersChanged".into(), json!(true));
    }
    if !r.skipped.is_empty() {
        out.insert("skipped".into(), json!(skipped(r)));
    }
    Value::Object(out)
}

fn skipped(r: &Receipt) -> Vec<Value> {
    r.skipped
        .iter()
        .map(|(target, why)| json!({ "target": target, "reason": why }))
        .collect()
}

/// A refusal. Never shaped like a success (constitution principle VI).
pub fn refusal(reason: &RefusalReason) -> Value {
    json!({ "status": "refused", "reason": reason.to_string() })
}
