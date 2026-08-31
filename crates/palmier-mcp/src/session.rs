//! The open project, its edit session, and the disk boundary.
//!
//! Editing never touches disk. A project is written only when `manage_project` is asked
//! to save it (FR-011).

use std::path::{Path, PathBuf};

use palmier_core::edit::EditSession;
use palmier_core::{ProjectFile, load_project};

#[derive(Default)]
pub struct Session {
    edit: Option<EditSession>,
    path: Option<PathBuf>,
    dirty: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("no project is open — call manage_project with action 'open' first")]
    NoProject,
    #[error("{0}")]
    Load(#[from] palmier_core::LoadError),
    #[error("cannot write {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
    #[error("nowhere to save: the project was created in memory, so save needs a path")]
    NoPath,
}

impl Session {
    pub fn open(&mut self, path: &Path) -> Result<&mut EditSession, SessionError> {
        let project = load_project(path)?;
        let file = if path.is_dir() {
            path.join("project.json")
        } else {
            path.to_path_buf()
        };
        self.edit = Some(EditSession::new(project));
        self.path = Some(file);
        self.dirty = false;
        Ok(self.edit.as_mut().expect("just assigned"))
    }

    pub fn close(&mut self) {
        self.edit = None;
        self.path = None;
        self.dirty = false;
    }

    pub fn save(&mut self, to: Option<&Path>) -> Result<PathBuf, SessionError> {
        let edit = self.edit.as_ref().ok_or(SessionError::NoProject)?;
        let target = to
            .map(Path::to_path_buf)
            .or_else(|| self.path.clone())
            .ok_or(SessionError::NoPath)?;
        let target = if target.is_dir() {
            target.join("project.json")
        } else {
            target
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|source| SessionError::Write {
                path: parent.display().to_string(),
                source,
            })?;
        }
        std::fs::write(&target, edit.project.encode()).map_err(|source| SessionError::Write {
            path: target.display().to_string(),
            source,
        })?;
        self.path = Some(target.clone());
        self.dirty = false;
        Ok(target)
    }

    pub fn edit(&mut self) -> Result<&mut EditSession, SessionError> {
        self.edit.as_mut().ok_or(SessionError::NoProject)
    }

    pub fn project(&self) -> Result<&ProjectFile, SessionError> {
        self.edit
            .as_ref()
            .map(|e| &e.project)
            .ok_or(SessionError::NoProject)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn is_open(&self) -> bool {
        self.edit.is_some()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}
