//! User Story 3: the summary and its failure paths.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_palmier"))
}

fn fixture(name: &str) -> String {
    format!(
        "{}/../palmier-core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn summarizes_a_full_project() {
    let out = bin()
        .args(["inspect", &fixture("full.json")])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();

    assert!(text.contains("Main [tl-main] (active)"), "{text}");
    assert!(text.contains("1920x1080 @ 30 fps"), "{text}");
    assert!(text.contains("150 frames"), "{text}");
    assert!(
        text.contains("00:00:05:00"),
        "timecode of 150 frames at 30 fps: {text}"
    );
    assert!(text.contains("0. video    V1"), "{text}");
    assert!(text.contains("1. audio"), "{text}");
    assert!(text.contains("2. text"), "{text}");
    assert!(text.contains("2 markers"), "{text}");
}

#[test]
fn summarizes_a_timeline_with_no_tracks() {
    let out = bin()
        .args(["inspect", &fixture("minimal.json")])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("(no tracks)"), "{text}");
    assert!(text.contains("0 frames"), "{text}");
}

#[test]
fn reports_validation_problems_without_failing() {
    let dir = tempdir("validation");
    let path = dir.join("project.json");
    std::fs::write(
        &path,
        br#"{"timelines":[{"id":"t","fps":30,"width":1920,"height":1080,
          "tracks":[{"type":"video","clips":[
            {"id":"c","mediaRef":"a","startFrame":0,"durationFrames":30},
            {"id":"c","mediaRef":"b","startFrame":30,"durationFrames":30}]}]}]}"#,
    )
    .unwrap();
    let out = bin()
        .args(["inspect", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "a loadable project exits 0 even with problems"
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("validation problems"), "{text}");
    assert!(text.contains("duplicate clip id"), "{text}");
}

#[test]
fn accepts_a_folder_as_well_as_a_file() {
    let dir = tempdir("folder");
    std::fs::write(
        dir.join("project.json"),
        std::fs::read(fixture("minimal.json")).unwrap(),
    )
    .unwrap();
    let out = bin()
        .args(["inspect", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ---- failure paths: non-zero exit, no partial summary ----

#[test]
fn missing_path_fails_cleanly() {
    let out = bin()
        .args(["inspect", "/nonexistent/nope.palmier"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "no partial summary on failure");
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a project"));
}

#[test]
fn malformed_json_fails_with_a_diagnostic() {
    let dir = tempdir("malformed");
    let path = dir.join("project.json");
    std::fs::write(&path, b"{ not json").unwrap();
    let out = bin()
        .args(["inspect", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid JSON"));
}

#[test]
fn rejected_document_fails_with_the_json_path() {
    let dir = tempdir("rejected");
    let path = dir.join("project.json");
    std::fs::write(
        &path,
        br#"{"timelines":[{"id":"t","fps":30,"width":1920,"height":1080,
          "tracks":[{"clips":[]}]}]}"#,
    )
    .unwrap();
    let out = bin()
        .args(["inspect", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("missing required key `type`"), "{err}");
    assert!(
        err.contains("tracks"),
        "the diagnostic names the location: {err}"
    );
}

#[test]
fn zero_timeline_project_is_rejected() {
    let dir = tempdir("empty");
    let path = dir.join("project.json");
    std::fs::write(&path, br#"{"timelines":[]}"#).unwrap();
    let out = bin()
        .args(["inspect", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no timelines"));
}

/// Tests run in parallel, so every one gets its own directory.
fn tempdir(tag: &str) -> std::path::PathBuf {
    let unique = format!(
        "palmier-inspect-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    );
    let dir = std::env::temp_dir().join(unique);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Double-clicking a console program on Windows runs it with no arguments and closes
/// the window instantly. Running bare must therefore explain itself rather than fail
/// silently — that is the first thing a new user does, and it is how they learn what
/// this is.
#[test]
fn running_with_no_arguments_explains_what_this_is() {
    let out = bin().output().unwrap();
    assert!(
        out.status.success(),
        "a bare run must not look like an error"
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("not an installer"), "{text}");
    assert!(
        text.contains("serve"),
        "it must name the command to run: {text}"
    );
    assert!(
        text.contains("claude mcp add"),
        "and how to connect an agent: {text}"
    );
    assert!(
        text.contains("FFmpeg"),
        "and whether FFmpeg is there: {text}"
    );
}

#[test]
fn the_ffmpeg_check_reports_what_is_actually_installed() {
    let out = bin().output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let found = std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if found {
        assert!(text.contains("FFmpeg: found"), "{text}");
    } else {
        assert!(text.contains("NOT FOUND"), "{text}");
    }
}

/// Piping the greeting into something that stops reading must not kill the process.
/// `println!` panics on a broken pipe, and CI caught it doing exactly that.
#[test]
fn a_closed_pipe_does_not_panic() {
    use std::io::Read as _;
    let mut child = bin()
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Read one byte and drop the pipe, the way `head -1` or `grep -q` does.
    let mut stdout = child.stdout.take().unwrap();
    let mut byte = [0u8; 1];
    let _ = stdout.read(&mut byte);
    drop(stdout);

    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "panicked on a closed pipe: {stderr}"
    );
}
