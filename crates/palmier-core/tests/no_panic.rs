//! SC-004: no input — valid, malformed, hostile, or truncated — causes a panic.
//! Errors are fine; panics are not.

use palmier_core::ProjectFile;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        let _ = ProjectFile::decode(&bytes);
    }

    #[test]
    fn arbitrary_text_never_panics(text in ".{0,400}") {
        let _ = ProjectFile::decode(text.as_bytes());
    }

    /// Well-formed JSON with arbitrary values in the keys the model reads. This is the
    /// case that actually exercises the strictness helpers.
    #[test]
    fn arbitrary_field_values_never_panic(
        speed in prop::option::of(any::<f64>()),
        start in any::<i64>(),
        duration in any::<i64>(),
        fps in any::<i64>(),
        rounding in any::<f64>(),
        height in any::<f64>(),
        name in ".{0,120}",
    ) {
        let clip = serde_json::json!({
            "mediaRef": "a",
            "startFrame": start,
            "durationFrames": duration,
            "speed": speed,
            "edgeRounding": rounding,
        });
        let doc = serde_json::json!({
            "timelines": [{
                "id": "t", "fps": fps, "width": 1920, "height": 1080,
                "tracks": [{ "type": "video", "name": name,
                             "displayHeight": height, "clips": [clip] }]
            }]
        });
        // serde_json cannot emit NaN or Infinity, so a non-finite generated value
        // simply produces null here — still a valid decode input.
        if let Ok(bytes) = serde_json::to_vec(&doc)
            && let Ok(project) = ProjectFile::decode(&bytes) {
            // Every derived computation must also be panic-free on hostile values.
            let _ = project.timelines[0].total_frames();
            let _ = palmier_core::validate(&project);
            let _ = project.encode();
        }
    }

    /// Round-tripping arbitrary values must not panic either.
    #[test]
    fn encode_of_arbitrary_decoded_input_never_panics(
        markers in prop::collection::vec(
            (any::<i64>(), any::<i64>(), ".{0,40}"), 0..8)
    ) {
        let markers: Vec<_> = markers.into_iter().map(|(s, d, n)| serde_json::json!({
            "id": "m", "name": n, "startFrame": s, "durationFrames": d,
            "color": {"r":1,"g":1,"b":1,"a":1}, "comment": ""
        })).collect();
        let doc = serde_json::json!({
            "timelines": [{ "id": "t", "fps": 30, "width": 1920, "height": 1080,
                            "tracks": [], "markers": markers }]
        });
        let bytes = serde_json::to_vec(&doc).unwrap();
        if let Ok(project) = ProjectFile::decode(&bytes) {
            let _ = project.encode();
        }
    }
}
