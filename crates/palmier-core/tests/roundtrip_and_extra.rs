//! Unknown-field preservation (FR-003) and id materialization (plan.md Q4).
//!
//! Full round-trip equality is User Story 2 and is not implemented yet; these cover
//! the reading half that US1 is responsible for.

use palmier_core::codec::FromObject;
use palmier_core::{ProjectFile, ids};

#[test]
fn unknown_fields_survive_at_every_depth() {
    let bytes = include_bytes!("fixtures/unknown-fields.json");
    let p = ProjectFile::decode(bytes).unwrap();

    assert_eq!(p.extra.get("futureRootKey").unwrap(), "also keep me");

    let timeline = &p.timelines[0];
    assert!(
        timeline.extra.contains_key("futureTimelineKey"),
        "timeline level"
    );

    let track = &timeline.tracks[0];
    assert_eq!(track.extra.get("futureTrackKey").unwrap(), "keep me");

    let clip = &track.clips[0];
    assert_eq!(clip.extra.get("futureClipKey").unwrap(), 42);
    assert_eq!(
        clip.transform.extra.get("futureTransformKey").unwrap(),
        true
    );
}

#[test]
fn known_fields_are_not_captured_as_extra() {
    let bytes = include_bytes!("fixtures/full.json");
    let p = ProjectFile::decode(bytes).unwrap();
    let clip = &p.timelines[0].tracks[0].clips[0];
    assert!(clip.extra.is_empty(), "leftover: {:?}", clip.extra);
    assert!(
        clip.transform.extra.is_empty(),
        "leftover: {:?}",
        clip.transform.extra
    );
}

/// Decoding must stay a pure function, or the round-trip comparison in SC-002 is
/// nondeterministic. This is the whole reason id generation was split out.
#[test]
fn decoding_is_deterministic_and_leaves_absent_ids_none() {
    let json = br#"{"timelines":[{"fps":30,"width":1920,"height":1080,
      "tracks":[{"type":"video","clips":[
        {"mediaRef":"a","startFrame":0,"durationFrames":30}]}]}]}"#;
    let a = ProjectFile::decode(json).unwrap();
    let b = ProjectFile::decode(json).unwrap();
    assert_eq!(a, b, "two decodes of the same bytes must be equal");
    assert_eq!(a.timelines[0].id, None);
    assert_eq!(a.timelines[0].tracks[0].clips[0].id, None);
}

#[test]
fn materialization_fills_every_absent_id() {
    let json = br#"{"timelines":[{"fps":30,"width":1920,"height":1080,
      "tracks":[{"type":"video","clips":[
        {"mediaRef":"a","startFrame":0,"durationFrames":30,
         "effects":[{"type":"vignette"}]}]}]}]}"#;
    let mut p = ProjectFile::decode(json).unwrap();
    ids::materialize_ids(&mut p);
    let t = &p.timelines[0];
    assert!(t.id.is_some());
    assert!(t.tracks[0].id.is_some());
    let clip = &t.tracks[0].clips[0];
    assert!(clip.id.is_some());
    assert!(clip.effects.as_ref().unwrap()[0].id.is_some());
}

#[test]
fn materialization_preserves_existing_ids() {
    let bytes = include_bytes!("fixtures/full.json");
    let mut p = ProjectFile::decode(bytes).unwrap();
    ids::materialize_ids(&mut p);
    assert_eq!(p.timelines[0].id.as_deref(), Some("tl-main"));
    assert_eq!(
        p.timelines[0].tracks[0].clips[0].id.as_deref(),
        Some("cl-1")
    );
}

#[test]
fn full_fixture_decodes_with_expected_values() {
    let bytes = include_bytes!("fixtures/full.json");
    let p = ProjectFile::decode(bytes).unwrap();
    let t = &p.timelines[0];
    assert_eq!(t.fps, 30);
    assert_eq!(t.tracks.len(), 3);
    assert_eq!(t.markers.len(), 2);
    assert_eq!(t.total_frames().unwrap(), 150);
    assert!(t.has_audio_clips());

    let clip = &t.tracks[0].clips[0];
    assert_eq!(clip.speed, 1.5);
    assert_eq!(clip.edge_rounding, 0.25, "in range, so kept");
    assert_eq!(
        clip.blend_mode,
        Some(palmier_core::timeline::BlendMode::SoftLight)
    );
    assert_eq!(clip.opacity_track.as_ref().unwrap().keyframes.len(), 2);
    assert_eq!(clip.effects.as_ref().unwrap()[0].effect_type, "vignette");

    let text = &t.tracks[2].clips[0];
    assert_eq!(text.text_content.as_deref(), Some("Olá, mundo 🎬"));
    let style = text.text_style.as_ref().unwrap();
    assert!(
        style.is_bold,
        "inferred from the font name `Helvetica-Bold`"
    );
    assert!(style.background.enabled);
    assert_eq!(style.border.width, 3.0);
}

#[test]
fn legacy_bare_timeline_is_wrapped() {
    let bytes = include_bytes!("fixtures/legacy-bare-timeline.json");
    let p = ProjectFile::decode(bytes).unwrap();
    assert_eq!(p.timelines.len(), 1);
    assert_eq!(p.timelines[0].name, "Legacy");
    assert_eq!(p.active_timeline_id.as_deref(), Some("tl-legacy"));
    assert_eq!(
        p.open_timeline_ids.as_deref(),
        Some(&["tl-legacy".to_string()][..])
    );
}

#[test]
fn view_states_decode_by_timeline_id() {
    let json = br#"{"timelines":[{"id":"t","fps":30,"width":1920,"height":1080,"tracks":[]}],
      "viewStates":{"t":{"playheadFrame":42,"zoomScale":2.5,"scrollOffsetX":10}}}"#;
    let p = ProjectFile::decode(json).unwrap();
    let vs = p.view_states.as_ref().unwrap().get("t").unwrap();
    assert_eq!(vs.playhead_frame, 42);
    assert_eq!(vs.zoom_scale, 2.5);
}

#[test]
fn from_object_is_reachable_for_downstream_crates() {
    // The trait must stay public: layer 1 decodes nested timelines through it.
    let obj = serde_json::json!({"fps":30,"width":1920,"height":1080,"tracks":[]});
    let serde_json::Value::Object(map) = obj else {
        unreachable!()
    };
    let mut path = palmier_core::codec::PathStack::new();
    assert!(palmier_core::Timeline::from_object(map, &mut path).is_ok());
}
