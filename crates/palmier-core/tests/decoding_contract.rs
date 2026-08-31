//! The three strictness levels, per specs/001-project-model/research.md.
//!
//! The likeliest silent failure in this crate is a field decoded at the wrong
//! strictness: it looks correct on well-formed input and diverges only on malformed
//! input. These tests exist to catch exactly that.

use palmier_core::ProjectFile;
use palmier_core::codec::ErrorKind;

fn project_with_clip(clip_extra: &str) -> String {
    format!(
        r#"{{"timelines":[{{"id":"t","fps":30,"width":1920,"height":1080,
        "tracks":[{{"type":"video","clips":[
          {{"mediaRef":"a","startFrame":0,"durationFrames":30{clip_extra}}}
        ]}}]}}]}}"#
    )
}

fn decode(json: &str) -> Result<ProjectFile, palmier_core::DecodeError> {
    ProjectFile::decode(json.as_bytes())
}

fn only_clip(project: &ProjectFile) -> &palmier_core::Clip {
    &project.timelines[0].tracks[0].clips[0]
}

// ---- lenient: missing key OR wrong type yields the default, silently ----

#[test]
fn clip_speed_is_lenient_on_wrong_type() {
    let p = decode(&project_with_clip(r#","speed":"fast""#)).expect("must not fail");
    assert_eq!(only_clip(&p).speed, 1.0, "`try?` swallows the type error");
}

#[test]
fn clip_speed_is_lenient_on_missing_key() {
    let p = decode(&project_with_clip("")).unwrap();
    assert_eq!(only_clip(&p).speed, 1.0);
}

#[test]
fn clip_media_type_is_lenient_on_unknown_variant() {
    let p = decode(&project_with_clip(r#","mediaType":"hologram""#)).unwrap();
    assert_eq!(only_clip(&p).media_type, palmier_core::ClipType::Video);
}

// ---- strict: missing key yields the default, wrong type is an ERROR ----

/// Strictness is observable as **blast radius**, not as load failure.
///
/// `Transform` uses `decodeIfPresent`, so a wrong type throws inside its decoder — but
/// `Clip` reaches it through `(try? ...) ?? Transform()`, which catches. The whole
/// Transform is therefore replaced by its default, taking the sibling `centerX` down
/// with it. A fully lenient Transform would have kept `centerX` and defaulted only
/// `width`. That difference is what these two levels actually mean in practice.
#[test]
fn strict_field_destroys_its_whole_object_not_just_itself() {
    let p = decode(&project_with_clip(
        r#","transform":{"width":"wide","centerX":0.25}"#,
    ))
    .expect("the lenient parent catches it, so the load succeeds");
    let t = &only_clip(&p).transform;
    assert_eq!(t.width, 1.0, "width defaulted");
    assert_eq!(
        t.center_x, 0.5,
        "and centerX went down with it — the whole object was replaced"
    );
}

#[test]
fn transform_width_defaults_on_missing_key() {
    let p = decode(&project_with_clip(r#","transform":{"centerX":0.25}"#)).unwrap();
    assert_eq!(only_clip(&p).transform.width, 1.0);
}

/// The pair that proves the two levels are actually distinguished. Both loads succeed,
/// but the damage differs: a lenient field defaults alone, a strict one takes its
/// siblings with it. If both behaved the same way, a helper was chosen wrong.
#[test]
fn lenient_and_strict_differ_in_blast_radius() {
    let lenient = decode(&project_with_clip(r#","speed":"fast","opacity":0.25"#)).unwrap();
    assert_eq!(only_clip(&lenient).speed, 1.0, "speed defaulted");
    assert_eq!(
        only_clip(&lenient).opacity,
        0.25,
        "but its sibling survived"
    );

    let strict = decode(&project_with_clip(
        r#","transform":{"width":"wide","centerX":0.25}"#,
    ))
    .unwrap();
    assert_eq!(
        only_clip(&strict).transform.center_x,
        0.5,
        "here the sibling did not survive"
    );
}

// ---- required: missing key is an error ----

/// A clip missing `mediaRef` fails to decode — and because `Track.clips` is
/// `(try? c.decode([Clip].self, ...)) ?? []`, the failure takes **every clip on that
/// track** with it. The load still succeeds, silently, with an empty track.
///
/// This is real data loss in the original format, faithfully reproduced. It is the
/// strongest argument for validating a project after loading it rather than trusting
/// a successful decode.
#[test]
fn one_malformed_clip_empties_its_entire_track() {
    let json = r#"{"timelines":[{"id":"t","fps":30,"width":1920,"height":1080,
      "tracks":[{"type":"video","clips":[
        {"mediaRef":"good","startFrame":0,"durationFrames":30},
        {"startFrame":30,"durationFrames":30}
      ]}]}]}"#;
    let p = decode(json).expect("the lenient array catches it");
    assert!(
        p.timelines[0].tracks[0].clips.is_empty(),
        "the valid clip is lost too — the whole array decode threw"
    );
}

/// The same missing key, reached through a strict path, does fail the load: `tracks`
/// is `try c.decode`, so a malformed track is not survivable.
#[test]
fn malformed_track_fails_the_load() {
    let json = r#"{"timelines":[{"id":"t","fps":30,"width":1920,"height":1080,
      "tracks":[{"clips":[]}]}]}"#;
    let err = decode(json).expect_err("Track.type is required and tracks is strict");
    assert_eq!(err.kind, ErrorKind::MissingKey("type"));
    assert!(
        err.path.contains("tracks"),
        "path names the location: {}",
        err.path
    );
}

#[test]
fn timeline_requires_fps() {
    let json = r#"{"timelines":[{"id":"t","width":1920,"height":1080,"tracks":[]}]}"#;
    let err = decode(json).expect_err("fps is required");
    assert_eq!(err.kind, ErrorKind::MissingKey("fps"));
}

#[test]
fn marker_requires_comment() {
    let json = r#"{"timelines":[{"id":"t","fps":30,"width":1920,"height":1080,"tracks":[],
      "markers":[{"id":"m","name":"n","startFrame":0,"durationFrames":0,
                  "color":{"r":1,"g":1,"b":1,"a":1}}]}]}"#;
    let p = decode(json).unwrap();
    assert!(
        p.timelines[0].markers.is_empty(),
        "markers is lenient, so a bad marker array is dropped whole"
    );
}

