//! User Story 2: what this crate writes, it reads back unchanged, and what it writes
//! satisfies the obligations in contracts/project-json.md.
//!
//! These prove **self-consistency only**. They cannot prove a real Mac accepts the
//! output — that is task T060 and it is manual.

use palmier_core::ProjectFile;
use palmier_core::codec::ToObject;
use serde_json::Value;

const FIXTURES: &[(&str, &[u8])] = &[
    ("full", include_bytes!("fixtures/full.json")),
    ("minimal", include_bytes!("fixtures/minimal.json")),
    (
        "unknown-fields",
        include_bytes!("fixtures/unknown-fields.json"),
    ),
    (
        "legacy-transform-xy",
        include_bytes!("fixtures/legacy-transform-xy.json"),
    ),
    (
        "legacy-bare-timeline",
        include_bytes!("fixtures/legacy-bare-timeline.json"),
    ),
];

/// SC-002: decode → encode → decode is stable. Compared on the decoded value, so key
/// order and whitespace are excluded by construction.
#[test]
fn every_fixture_round_trips() {
    for (name, bytes) in FIXTURES {
        let first = ProjectFile::decode(bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let encoded = first.encode();
        let second = ProjectFile::decode(&encoded).unwrap_or_else(|e| {
            panic!(
                "{name}: re-decode failed: {e}\n{}",
                String::from_utf8_lossy(&encoded)
            )
        });
        assert_eq!(first, second, "{name} is not stable across a round trip");
    }
}

#[test]
fn round_trip_is_idempotent_after_the_first_pass() {
    for (name, bytes) in FIXTURES {
        let once = ProjectFile::decode(bytes).unwrap().encode();
        let twice = ProjectFile::decode(&once).unwrap().encode();
        assert_eq!(
            String::from_utf8_lossy(&once),
            String::from_utf8_lossy(&twice),
            "{name}: encoding is not idempotent"
        );
    }
}

fn encode_to_value(bytes: &[u8]) -> Value {
    let project = ProjectFile::decode(bytes).unwrap();
    serde_json::from_slice(&project.encode()).unwrap()
}

/// FR-005: absent optionals are omitted, never written as null.
#[test]
fn absent_optionals_are_omitted_not_null() {
    let v = encode_to_value(include_bytes!("fixtures/minimal.json"));
    let timeline = &v["timelines"][0];
    assert!(
        timeline.get("folderId").is_none(),
        "folderId must be absent, not null"
    );
    assert!(v.get("speakers").is_none());
    assert!(v.get("multicamGroups").is_none());

    fn no_nulls(value: &Value, path: &str) {
        match value {
            Value::Null => panic!("null at {path}"),
            Value::Object(map) => {
                for (k, v) in map {
                    no_nulls(v, &format!("{path}.{k}"));
                }
            }
            Value::Array(items) => {
                for (i, v) in items.iter().enumerate() {
                    no_nulls(v, &format!("{path}[{i}]"));
                }
            }
            _ => {}
        }
    }
    for (name, bytes) in FIXTURES {
        no_nulls(&encode_to_value(bytes), name);
    }
}

/// The original's only custom encoder: nine modern keys, never the legacy pair.
#[test]
fn transform_encodes_modern_keys_and_never_legacy() {
    let v = encode_to_value(include_bytes!("fixtures/legacy-transform-xy.json"));
    let t = &v["timelines"][0]["tracks"][0]["clips"][0]["transform"];
    for key in [
        "centerX",
        "centerY",
        "width",
        "height",
        "rotation",
        "rotationX",
        "rotationY",
        "flipHorizontal",
        "flipVertical",
    ] {
        assert!(t.get(key).is_some(), "missing modern key `{key}`");
    }
    assert!(t.get("x").is_none(), "legacy `x` must never be emitted");
    assert!(t.get("y").is_none(), "legacy `y` must never be emitted");
    // The migrated value survives, so the migration is not re-applied on the next read.
    assert!((t["centerX"].as_f64().unwrap() - 0.1).abs() < 1e-9);
}

/// FR-003: captured unknown keys come back out.
#[test]
fn unknown_keys_are_re_emitted() {
    let v = encode_to_value(include_bytes!("fixtures/unknown-fields.json"));
    assert_eq!(v["futureRootKey"], "also keep me");
    let timeline = &v["timelines"][0];
    assert_eq!(timeline["futureTimelineKey"]["nested"][2], 3);
    let track = &timeline["tracks"][0];
    assert_eq!(track["futureTrackKey"], "keep me");
    let clip = &track["clips"][0];
    assert_eq!(clip["futureClipKey"], 42);
    assert_eq!(clip["transform"]["futureTransformKey"], true);
}

/// Frame values are integers, never exponential notation.
#[test]
fn frame_values_encode_as_integers() {
    let project = ProjectFile::decode(include_bytes!("fixtures/full.json")).unwrap();
    let text = String::from_utf8(project.encode()).unwrap();
    assert!(text.contains("\"startFrame\": 0"), "got: {text}");
    assert!(text.contains("\"durationFrames\": 150"));
    // No number token may use exponential notation: Swift's JSONDecoder accepts it for
    // Double but not for Int, so a frame written as 1e2 would fail on macOS.
    for token in
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+'))
    {
        let looks_numeric = token.starts_with(|c: char| c.is_ascii_digit() || c == '-');
        assert!(
            !(looks_numeric && (token.contains('e') || token.contains('E'))),
            "exponential notation in number token `{token}`"
        );
    }

    let v: Value = serde_json::from_str(&text).unwrap();
    let clip = &v["timelines"][0]["tracks"][0]["clips"][0];
    assert!(clip["startFrame"].is_i64());
    assert!(clip["durationFrames"].is_i64());
    assert!(clip["fadeInFrames"].is_i64());
}

/// Every key the Swift decoder requires must be present in what we write.
#[test]
fn required_keys_are_always_emitted() {
    let v = encode_to_value(include_bytes!("fixtures/minimal.json"));
    let timeline = &v["timelines"][0];
    for key in ["fps", "width", "height", "tracks"] {
        assert!(
            timeline.get(key).is_some(),
            "timeline missing required `{key}`"
        );
    }

    let v = encode_to_value(include_bytes!("fixtures/full.json"));
    let track = &v["timelines"][0]["tracks"][0];
    assert!(track.get("type").is_some(), "track missing required `type`");
    let clip = &track["clips"][0];
    for key in ["mediaRef", "startFrame", "durationFrames"] {
        assert!(clip.get(key).is_some(), "clip missing required `{key}`");
    }
    let marker = &v["timelines"][0]["markers"][0];
    for key in [
        "id",
        "name",
        "startFrame",
        "durationFrames",
        "color",
        "comment",
    ] {
        assert!(marker.get(key).is_some(), "marker missing required `{key}`");
    }
}

/// A project decoded without ids must not gain invented ones on encode — that would
/// make encoding non-deterministic, the same problem id materialization was split out
/// to avoid.
#[test]
fn encoding_does_not_invent_ids() {
    let json = br#"{"timelines":[{"fps":30,"width":1920,"height":1080,
      "tracks":[{"type":"video","clips":[
        {"mediaRef":"a","startFrame":0,"durationFrames":30}]}]}]}"#;
    let a = ProjectFile::decode(json).unwrap().encode();
    let b = ProjectFile::decode(json).unwrap().encode();
    assert_eq!(a, b, "two encodes of the same input must be byte-identical");
    let v: Value = serde_json::from_slice(&a).unwrap();
    assert!(
        v["timelines"][0].get("id").is_none(),
        "absent id stays absent"
    );
}

#[test]
fn a_written_project_still_validates() {
    for (name, bytes) in FIXTURES {
        let project = ProjectFile::decode(bytes).unwrap();
        let reread = ProjectFile::decode(&project.encode()).unwrap();
        assert_eq!(
            palmier_core::validate(&project),
            palmier_core::validate(&reread),
            "{name}: validation differs after a round trip"
        );
    }
}

#[test]
fn to_object_is_reachable_for_downstream_crates() {
    let project = ProjectFile::decode(include_bytes!("fixtures/minimal.json")).unwrap();
    assert!(project.to_object().contains_key("timelines"));
}
