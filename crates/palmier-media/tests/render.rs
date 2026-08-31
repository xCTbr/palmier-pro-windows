//! Graph construction asserted as a string, plus real renders whose *frames* are
//! checked. "ffmpeg exited zero" is not evidence that the picture is right.

use std::path::{Path, PathBuf};
use std::process::Command;

use palmier_core::ProjectFile;
use palmier_media::graph::build;
use palmier_media::{RenderOptions, ResolvedMedia, probe, render};

fn ffmpeg_available() -> bool {
    palmier_media::require_tool("ffmpeg").is_ok() && palmier_media::require_tool("ffprobe").is_ok()
}

/// Unique per test: the suite runs in parallel.
fn workdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "palmier-render-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A synthetic clip with a distinct visual pattern, so frames can be told apart.
fn make_source(dir: &Path, name: &str, pattern: &str, seconds: u32, with_audio: bool) -> PathBuf {
    let path = dir.join(name);
    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-y"]);
    command.args([
        "-f",
        "lavfi",
        "-i",
        &format!("{pattern}=size=320x240:rate=30:duration={seconds}"),
    ]);
    if with_audio {
        command.args([
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={seconds}"),
        ]);
    }
    command.args(["-c:v", "libx264", "-pix_fmt", "yuv420p"]);
    if with_audio {
        command.args(["-c:a", "aac", "-shortest"]);
    }
    command.arg(&path);
    let out = command.output().expect("ffmpeg must run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    path
}

/// The grey value of one pixel row at a given second — enough to tell frames apart.
fn frame_signature(video: &Path, at: f64) -> Vec<u8> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-ss", &format!("{at:.3}")])
        .arg("-i")
        .arg(video)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "gray", "-"])
        .output()
        .expect("ffmpeg must run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn timeline_from(json: &str) -> palmier_core::Timeline {
    ProjectFile::decode(json.as_bytes())
        .unwrap()
        .timelines
        .remove(0)
}

// ------------------------------------------------------------- graph shape

const TWO_CLIPS: &str = r#"{"timelines":[{"id":"tl","fps":30,"width":640,"height":360,
  "tracks":[{"id":"v1","type":"video","clips":[
    {"id":"a","mediaRef":"A","startFrame":0,"durationFrames":60,"trimStartFrame":30},
    {"id":"b","mediaRef":"B","startFrame":60,"durationFrames":60}]}]}]}"#;

/// Resolve a ref to a file, probing it so the graph knows whether it has audio.
fn resolver(map: Vec<(&'static str, PathBuf)>) -> impl Fn(&str) -> Option<ResolvedMedia> {
    move |r: &str| {
        let path = map.iter().find(|(k, _)| *k == r).map(|(_, v)| v.clone())?;
        match probe(&path) {
            Ok(info) => Some(ResolvedMedia::new(path, info.has_audio, info.has_video)),
            // Graph-shape tests use paths that do not exist; assume both streams.
            Err(_) => Some(ResolvedMedia::new(path, true, true)),
        }
    }
}

#[test]
fn a_clip_is_positioned_with_setpts_not_with_overlay_enable() {
    let resolve = resolver(vec![("A", "/a.mp4".into()), ("B", "/b.mp4".into())]);
    let (graph, missing) = build(&timeline_from(TWO_CLIPS), &resolve);
    assert!(missing.is_empty());

    // The second clip starts at frame 60 = 2s, and that offset must be in setpts.
    assert!(
        graph
            .filter_complex
            .contains("setpts=PTS-STARTPTS+2.000000/TB"),
        "{}",
        graph.filter_complex
    );
    // `enable=` gates visibility only; using it to place a clip freezes its last frame.
    assert!(
        !graph.filter_complex.contains("enable="),
        "placement must not rely on enable="
    );
    assert!(
        graph.filter_complex.contains("eof_action=pass"),
        "the base must outlive each clip"
    );
}

#[test]
fn the_source_span_honours_trim_and_speed() {
    let resolve = resolver(vec![("A", "/a.mp4".into()), ("B", "/b.mp4".into())]);
    let (graph, _) = build(&timeline_from(TWO_CLIPS), &resolve);
    // Clip A trims 30 source frames in and consumes 60 at speed 1: [1s, 3s).
    assert!(
        graph
            .filter_complex
            .contains("trim=start=1.000000:end=3.000000"),
        "{}",
        graph.filter_complex
    );
}

#[test]
fn each_distinct_file_becomes_one_input_and_is_reused() {
    let same = PathBuf::from("/same.mp4");
    let resolve = resolver(vec![("A", same.clone()), ("B", same.clone())]);
    let (graph, _) = build(&timeline_from(TWO_CLIPS), &resolve);
    assert_eq!(graph.inputs.len(), 1, "one file, one -i");
    assert_eq!(
        graph.filter_complex.matches("[0:v]").count(),
        2,
        "both clips read input 0"
    );
}

#[test]
fn unresolvable_media_is_reported_not_silently_dropped() {
    let resolve = resolver(vec![("A", "/a.mp4".into())]);
    let (graph, missing) = build(&timeline_from(TWO_CLIPS), &resolve);
    assert_eq!(missing, vec!["B".to_string()]);
    assert_eq!(graph.inputs.len(), 1);
}

#[test]
fn a_hidden_track_contributes_nothing() {
    let json = TWO_CLIPS.replace(
        r#""id":"v1","type":"video""#,
        r#""id":"v1","type":"video","hidden":true"#,
    );
    let resolve = resolver(vec![("A", "/a.mp4".into()), ("B", "/b.mp4".into())]);
    let (graph, _) = build(&timeline_from(&json), &resolve);
    assert!(graph.inputs.is_empty());
    assert!(!graph.filter_complex.contains("overlay"));
}

#[test]
fn an_empty_timeline_still_produces_a_black_base() {
    let json = r#"{"timelines":[{"id":"t","fps":30,"width":640,"height":360,"tracks":[]}]}"#;
    let (graph, _) = build(&timeline_from(json), &resolver(vec![]));
    assert!(
        graph
            .filter_complex
            .starts_with("color=c=black:s=640x360:r=30")
    );
    assert_eq!(graph.video_label, "base");
    assert!(graph.audio_label.is_none());
}

#[test]
fn numbers_never_use_exponential_notation() {
    let json = r#"{"timelines":[{"id":"t","fps":30,"width":640,"height":360,
      "tracks":[{"id":"v","type":"video","clips":[
        {"id":"c","mediaRef":"A","startFrame":0,"durationFrames":1}]}]}]}"#;
    let (graph, _) = build(
        &timeline_from(json),
        &resolver(vec![("A", "/a.mp4".into())]),
    );
    for token in graph
        .filter_complex
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'e' || c == '-'))
    {
        let numeric = token.starts_with(|c: char| c.is_ascii_digit() || c == '-')
            && token.chars().any(|c| c.is_ascii_digit());
        assert!(
            !(numeric && token.contains('e')),
            "exponential notation in `{token}` — ffmpeg parses it as a filter name"
        );
    }
}