// ---- defaults that are easy to get backwards ----

#[test]
fn track_sync_locked_defaults_to_true() {
    let json = r#"{"timelines":[{"id":"t","fps":30,"width":1920,"height":1080,
      "tracks":[{"type":"video"}]}]}"#;
    let p = decode(json).unwrap();
    assert!(
        p.timelines[0].tracks[0].sync_locked,
        "syncLocked defaults to true, not false"
    );
}

#[test]
fn timeline_name_defaults_to_timeline_one() {
    let p = decode(r#"{"timelines":[{"fps":30,"width":1920,"height":1080,"tracks":[]}]}"#).unwrap();
    assert_eq!(p.timelines[0].name, "Timeline 1");
}

// ---- the two out-of-range policies, which are NOT interchangeable ----

#[test]
fn edge_rounding_coerces_out_of_range_to_zero() {
    let p = decode(&project_with_clip(r#","edgeRounding":1.5"#)).unwrap();
    assert_eq!(
        only_clip(&p).edge_rounding,
        0.0,
        "coerced to 0, not clamped to 1"
    );
}

#[test]
fn display_height_clamps_out_of_range() {
    let json = r#"{"timelines":[{"id":"t","fps":30,"width":1920,"height":1080,
      "tracks":[{"type":"video","displayHeight":9000}]}]}"#;
    let p = decode(json).unwrap();
    assert_eq!(
        p.timelines[0].tracks[0].display_height,
        palmier_core::timeline::track_size::MAX_HEIGHT,
        "clamped to the bound, not coerced to 0"
    );
}

// ---- null is indistinguishable from absent (research.md T007) ----

#[test]
fn explicit_null_behaves_as_absent() {
    let with_null = decode(&project_with_clip(r#","speed":null"#)).unwrap();
    let absent = decode(&project_with_clip("")).unwrap();
    assert_eq!(only_clip(&with_null).speed, only_clip(&absent).speed);
}

// ---- track name normalization drops rather than fails ----

#[test]
fn invalid_track_name_becomes_none_without_failing() {
    let json = r#"{"timelines":[{"id":"t","fps":30,"width":1920,"height":1080,
      "tracks":[{"type":"video","name":"bad\nname"}]}]}"#;
    let p = decode(json).expect("an invalid name must not fail the load");
    assert_eq!(p.timelines[0].tracks[0].name, None);
}

#[test]
fn overlong_track_name_becomes_none() {
    let long = "x".repeat(81);
    let json = format!(
        r#"{{"timelines":[{{"id":"t","fps":30,"width":1920,"height":1080,
        "tracks":[{{"type":"video","name":"{long}"}}]}}]}}"#
    );
    let p = decode(&json).unwrap();
    assert_eq!(p.timelines[0].tracks[0].name, None);
}

// ---- Transform legacy migration ----

#[test]
fn transform_migrates_legacy_x_y() {
    let bytes = include_bytes!("fixtures/legacy-transform-xy.json");
    let p = ProjectFile::decode(bytes).unwrap();
    let t = &only_clip(&p).transform;
    // centerX = x + width - 0.5 = 0.1 + 0.5 - 0.5
    assert!((t.center_x - 0.1).abs() < 1e-9, "got {}", t.center_x);
    // centerY = y + height - 0.5 = 0.2 + 0.4 - 0.5
    assert!((t.center_y - 0.1).abs() < 1e-9, "got {}", t.center_y);
}

// ---- media manifest: regression for the two bugs spec 001's coverage gap hid ----

/// `MediaSource` is a Swift enum with associated values, so it is a single-key object
/// on the wire, never a bare string. Modelling it as a plain string enum meant no real
/// `media.json` could be decoded at all.
#[test]
fn media_source_decodes_as_a_single_key_object() {
    use palmier_core::media::{MediaManifest, MediaSource};
    let json = br#"{"version":1,"entries":[
        {"id":"a1","name":"clip.mp4","type":"video","duration":12.5,
         "source":{"external":{"absolutePath":"/movies/clip.mp4"}}},
        {"id":"a2","name":"take.mov","type":"video","duration":3.0,
         "source":{"project":{"relativePath":"media/take.mov"}}}],
      "folders":[]}"#;
    let mut path = palmier_core::codec::PathStack::new();
    let value: serde_json::Value = serde_json::from_slice(json).unwrap();
    let serde_json::Value::Object(map) = value else {
        unreachable!()
    };
    let manifest = <MediaManifest as palmier_core::codec::FromObject>::from_object(map, &mut path)
        .expect("a real media.json must decode");

    assert_eq!(manifest.entries.len(), 2);
    assert_eq!(
        manifest.entries[0].id, "a1",
        "id is required and was missing from the model"
    );
    assert_eq!(
        manifest.entries[0].source,
        MediaSource::External {
            absolute_path: "/movies/clip.mp4".into()
        }
    );

    let dir = std::path::Path::new("/projects/x.palmier");
    assert_eq!(
        manifest.entries[1].source.resolve(Some(dir)).unwrap(),
        dir.join("media/take.mov")
    );
}
