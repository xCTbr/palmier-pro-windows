//! Does render time grow with clip count, or only with duration?
//!
//! A real export of a 17s timeline with 26 clips took longer than a 34s timeline with
//! 2 clips. If cost tracks clip count rather than footage length, the graph is wrong,
//! not the encoder settings.

use std::path::PathBuf;
use std::time::Instant;

use palmier_core::ProjectFile;
use palmier_media::{RenderOptions, ResolvedMedia, render};

fn workdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("palmier-scale-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `count` clips laid end to end, always totalling the same number of frames.
fn timeline_of(count: usize, total_frames: i64) -> palmier_core::Timeline {
    let per = total_frames / count as i64;
    let clips: Vec<String> = (0..count)
        .map(|i| {
            format!(
                r#"{{"id":"c{i}","mediaRef":"S","startFrame":{},"durationFrames":{per},
                    "trimStartFrame":{}}}"#,
                i as i64 * per,
                i as i64 * per
            )
        })
        .collect();
    let json = format!(
        r#"{{"timelines":[{{"id":"t","fps":30,"width":640,"height":360,
           "tracks":[{{"id":"v","type":"video","clips":[{}]}}]}}]}}"#,
        clips.join(",")
    );
    ProjectFile::decode(json.as_bytes())
        .unwrap()
        .timelines
        .remove(0)
}

#[test]
#[ignore = "measurement; run in release with --ignored"]
fn render_cost_against_clip_count() {
    if palmier_media::require_tool("ffmpeg").is_err() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let dir = workdir("count");
    let source = dir.join("src.mp4");
    let out = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=640x360:rate=30:duration=20",
        ])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(out.status.success());

    let resolve = |_: &str| Some(ResolvedMedia::new(source.clone(), false, true));
    println!("\n{:>7} {:>9} {:>10}", "clips", "seconds", "render");
    let mut first = None;
    for count in [1usize, 2, 4, 8, 16, 32] {
        // Duration is held constant: only the clip count changes.
        let timeline = timeline_of(count, 300);
        let output = dir.join(format!("out{count}.mp4"));
        let start = Instant::now();
        render(&timeline, &resolve, &RenderOptions::new(&output)).expect("render");
        let elapsed = start.elapsed();
        println!("{count:>7} {:>8}s {:>9.2?}", 10, elapsed);
        first.get_or_insert(elapsed);
    }
    println!();
}
