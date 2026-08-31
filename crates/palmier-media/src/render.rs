//! Running the graph.

use std::path::{Path, PathBuf};
use std::process::Command;

use palmier_core::timeline::Timeline;

use crate::graph::{FilterGraph, ResolvedMedia, build};
use crate::{MediaError, require_tool};

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub output: PathBuf,
    /// `h264` or `h265`.
    pub codec: String,
    /// Constant Rate Factor. Lower is better quality and a larger file.
    pub crf: u32,
}

impl RenderOptions {
    pub fn new(output: impl Into<PathBuf>) -> Self {
        Self {
            output: output.into(),
            codec: "h264".into(),
            crf: 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderReport {
    pub output: PathBuf,
    pub duration_seconds: f64,
    pub width: i64,
    pub height: i64,
    pub fps: i64,
    /// Media refs that did not resolve; their clips are absent from the render.
    pub missing_media: Vec<String>,
}

fn encoder(codec: &str) -> Result<&'static str, MediaError> {
    match codec {
        "h264" => Ok("libx264"),
        "h265" | "hevc" => Ok("libx265"),
        other => Err(MediaError::Unsupported(format!(
            "codec `{other}`; layer 0 renders h264 or h265"
        ))),
    }
}

pub fn render(
    timeline: &Timeline,
    resolve: &dyn Fn(&str) -> Option<ResolvedMedia>,
    options: &RenderOptions,
) -> Result<RenderReport, MediaError> {
    require_tool("ffmpeg")?;
    let encoder = encoder(&options.codec)?;

    let (graph, missing) = build(timeline, resolve);
    if graph.duration_seconds <= 0.0 {
        return Err(MediaError::Invalid(
            "the timeline is empty; there is nothing to render".into(),
        ));
    }
    run(&graph, encoder, options)?;

    Ok(RenderReport {
        output: options.output.clone(),
        duration_seconds: graph.duration_seconds,
        width: graph.width,
        height: graph.height,
        fps: graph.fps,
        missing_media: missing,
    })
}

fn run(graph: &FilterGraph, encoder: &str, options: &RenderOptions) -> Result<(), MediaError> {
    if let Some(parent) = options.output.parent() {
        std::fs::create_dir_all(parent).map_err(|source| MediaError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }

    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-nostdin", "-y"]);
    for input in &graph.inputs {
        command.arg("-i").arg(&input.path);
    }
    command.arg("-filter_complex").arg(&graph.filter_complex);
    command.arg("-map").arg(format!("[{}]", graph.video_label));
    if let Some(audio) = &graph.audio_label {
        command.arg("-map").arg(format!("[{audio}]"));
        command.args(["-c:a", "aac"]);
    }
    command.args(["-c:v", encoder]);
    command.args(["-crf", &options.crf.to_string()]);
    command.args(["-pix_fmt", "yuv420p"]);
    command.args(["-t", &format!("{:.6}", graph.duration_seconds)]);
    command.arg(&options.output);

    let output = command
        .output()
        .map_err(|_| MediaError::ToolMissing { tool: "ffmpeg" })?;
    if !output.status.success() {
        return Err(MediaError::ToolFailed {
            tool: "ffmpeg",
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(())
}

/// Render one frame to an image — the seed of layer 1's preview.
pub fn render_frame(
    timeline: &Timeline,
    resolve: &dyn Fn(&str) -> Option<ResolvedMedia>,
    frame: i64,
    output: &Path,
) -> Result<(), MediaError> {
    require_tool("ffmpeg")?;
    let (graph, _) = build(timeline, resolve);
    let at = frame as f64 / graph.fps.max(1) as f64;

    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-nostdin", "-y"]);
    for input in &graph.inputs {
        command.arg("-i").arg(&input.path);
    }
    command.arg("-filter_complex").arg(&graph.filter_complex);
    command.arg("-map").arg(format!("[{}]", graph.video_label));
    command.args(["-ss", &format!("{at:.6}")]);
    command.args(["-frames:v", "1"]);
    command.arg(output);

    let out = command
        .output()
        .map_err(|_| MediaError::ToolMissing { tool: "ffmpeg" })?;
    if !out.status.success() {
        return Err(MediaError::ToolFailed {
            tool: "ffmpeg",
            message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(())
}
