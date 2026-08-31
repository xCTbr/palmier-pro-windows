//! Media probing and rendering, by shelling out to the `ffmpeg` and `ffprobe` binaries.
//!
//! Layer 0 links no `libav*` and runs no bindgen: it builds a filter graph and hands it
//! to the CLI. The graph is a string, so it is testable without decoding a single frame.

pub mod graph;
pub mod probe;
pub mod render;
pub mod silence;

pub use graph::{FilterGraph, ResolvedMedia, build_with};
pub use probe::{MediaInfo, probe};
pub use render::{FrameOptions, RenderOptions, frame_png, render, render_frame};
pub use silence::{SilentSpan, detect as detect_silence, speech_spans};

use std::collections::HashSet;
use std::process::Command;
use std::sync::{LazyLock, Mutex};

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

/// Keep a child process from opening a console window of its own.
///
/// Windows gives every console-subsystem child of a GUI process a fresh console, so
/// without this each `ffmpeg` call flashes a black box over the app.
pub fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Tools already found. A binary on PATH does not leave mid-session, and the UI asks
/// for status every few seconds — probing each time is a process spawn for nothing.
static FOUND: LazyLock<Mutex<HashSet<&'static str>>> = LazyLock::new(Mutex::default);

/// Check that a tool exists before trying to use it, so the diagnostic names the real
/// problem instead of surfacing an ENOENT from deep inside a render.
pub fn require_tool(tool: &'static str) -> Result<(), MediaError> {
    if FOUND.lock().is_ok_and(|found| found.contains(tool)) {
        return Ok(());
    }

    quiet(&mut Command::new(tool))
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| MediaError::ToolMissing { tool })
        .and_then(|status| {
            if !status.success() {
                return Err(MediaError::ToolMissing { tool });
            }
            if let Ok(mut found) = FOUND.lock() {
                found.insert(tool);
            }
            Ok(())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // The UI polls status every few seconds. Before the cache, every poll spawned
    // `ffmpeg -version` and `ffprobe -version` — on Windows that is a console window
    // flashing over the app, twice, forever.
    #[test]
    fn a_found_tool_is_not_probed_again() {
        let ghost = "palmier-cache-probe-not-a-real-binary";
        FOUND.lock().unwrap().insert(ghost);

        // No such binary exists, so only the cache can make this succeed.
        assert!(require_tool(ghost).is_ok());
    }

    #[test]
    fn a_missing_tool_is_not_cached() {
        let ghost = "palmier-missing-probe-not-a-real-binary";
        assert!(matches!(
            require_tool(ghost),
            Err(MediaError::ToolMissing { .. })
        ));
        // Otherwise installing FFmpeg would need an app restart to be noticed.
        assert!(!FOUND.lock().unwrap().contains(ghost));
    }
}
