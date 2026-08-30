//! Root of `project.json`, including the legacy bare-`Timeline` fallback.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::codec::{
    DecodeError, Extra, FromObject, Object, PathStack, object_from_value, take_lenient_opt,
    take_object_array, take_object_array_opt,
};
use crate::marker::SpeakerRegistryEntry;
use crate::media::MulticamSource;
use crate::timeline::{Timeline, TimelineViewState};

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectFile {
    pub timelines: Vec<Timeline>,
    pub active_timeline_id: Option<String>,
    pub open_timeline_ids: Option<Vec<String>>,
    pub view_states: Option<BTreeMap<String, TimelineViewState>>,
    pub speakers: Option<Vec<SpeakerRegistryEntry>>,
    pub multicam_groups: Option<Vec<MulticamSource>>,
    pub extra: Extra,
}

impl FromObject for ProjectFile {
    fn from_object(mut o: Object, p: &mut PathStack) -> Result<Self, DecodeError> {
        let timelines = take_object_array(&mut o, "timelines", p)?;
        let view_states = o
            .remove("viewStates")
            .filter(|v| !v.is_null())
            .and_then(|v| match v {
                Value::Object(map) => {
                    let mut out = BTreeMap::new();
                    for (k, v) in map {
                        let inner = object_from_value(v, p).ok()?;
                        out.insert(k, TimelineViewState::from_object(inner, p).ok()?);
                    }
                    Some(out)
                }
                _ => None,
            });
        Ok(Self {
            timelines,
            active_timeline_id: take_lenient_opt(&mut o, "activeTimelineId"),
            open_timeline_ids: take_lenient_opt(&mut o, "openTimelineIds"),
            view_states,
            speakers: take_object_array_opt(&mut o, "speakers", p),
            multicam_groups: take_object_array_opt(&mut o, "multicamGroups", p),
            extra: o,
        })
    }
}

impl ProjectFile {
    /// Decode `project.json`.
    ///
    /// Mirrors `ProjectFile.decode`: parse as a project, and on any failure retry as a
    /// bare `Timeline` and wrap it. A project with zero timelines is rejected.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut path = PathStack::new();
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|e| DecodeError::malformed(&path, format!("invalid JSON: {e}")))?;
        let object = object_from_value(value, &path)?;

        let project = match Self::from_object(object.clone(), &mut path) {
            Ok(project) if !project.timelines.is_empty() => project,
            Ok(_) => {
                return Err(DecodeError::malformed(&path, "project has no timelines"));
            }
            Err(project_error) => {
                // Legacy files are a bare Timeline; anything else surfaces the real error.
                let mut legacy_path = PathStack::new();
                match Timeline::from_object(object, &mut legacy_path) {
                    Ok(timeline) => Self {
                        active_timeline_id: timeline.id.clone(),
                        open_timeline_ids: timeline.id.clone().map(|id| vec![id]),
                        timelines: vec![timeline],
                        view_states: None,
                        speakers: None,
                        multicam_groups: None,
                        extra: Extra::new(),
                    },
                    Err(_) => return Err(project_error),
                }
            }
        };
        Ok(project)
    }

    pub fn timeline(&self, id: &str) -> Option<&Timeline> {
        self.timelines.iter().find(|t| t.id.as_deref() == Some(id))
    }
}
