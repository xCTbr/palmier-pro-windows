//! The project's record of its media. Modeled and round-tripped only — this feature
//! resolves no file on disk.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::codec::{DecodeError, Extra, FromObject, Object, PathStack, take_or_default};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MediaSource {
    Local,
    Generated,
    Remote,
}

/// `name` required; `parentFolderId` optional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFolder {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_folder_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// `name`, `type`, `source`, and `duration` required; the rest optional.
///
/// `generationInput` is deliberately kept as an opaque `Value`: it carries 30-plus
/// provider-shaped optional fields, and the project model must not know a provider
/// exists (constitution, Generation is optional everywhere). Preserved verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaManifestEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub media_type: crate::timeline::ClipType,
    pub source: MediaSource,
    pub duration: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_width: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_height: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_fps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_audio: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Nothing required — all `decodeIfPresent`.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaManifest {
    pub version: i64,
    pub entries: Vec<MediaManifestEntry>,
    pub folders: Vec<MediaFolder>,
    pub extra: Extra,
}

impl Default for MediaManifest {
    fn default() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
            folders: Vec::new(),
            extra: Extra::new(),
        }
    }
}

impl FromObject for MediaManifest {
    fn from_object(mut o: Object, p: &mut PathStack) -> Result<Self, DecodeError> {
        Ok(Self {
            version: take_or_default(&mut o, "version", "integer", 1, p)?,
            entries: take_or_default(&mut o, "entries", "array", Vec::new(), p)?,
            folders: take_or_default(&mut o, "folders", "array", Vec::new(), p)?,
            extra: o,
        })
    }
}

/// Multicam grouping. Kept opaque in this feature: it is modeled as preserved data
/// and read by no code until layer 3.
#[derive(Debug, Clone, PartialEq)]
pub struct MulticamSource {
    pub raw: Object,
}

impl FromObject for MulticamSource {
    fn from_object(o: Object, _p: &mut PathStack) -> Result<Self, DecodeError> {
        Ok(Self { raw: o })
    }
}
