//! Media probing and rendering, by shelling out to the `ffmpeg` and `ffprobe` binaries.
//!
//! Layer 0 links no `libav*` and runs no bindgen: it builds a filter graph and hands it
//! to the CLI. The graph is a string, so it is testable without decoding a single frame.

pub mod graph;
pub mod probe;
pub mod render;

pub use graph::{FilterGraph, ResolvedMedia};
pub use probe::{MediaInfo, probe};
pub use render::{RenderOptions, render};

use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("`{tool}` is not on PATH — install FFmpeg and try again")]
    ToolMissing { tool: &'static str },
    #[error("`{tool}` failed: {message}")]
    ToolFailed { tool: &'static str, message: String },
    #[error("cannot read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Invalid(String),
}

/// Check that a tool exists before trying to use it, so the diagnostic names the real
/// problem instead of surfacing an ENOENT from deep inside a render.
pub fn require_tool(tool: &'static str) -> Result<(), MediaError> {
    Command::new(tool)
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| MediaError::ToolMissing { tool })
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(MediaError::ToolMissing { tool })
            }
        })
}