// ------------------------------------------------------ real renders

#[test]
fn renders_a_two_clip_cut_whose_frames_actually_change() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let dir = workdir("cut");
    let a = make_source(&dir, "a.mp4", "testsrc", 4, true);
    let b = make_source(&dir, "b.mp4", "testsrc2", 4, false);

    let timeline = timeline_from(TWO_CLIPS);
    let resolve = resolver(vec![("A", a), ("B", b)]);
    let output = dir.join("out.mp4");
    let report = render(&timeline, &resolve, &RenderOptions::new(&output)).expect("render");

    assert!(report.missing_media.is_empty());
    assert_eq!((report.width, report.height, report.fps), (640, 360, 30));

    let info = probe(&output).expect("probe the render");
    assert!(
        (info.duration_seconds - 4.0).abs() < 0.2,
        "got {}s",
        info.duration_seconds
    );
    assert_eq!((info.width, info.height), (Some(640), Some(360)));
    assert!(
        info.has_audio,
        "clip A's audio must survive into the render"
    );

    // The second clip spans [2s, 4s). If it were placed with `enable=` its last frame
    // would be frozen across that span, so three samples would be identical.
    let (s1, s2, s3) = (
        frame_signature(&output, 2.2),
        frame_signature(&output, 2.9),
        frame_signature(&output, 3.6),
    );
    assert!(!s1.is_empty());
    assert!(
        s1 != s2 && s2 != s3,
        "frames in the second clip are frozen — placement is wrong, not merely ugly"
    );

    // And the two clips must not look the same: the cut really happened.
    assert_ne!(frame_signature(&output, 0.5), frame_signature(&output, 3.0));
}

#[test]
fn a_gap_renders_black_rather_than_shortening_the_film() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let dir = workdir("gap");
    let a = make_source(&dir, "a.mp4", "testsrc", 2, false);
    let json = r#"{"timelines":[{"id":"t","fps":30,"width":320,"height":240,
      "tracks":[{"id":"v","type":"video","clips":[
        {"id":"c","mediaRef":"A","startFrame":60,"durationFrames":30}]}]}]}"#;
    let output = dir.join("out.mp4");
    render(
        &timeline_from(json),
        &resolver(vec![("A", a)]),
        &RenderOptions::new(&output),
    )
    .expect("render");

    let info = probe(&output).unwrap();
    assert!(
        (info.duration_seconds - 3.0).abs() < 0.2,
        "got {}s",
        info.duration_seconds
    );

    let opening = frame_signature(&output, 0.5);
    let mean: f64 = opening.iter().map(|b| *b as f64).sum::<f64>() / opening.len() as f64;
    assert!(
        mean < 8.0,
        "the leading gap should be black, mean luminance was {mean}"
    );
}

#[test]
fn probing_reports_what_a_file_is() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let dir = workdir("probe");
    let a = make_source(&dir, "a.mp4", "testsrc", 2, true);
    let info = probe(&a).unwrap();
    assert!((info.duration_seconds - 2.0).abs() < 0.1);
    assert_eq!((info.width, info.height), (Some(320), Some(240)));
    assert_eq!(info.fps, Some(30.0));
    assert!(info.has_video && info.has_audio);
    assert_eq!(info.duration_frames(30), 60);
}

#[test]
fn probing_a_missing_or_bogus_file_errors_cleanly() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let dir = workdir("badprobe");
    assert!(probe(&dir.join("nope.mp4")).is_err());
    let junk = dir.join("junk.mp4");
    std::fs::write(&junk, b"this is not a video").unwrap();
    assert!(probe(&junk).is_err());
}

#[test]
fn rendering_an_empty_timeline_is_refused_with_a_reason() {
    let json = r#"{"timelines":[{"id":"t","fps":30,"width":320,"height":240,"tracks":[]}]}"#;
    let dir = workdir("emptyrender");
    let error = render(
        &timeline_from(json),
        &resolver(vec![]),
        &RenderOptions::new(dir.join("out.mp4")),
    )
    .expect_err("an empty timeline has nothing to render");
    assert!(error.to_string().contains("empty"), "{error}");
}

#[test]
fn an_unsupported_codec_is_refused_before_ffmpeg_runs() {
    let dir = workdir("codec");
    let mut options = RenderOptions::new(dir.join("out.mp4"));
    options.codec = "prores".into();
    let error = render(&timeline_from(TWO_CLIPS), &resolver(vec![]), &options).unwrap_err();
    assert!(error.to_string().contains("prores"), "{error}");
}
