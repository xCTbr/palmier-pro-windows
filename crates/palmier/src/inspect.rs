//! `palmier inspect <path>` — read a project and describe what is in it.

use std::path::Path;

use palmier_core::frames::timecode;
use palmier_core::{ClipType, ProjectFile, load_project, validate};

/// Render the summary. Separated from IO so it can be asserted in tests.
pub fn summarize(project: &ProjectFile) -> String {
    let mut out = String::new();
    for (index, timeline) in project.timelines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let id = timeline.id.as_deref().unwrap_or("<no id>");
        let active = if project.active_timeline_id.as_deref() == timeline.id.as_deref() {
            " (active)"
        } else {
            ""
        };
        out.push_str(&format!("{} [{id}]{active}\n", timeline.name));

        let frames = timeline.total_frames().unwrap_or(0);
        let tc = timecode(frames, timeline.fps).unwrap_or_else(|_| "--:--:--:--".into());
        out.push_str(&format!(
            "  {}x{} @ {} fps · {frames} frames · {tc}\n",
            timeline.width, timeline.height, timeline.fps
        ));

        if timeline.tracks.is_empty() {
            out.push_str("  (no tracks)\n");
        }
        for (i, track) in timeline.tracks.iter().enumerate() {
            let name = track.name.as_deref().unwrap_or("-");
            let kind = match track.track_type {
                ClipType::Video => "video",
                ClipType::Audio => "audio",
                ClipType::Image => "image",
                ClipType::Text => "text",
                ClipType::Lottie => "lottie",
                ClipType::Sequence => "sequence",
                ClipType::Subtitle => "subtitle",
            };
            let mut flags = Vec::new();
            if track.muted {
                flags.push("muted");
            }
            if track.hidden {
                flags.push("hidden");
            }
            let flags = if flags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", flags.join(", "))
            };
            out.push_str(&format!(
                "  {i}. {kind:<8} {name:<12} {} clips{flags}\n",
                track.clips.len()
            ));
        }
        if !timeline.markers.is_empty() {
            out.push_str(&format!("  {} markers\n", timeline.markers.len()));
        }
    }

    let problems = validate(project);
    if !problems.is_empty() {
        out.push_str(&format!("\n{} validation problems:\n", problems.len()));
        for problem in &problems {
            out.push_str(&format!("  - {problem}\n"));
        }
    }
    out
}

/// Returns the process exit code.
pub fn run(path: &Path) -> i32 {
    match load_project(path) {
        Ok(project) => {
            print!("{}", summarize(&project));
            0
        }
        Err(error) => {
            // A failed load prints no partial summary.
            eprintln!("palmier: {error}");
            1
        }
    }
}
