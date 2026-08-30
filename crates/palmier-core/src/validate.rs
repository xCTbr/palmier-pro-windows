//! Cross-entity invariants checked after decoding. Violations are reported, never
//! silently repaired.

use std::collections::HashSet;

use thiserror::Error;

use crate::project::ProjectFile;
use crate::timeline::ClipType;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValidationError {
    #[error("project has no timelines")]
    NoTimelines,
    #[error("duplicate {kind} id `{id}`")]
    DuplicateId { kind: &'static str, id: String },
    #[error("clip `{clip}` has negative duration {duration}")]
    NegativeDuration { clip: String, duration: i64 },
    #[error("clip `{clip}` frame range overflows i64")]
    FrameOverflow { clip: String },
    #[error("clip `{clip}` has non-finite speed")]
    NonFiniteSpeed { clip: String },
    #[error("clip `{clip}` nests timeline `{target}`, which does not exist")]
    DanglingTimeline { clip: String, target: String },
    #[error("timeline nesting forms a cycle through `{timeline}`")]
    NestingCycle { timeline: String },
}

/// Validate the whole project, returning every violation rather than the first.
pub fn validate(project: &ProjectFile) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    if project.timelines.is_empty() {
        errors.push(ValidationError::NoTimelines);
        return errors;
    }

    let mut timeline_ids = HashSet::new();
    for timeline in &project.timelines {
        if let Some(id) = &timeline.id
            && !timeline_ids.insert(id.clone())
        {
            errors.push(ValidationError::DuplicateId {
                kind: "timeline",
                id: id.clone(),
            });
        }

        let mut track_ids = HashSet::new();
        let mut clip_ids = HashSet::new();
        for track in &timeline.tracks {
            if let Some(id) = &track.id
                && !track_ids.insert(id.clone())
            {
                errors.push(ValidationError::DuplicateId {
                    kind: "track",
                    id: id.clone(),
                });
            }
            for clip in &track.clips {
                let label = clip.id.clone().unwrap_or_else(|| clip.media_ref.clone());
                if let Some(id) = &clip.id
                    && !clip_ids.insert(id.clone())
                {
                    errors.push(ValidationError::DuplicateId {
                        kind: "clip",
                        id: id.clone(),
                    });
                }
                if clip.duration_frames < 0 {
                    errors.push(ValidationError::NegativeDuration {
                        clip: label.clone(),
                        duration: clip.duration_frames,
                    });
                } else if clip.start_frame.checked_add(clip.duration_frames).is_none() {
                    errors.push(ValidationError::FrameOverflow {
                        clip: label.clone(),
                    });
                }
                if !clip.speed.is_finite() {
                    errors.push(ValidationError::NonFiniteSpeed {
                        clip: label.clone(),
                    });
                }
            }
        }
    }

    errors.extend(check_nesting(project, &timeline_ids));
    errors
}

fn check_nesting(project: &ProjectFile, known: &HashSet<String>) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for timeline in &project.timelines {
        for track in &timeline.tracks {
            for clip in &track.clips {
                if clip.source_clip_type == ClipType::Sequence && !known.contains(&clip.media_ref) {
                    errors.push(ValidationError::DanglingTimeline {
                        clip: clip.id.clone().unwrap_or_else(|| clip.media_ref.clone()),
                        target: clip.media_ref.clone(),
                    });
                }
            }
        }
    }

    // Depth-first cycle detection over the nesting graph.
    let mut visiting = HashSet::new();
    let mut done = HashSet::new();
    for timeline in &project.timelines {
        let Some(id) = &timeline.id else { continue };
        if visit(project, id, &mut visiting, &mut done) {
            errors.push(ValidationError::NestingCycle {
                timeline: id.clone(),
            });
        }
    }
    errors
}

/// Returns true when a cycle is reachable from `id`.
fn visit(
    project: &ProjectFile,
    id: &str,
    visiting: &mut HashSet<String>,
    done: &mut HashSet<String>,
) -> bool {
    if done.contains(id) {
        return false;
    }
    if !visiting.insert(id.to_string()) {
        return true;
    }
    let mut cycle = false;
    if let Some(timeline) = project.timeline(id) {
        for track in &timeline.tracks {
            for clip in &track.clips {
                if clip.source_clip_type == ClipType::Sequence
                    && visit(project, &clip.media_ref, visiting, done)
                {
                    cycle = true;
                }
            }
        }
    }
    visiting.remove(id);
    done.insert(id.to_string());
    cycle
}
