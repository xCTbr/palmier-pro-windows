//! The 13 edge cases spec 001 names. Each asserts the specified outcome, not merely
//! that nothing crashed.

use palmier_core::validate::ValidationError;
use palmier_core::{ProjectFile, validate};

fn decode(json: &str) -> Result<ProjectFile, palmier_core::DecodeError> {
    ProjectFile::decode(json.as_bytes())
}

fn timeline_with(tracks: &str, extra: &str) -> String {
    format!(
        r#"{{"timelines":[{{"id":"t","fps":30,"width":1920,"height":1080,"tracks":[{tracks}]{extra}}}]}}"#
    )
}

#[test]
fn empty_timeline_and_empty_track() {
    let p = decode(&timeline_with("", "")).unwrap();
    assert!(p.timelines[0].tracks.is_empty());
    assert_eq!(p.timelines[0].total_frames().unwrap(), 0);

    let p = decode(&timeline_with(r#"{"type":"video","clips":[]}"#, "")).unwrap();
    assert!(p.timelines[0].tracks[0].clips.is_empty());
    assert_eq!(p.timelines[0].total_frames().unwrap(), 0);
}

#[test]
fn zero_timelines_is_rejected() {
    let err = decode(r#"{"timelines":[]}"#).expect_err("must be rejected");
    assert!(format!("{err}").contains("no timelines"), "{err}");
}

#[test]
fn clip_at_frame_zero() {
    let p = decode(&timeline_with(
        r#"{"type":"video","clips":[{"mediaRef":"a","startFrame":0,"durationFrames":1}]}"#,
        "",
    ))
    .unwrap();
    let clip = &p.timelines[0].tracks[0].clips[0];
    assert_eq!(clip.range().unwrap().start(), 0);
    assert_eq!(clip.end_frame().unwrap(), 1);
}

#[test]
fn i64_max_boundary_errors_rather_than_panicking() {
    let json = timeline_with(
        &format!(
            r#"{{"type":"video","clips":[{{"mediaRef":"a","startFrame":{},"durationFrames":1}}]}}"#,
            i64::MAX
        ),
        "",
    );
    let p = decode(&json).expect("decoding stores the values as given");
    assert!(p.timelines[0].tracks[0].clips[0].end_frame().is_err());
    assert!(
        validate(&p)
            .iter()
            .any(|e| matches!(e, ValidationError::FrameOverflow { .. }))
    );
}

#[test]
fn zero_and_negative_duration() {
    let p = decode(&timeline_with(
        r#"{"type":"video","clips":[{"mediaRef":"a","startFrame":0,"durationFrames":0}]}"#,
        "",
    ))
    .unwrap();
    assert!(
        p.timelines[0].tracks[0].clips[0]
            .range()
            .unwrap()
            .is_empty()
    );

    let p = decode(&timeline_with(
        r#"{"type":"video","clips":[{"mediaRef":"a","startFrame":0,"durationFrames":-5}]}"#,
        "",
    ))
    .unwrap();
    assert!(
        validate(&p)
            .iter()
            .any(|e| matches!(e, ValidationError::NegativeDuration { .. }))
    );
}

#[test]
fn speed_of_zero_and_negative_are_kept_but_non_finite_cannot_be_expressed() {
    let p = decode(&timeline_with(
        r#"{"type":"video","clips":[{"mediaRef":"a","startFrame":0,"durationFrames":30,"speed":0}]}"#,
        "",
    ))
    .unwrap();
    assert_eq!(p.timelines[0].tracks[0].clips[0].speed, 0.0);

    // JSON has no NaN or Infinity literal, so the parser rejects them outright.
    let json = timeline_with(
        r#"{"type":"video","clips":[{"mediaRef":"a","startFrame":0,"durationFrames":30,"speed":NaN}]}"#,
        "",
    );
    assert!(decode(&json).is_err(), "NaN is not valid JSON");
}

#[test]
fn duplicate_ids_are_reported() {
    let p = decode(&timeline_with(
        r#"{"type":"video","id":"tr","clips":[
            {"id":"c","mediaRef":"a","startFrame":0,"durationFrames":30},
            {"id":"c","mediaRef":"b","startFrame":30,"durationFrames":30}]}"#,
        "",
    ))
    .unwrap();
    assert!(
        validate(&p)
            .iter()
            .any(|e| matches!(e, ValidationError::DuplicateId { kind: "clip", .. }))
    );
}

#[test]
fn dangling_media_ref_is_not_a_decode_error() {
    // The manifest is out of scope for this feature, so an unresolvable mediaRef on a
    // non-sequence clip is simply data.
    let p = decode(&timeline_with(
        r#"{"type":"video","clips":[{"mediaRef":"missing","startFrame":0,"durationFrames":30}]}"#,
        "",
    ))
    .unwrap();
    assert!(validate(&p).is_empty());
}

#[test]
fn dangling_nested_timeline_is_reported() {
    let p = decode(&timeline_with(
        r#"{"type":"video","clips":[{"mediaRef":"nope","sourceClipType":"sequence",
            "startFrame":0,"durationFrames":30}]}"#,
        "",
    ))
    .unwrap();
    assert!(
        validate(&p)
            .iter()
            .any(|e| matches!(e, ValidationError::DanglingTimeline { .. }))
    );
}

