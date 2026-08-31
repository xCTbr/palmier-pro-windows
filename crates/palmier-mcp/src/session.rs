//! The open project, its edit session, and the disk boundary.
//!
//! Editing never touches disk. A project is written only when `manage_project` is asked
//! to save it (FR-011).

use std::path::{Path, PathBuf};

use palmier_core::edit::EditSession;
use palmier_core::media::{MediaManifest, MediaManifestEntry, MediaSource};
use palmier_core::{ProjectFile, load_project};
use palmier_media::ResolvedMedia;

#[derive(Default)]
pub struct Session {
    edit: Option<EditSession>,
    path: Option<PathBuf>,
    manifest: MediaManifest,
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
    #[error("this project was created in memory and has never been saved — give save a path")]
    NoPath,
    #[error("invalid project settings: fps {fps}, {width}x{height}")]
    InvalidSettings { fps: i64, width: i64, height: i64 },
    #[error("{0}")]
    Decode(#[from] palmier_core::DecodeError),
}

impl Session {
    /// Start an empty project in memory. Nothing is written until `save`.
    ///
    /// Without this the whole "edit my video" flow is unreachable: every other tool
    /// needs an open project, and the only way to get one was to hand-write a
    /// `.palmier` folder first.
    pub fn create(&mut self, fps: i64, width: i64, height: i64) -> Result<(), SessionError> {
        if fps <= 0 || width <= 1 || height <= 1 {
            return Err(SessionError::InvalidSettings { fps, width, height });
        }
        let json = serde_json::json!({
            "timelines": [{
                "id": uuid::Uuid::new_v4().to_string(),
                "name": "Timeline 1",
                "fps": fps,
                "width": width,
                "height": height,
                "tracks": [
                    { "id": uuid::Uuid::new_v4().to_string(), "type": "video", "clips": [] },
                    { "id": uuid::Uuid::new_v4().to_string(), "type": "audio", "clips": [] }
                ]
            }]
        });
        let bytes = serde_json::to_vec(&json).expect("a literal cannot fail to serialize");
        let mut project = ProjectFile::decode(&bytes)?;
        project.active_timeline_id = project.timelines[0].id.clone();
        self.edit = Some(EditSession::new(project));
        self.path = None;
        self.manifest = MediaManifest::default();
        self.dirty = true;
        Ok(())
    }

    pub fn open(&mut self, path: &Path) -> Result<&mut EditSession, SessionError> {
        let project = load_project(path)?;
        let file = if path.is_dir() {
            path.join("project.json")
        } else {
            path.to_path_buf()
        };
        self.edit = Some(EditSession::new(project));
        self.path = Some(file);
        self.manifest = load_manifest(self.package_dir().as_deref());
        self.dirty = false;
        Ok(self.edit.as_mut().expect("just assigned"))
    }

    pub fn close(&mut self) {
        self.edit = None;
        self.path = None;
        self.manifest = MediaManifest::default();
        self.dirty = false;
    }

    /// The `.palmier` folder holding `project.json`, `media.json`, and `media/`.
    pub fn package_dir(&self) -> Option<PathBuf> {
        self.path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
    }

    pub fn manifest(&self) -> &MediaManifest {
        &self.manifest
    }

    pub fn add_media(&mut self, entry: MediaManifestEntry) {
        self.manifest.entries.retain(|e| e.id != entry.id);
        self.manifest.entries.push(entry);
        self.dirty = true;
    }

    /// Write `media.json` without touching the timeline.
    ///
    /// A generated image is already on disk by the time it reaches the manifest, so
    /// leaving the entry in memory would orphan the file if the session ended here —
    /// half the effect persistent and half not. The timeline is a different matter: it
    /// is only written when asked.
    pub fn save_manifest(&self) -> Result<(), SessionError> {
        let Some(path) = self.manifest_path() else {
            return Ok(());
        };
        let object = palmier_core::codec::ToObject::to_object(&self.manifest);
        let bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(object)).map_err(|e| {
            SessionError::Write {
                path: path.display().to_string(),
                source: std::io::Error::other(e),
            }
        })?;
        std::fs::write(&path, bytes).map_err(|source| SessionError::Write {
            path: path.display().to_string(),
            source,
        })
    }

