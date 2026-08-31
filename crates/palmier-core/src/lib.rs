//! The `.palmier` project model.
//!
//! Compatibility with the original macOS format is a constitutional principle, not a
//! feature: the decoding contract these types implement is audited in
//! `specs/001-project-model/research.md`, and that audit is authoritative.

pub mod codec;
pub mod edit;
pub mod effect;
pub mod frames;
pub mod ids;
pub mod keyframe;
pub mod marker;
pub mod media;
pub mod project;
pub mod text;
pub mod timeline;
pub mod transform;
pub mod validate;

pub use codec::{DecodeError, ErrorKind};
pub use frames::{FrameError, FrameRange};
pub use project::ProjectFile;
pub use timeline::{Clip, ClipType, Timeline, Track};
pub use validate::{ValidationError, validate};

use std::path::Path;

/// Everything that can go wrong loading a project from disk.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Decode(#[from] DecodeError),
    #[error("path is not a project: {0}")]
    NotAProject(String),
}

/// Load a project from a `.palmier` folder or a `project.json` path.
///
/// This is the only place in the crate that touches the filesystem (FR-010): the model
/// types decode from bytes and know nothing about paths.
pub fn load_project(path: &Path) -> Result<ProjectFile, LoadError> {
    let file = if path.is_dir() {
        path.join("project.json")
    } else {
        path.to_path_buf()
    };
    if !file.is_file() {
        return Err(LoadError::NotAProject(file.display().to_string()));
    }
    let bytes = std::fs::read(&file)?;
    let mut project = ProjectFile::decode(&bytes)?;
    ids::materialize_ids(&mut project);
    Ok(project)
}
