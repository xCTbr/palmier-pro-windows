//! SC-005: a 10,000-clip project loads in under 500 ms.
//!
//! This is the gate on the map-buffering kernel, which allocates per object. Run with:
//! `cargo test -p palmier-core --release -- --ignored bench_large_project`

use std::time::Instant;

use palmier_core::ProjectFile;

fn large_project(tracks: usize, clips_per_track: usize) -> Vec<u8> {
    let mut out =
        String::from(r#"{"timelines":[{"id":"t","fps":30,"width":1920,"height":1080,"tracks":["#);
    for t in 0..tracks {
        if t > 0 {
            out.push(',');
        }
        out.push_str(&format!(r#"{{"type":"video","id":"tr-{t}","clips":["#));
        for c in 0..clips_per_track {
            if c > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                r#"{{"id":"c-{t}-{c}","mediaRef":"asset-{c}","startFrame":{},"durationFrames":30,
                   "speed":1.0,"opacity":1.0,"transform":{{"centerX":0.5,"centerY":0.5}}}}"#,
                c * 30
            ));
        }
        out.push_str("]}");
    }
    out.push_str("]}]}");
    out.into_bytes()
}

#[test]
#[ignore = "performance gate; run in release with --ignored"]
fn bench_large_project() {
    let bytes = large_project(20, 500);
    let clips = 20 * 500;
    println!("document: {} KiB, {clips} clips", bytes.len() / 1024);

    // Warm up so the measurement is not dominated by first-touch page faults.
    let _ = ProjectFile::decode(&bytes).unwrap();

    let start = Instant::now();
    let project = ProjectFile::decode(&bytes).unwrap();
    let elapsed = start.elapsed();

    let total: usize = project.timelines[0]
        .tracks
        .iter()
        .map(|t| t.clips.len())
        .sum();
    assert_eq!(total, clips);
    println!("decoded {clips} clips in {elapsed:?}");
    assert!(
        elapsed.as_millis() < 500,
        "SC-005: {clips} clips took {elapsed:?}, budget is 500ms"
    );
}

/// A ripple across a large timeline must stay interactive.
#[test]
#[ignore = "performance gate; run in release with --ignored"]
fn bench_large_ripple() {
    use palmier_core::edit::{EditCommand, EditSession};
    use std::time::Instant;

    let mut clips = String::new();
    for i in 0..10_000 {
        if i > 0 {
            clips.push(',');
        }
        clips.push_str(&format!(
            r#"{{"id":"c{i}","mediaRef":"m","startFrame":{},"durationFrames":30}}"#,
            i * 30
        ));
    }
    let json = format!(
        r#"{{"timelines":[{{"id":"tl","fps":30,"width":1920,"height":1080,
           "tracks":[{{"id":"v1","type":"video","clips":[{clips}]}}]}}],"activeTimelineId":"tl"}}"#
    );
    let mut session = EditSession::new(ProjectFile::decode(json.as_bytes()).unwrap());

    let start = Instant::now();
    session
        .apply(EditCommand::RippleDeleteRanges {
            ranges: vec![(0, 3000)],
        })
        .unwrap();
    let elapsed = start.elapsed();

    println!("rippled 10,000 clips in {elapsed:?}");
    assert!(
        elapsed.as_millis() < 50,
        "ripple took {elapsed:?}, budget is 50ms"
    );
}