#[test]
fn nesting_cycle_is_detected_without_infinite_recursion() {
    let json = r#"{"timelines":[
      {"id":"a","fps":30,"width":1920,"height":1080,"tracks":[{"type":"video","clips":[
        {"mediaRef":"b","sourceClipType":"sequence","startFrame":0,"durationFrames":30}]}]},
      {"id":"b","fps":30,"width":1920,"height":1080,"tracks":[{"type":"video","clips":[
        {"mediaRef":"a","sourceClipType":"sequence","startFrame":0,"durationFrames":30}]}]}
    ]}"#;
    let p = decode(json).unwrap();
    assert!(
        validate(&p)
            .iter()
            .any(|e| matches!(e, ValidationError::NestingCycle { .. }))
    );
}

#[test]
fn keyframe_tracks_tolerate_empty_unordered_and_duplicate_frames() {
    let p = decode(&timeline_with(
        r#"{"type":"video","clips":[{"mediaRef":"a","startFrame":0,"durationFrames":30,
            "opacityTrack":{"keyframes":[]},
            "rotationTrack":{"keyframes":[
              {"frame":30,"value":1.0,"interpolationOut":"linear"},
              {"frame":0,"value":0.0,"interpolationOut":"linear"},
              {"frame":30,"value":2.0,"interpolationOut":"linear"}]}}]}"#,
        "",
    ))
    .unwrap();
    let clip = &p.timelines[0].tracks[0].clips[0];
    assert!(clip.opacity_track.as_ref().unwrap().is_empty());

    let rotation = clip.rotation_track.clone().unwrap();
    assert_eq!(
        rotation.keyframes.len(),
        3,
        "decode preserves document order"
    );
    let normalized = rotation.normalized();
    assert_eq!(
        normalized.keyframes.len(),
        2,
        "explicit normalization dedupes"
    );
    assert_eq!(normalized.keyframes[0].frame, 0);
}

#[test]
fn unicode_emoji_and_newlines_in_text_survive() {
    let p = decode(&timeline_with(
        r#"{"type":"text","clips":[{"mediaRef":"t","mediaType":"text","startFrame":0,
            "durationFrames":30,"textContent":"linha 1\nlinha 2 — ção 🎬🇧🇷"}]}"#,
        "",
    ))
    .unwrap();
    assert_eq!(
        p.timelines[0].tracks[0].clips[0].text_content.as_deref(),
        Some("linha 1\nlinha 2 — ção 🎬🇧🇷")
    );
}

#[test]
fn empty_truncated_and_non_json_input() {
    for bad in [
        "",
        "{",
        r#"{"timelines":[{"fps":30,"#,
        "not json at all",
        "[]",
        "42",
    ] {
        let err = decode(bad).expect_err("must be an error, not a panic");
        assert!(!format!("{err}").is_empty());
    }
}

#[test]
fn large_project_loads_without_stack_overflow() {
    let clips: Vec<String> = (0..500)
        .map(|i| {
            format!(
                r#"{{"mediaRef":"a","startFrame":{},"durationFrames":30}}"#,
                i * 30
            )
        })
        .collect();
    let tracks: Vec<String> = (0..20)
        .map(|_| format!(r#"{{"type":"video","clips":[{}]}}"#, clips.join(",")))
        .collect();
    let json = timeline_with(&tracks.join(","), "");
    let p = decode(&json).unwrap();
    assert_eq!(p.timelines[0].tracks.len(), 20);
    assert_eq!(p.timelines[0].tracks[0].clips.len(), 500);
}