    /// Resolve a `mediaRef` for the render graph. Probes lazily, because the manifest's
    /// `hasAudio` is optional and an older project may not carry it.
    pub fn resolver(&self) -> impl Fn(&str) -> Option<ResolvedMedia> + use<> {
        let dir = self.package_dir();
        let entries: Vec<(String, Option<PathBuf>, Option<bool>)> = self
            .manifest
            .entries
            .iter()
            .map(|e| (e.id.clone(), e.source.resolve(dir.as_deref()), e.has_audio))
            .collect();
        move |media_ref: &str| {
            let (_, path, has_audio) = entries.iter().find(|(id, _, _)| id == media_ref)?;
            let path = path.clone()?;
            if !path.is_file() {
                return None;
            }
            // Probe regardless of what the manifest remembers: only the file itself says
            // whether it is a photograph, and a still has to be looped to render at all.
            let info = palmier_media::probe(&path).ok()?;
            let mut media =
                ResolvedMedia::new(path, has_audio.unwrap_or(info.has_audio), info.has_video);
            media.is_still = info.is_still;
            Some(media)
        }
    }

    fn manifest_path(&self) -> Option<PathBuf> {
        self.package_dir().map(|d| d.join("media.json"))
    }

    pub fn save(&mut self, to: Option<&Path>) -> Result<PathBuf, SessionError> {
        let edit = self.edit.as_ref().ok_or(SessionError::NoProject)?;
        let target = to
            .map(Path::to_path_buf)
            .or_else(|| self.path.clone())
            .ok_or(SessionError::NoPath)?;
        // A `.palmier` is a package directory holding project.json, media.json, and
        // media/ — not a single file. Only an explicit `.json` path means "write here".
        let target = if target
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            target
        } else {
            std::fs::create_dir_all(&target).map_err(|source| SessionError::Write {
                path: target.display().to_string(),
                source,
            })?;
            target.join("project.json")
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
        // The manifest goes out through the same encoder the project uses, so unknown
        // fields from another version survive here too.
        if let Some(manifest_path) = self.manifest_path() {
            let object = palmier_core::codec::ToObject::to_object(&self.manifest);
            if let Ok(bytes) = serde_json::to_vec_pretty(&serde_json::Value::Object(object)) {
                let _ = std::fs::write(manifest_path, bytes);
            }
        }
        self.dirty = false;
        Ok(target)
    }

    pub fn edit(&mut self) -> Result<&mut EditSession, SessionError> {
        self.edit.as_mut().ok_or(SessionError::NoProject)
    }

    /// The timeline every tool reads and edits.
    ///
    /// Reads used to take `timelines[0]` while edits went through the session's active
    /// timeline — invisible while a project had one timeline, wrong the moment it had two.
    pub fn active_timeline(&self) -> Result<&palmier_core::Timeline, SessionError> {
        let project = self.project()?;
        let active = project.active_timeline_id.as_deref();
        project
            .timelines
            .iter()
            .find(|t| active.is_none() || t.id.as_deref() == active)
            .or_else(|| project.timelines.first())
            .ok_or(SessionError::NoProject)
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

/// Read `media.json` beside the project. A missing or unreadable manifest is an empty
/// one, never a failure to open the project.
fn load_manifest(dir: Option<&Path>) -> MediaManifest {
    let Some(dir) = dir else {
        return MediaManifest::default();
    };
    let Ok(bytes) = std::fs::read(dir.join("media.json")) else {
        return MediaManifest::default();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return MediaManifest::default();
    };
    let serde_json::Value::Object(map) = value else {
        return MediaManifest::default();
    };
    let mut path = palmier_core::codec::PathStack::new();
    <MediaManifest as palmier_core::codec::FromObject>::from_object(map, &mut path)
        .unwrap_or_default()
}

/// A manifest entry for a newly imported file.
pub fn entry_for(
    id: String,
    path: &Path,
    info: &palmier_media::MediaInfo,
    package_dir: Option<&Path>,
) -> MediaManifestEntry {
    // Keep the reference relative when the file already lives inside the package.
    let source = package_dir
        .and_then(|dir| path.strip_prefix(dir).ok())
        .map(|rel| MediaSource::Project {
            relative_path: rel.to_string_lossy().into_owned(),
        })
        .unwrap_or_else(|| MediaSource::External {
            absolute_path: path.to_string_lossy().into_owned(),
        });

    MediaManifestEntry {
        id,
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        media_type: if info.has_video {
            palmier_core::ClipType::Video
        } else {
            palmier_core::ClipType::Audio
        },
        source,
        duration: info.duration_seconds,
        generation_input: None,
        source_width: info.width,
        source_height: info.height,
        source_fps: info.fps,
        has_audio: Some(info.has_audio),
        folder_id: None,
        extra: Default::default(),
    }
}
