//! Running the graph.

use std::path::{Path, PathBuf};
use std::process::Command;

use palmier_core::timeline::Timeline;

use crate::graph::{FilterGraph, ResolvedMedia, build, build_with};
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

/// How a single composited frame should be drawn.
#[derive(Debug, Clone, Copy)]
pub struct FrameOptions {
    /// Overlay a 0–1 coordinate grid so an agent can describe positions in the canvas.
    pub grid: bool,
    /// Longest edge of the returned image. Keeps a response readable without shipping a
    /// full-resolution frame as base64.
    pub max_width: i64,
}

impl Default for FrameOptions {
    fn default() -> Self {
        Self {
            grid: true,
            max_width: 640,
        }
    }
}

/// Render one composited frame to an image file — what an agent looks at to check its
/// own edit, and the seed of layer 1's preview.
///
/// The frame number is deliberately *not* burned into the pixels. The original does that
/// because its agent only receives images; over MCP a text block precedes each image,
/// which is unambiguous, machine-readable, and does not make the render depend on
/// `drawtext`, libfreetype, and a platform-specific font path.
pub fn render_frame(
    timeline: &Timeline,
    resolve: &dyn Fn(&str) -> Option<ResolvedMedia>,
    frame: i64,
    output: &Path,
    options: FrameOptions,
) -> Result<(), MediaError> {
    require_tool("ffmpeg")?;
    let (graph, _) = build_with(timeline, resolve, false);
    let frame = frame.max(0);
    let at = frame as f64 / graph.fps.max(1) as f64;

    let mut post = String::new();
    // A frame past the end of all content still has an answer: black. Pad the composited
    // stream so the frame exists, which is what the tool contract promises.
    if at >= graph.duration_seconds {
        let extra = at - graph.duration_seconds + 1.0;
        post.push_str(&format!(
            "tpad=stop_mode=add:stop_duration={extra:.6}:color=black"
        ));
        post.push(',');
    }
    // Select by frame number rather than seeking by time. A seek lands on the nearest
    // decodable frame; an editor needs the frame it asked for.
    post.push_str(&format!("select=eq(n\\,{frame})"));
    if options.grid {
        if !post.is_empty() {
            post.push(',');
        }
        // Ten cells across each axis, so one cell edge is 0.1 in canvas coordinates.
        post.push_str("drawgrid=w=iw/10:h=ih/10:t=1:c=white@0.35");
    }
    if options.max_width > 0 && graph.width > options.max_width {
        if !post.is_empty() {
            post.push(',');
        }
        post.push_str(&format!("scale={}:-2", options.max_width));
    }

    let filter = format!(
        "{};[{}]{}[shown]",
        graph.filter_complex, graph.video_label, post
    );
    let label = "shown";

    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-nostdin", "-y"]);
    for input in &graph.inputs {
        command.arg("-i").arg(&input.path);
    }
    command.arg("-filter_complex").arg(&filter);
    command.arg("-map").arg(format!("[{label}]"));
    command.args(["-frames:v", "1"]);
    command.args(["-fps_mode", "passthrough"]);
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

/// Render one composited frame and hand back the PNG bytes.
pub fn frame_png(
    timeline: &Timeline,
    resolve: &dyn Fn(&str) -> Option<ResolvedMedia>,
    frame: i64,
    options: FrameOptions,
) -> Result<Vec<u8>, MediaError> {
    // Unique per process and thread: renders run in parallel under test.
    let dir = std::env::temp_dir().join(format!(
        "palmier-frame-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).map_err(|source| MediaError::Io {
        path: dir.display().to_string(),
        source,
    })?;
    let path = dir.join(format!("f{frame}.png"));
    let result = render_frame(timeline, resolve, frame, &path, options).and_then(|()| {
        std::fs::read(&path).map_err(|source| MediaError::Io {
            path: path.display().to_string(),
            source,
        })
    });
    let _ = std::fs::remove_file(&path);
    result
}
