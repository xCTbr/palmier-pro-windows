//! The inverse patch: the minimal prior state needed to reverse one command.
//!
//! Produced during the plan phase, so an entry can never exist for a command that did
//! not commit. See specs/002-edit-commands/research.md Q1.

use crate::marker::TimelineMarker;
use crate::timeline::{Clip, Timeline, Track};

/// One reversible change to a timeline.
#[derive(Debug, Clone, PartialEq)]
pub enum Undo {
    /// A clip that existed before and must come back, with the track it lived on.
    RestoreClip { track_id: String, clip: Box<Clip> },
    /// A clip that did not exist before and must go away.
    DeleteClip { clip_id: String },
    /// A clip whose fields changed; the whole prior value is restored.
    RestoreClipState { clip: Box<Clip> },
    /// A clip that only moved along its track. Recording the frame instead of the whole
    /// clip keeps a ripple over thousands of clips from cloning all of them.
    RestoreClipStart { clip_id: String, start_frame: i64 },
    /// A clip that moved between tracks.
    MoveClipToTrack { clip_id: String, track_id: String },
    /// A track that existed before.
    RestoreTrack { index: usize, track: Box<Track> },
    /// A track that did not exist before.
    DeleteTrack { track_id: String },
    /// A track whose own fields changed, clips excluded.
    RestoreTrackState { track_id: String, track: Box<Track> },
    /// The marker list before the command.
    RestoreMarkers { markers: Vec<TimelineMarker> },
}

/// Everything needed to reverse one command, in the order it must be undone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InversePatch {
    pub timeline_id: String,
    pub steps: Vec<Undo>,
}

impl InversePatch {
    pub fn new(timeline_id: impl Into<String>) -> Self {
        Self {
            timeline_id: timeline_id.into(),
            steps: Vec::new(),
        }
    }

    pub fn push(&mut self, step: Undo) {
        self.steps.push(step);
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Reverse the command this patch describes. Steps are applied in reverse order so
    /// later effects unwind before earlier ones.
    ///
    /// Crate-private: `EditSession` is the only public way to mutate a project
    /// (constitution principle I), so a caller cannot apply a patch behind its back.
    pub(crate) fn apply(&self, timeline: &mut Timeline) {
        for step in self.steps.iter().rev() {
            apply_step(timeline, step);
        }
        sort_all(timeline);
    }
}

fn apply_step(timeline: &mut Timeline, step: &Undo) {
    match step {
        Undo::RestoreClip { track_id, clip } => {
            remove_clip_anywhere(timeline, clip.id.as_deref().unwrap_or_default());
            if let Some(track) = track_by_id_mut(timeline, track_id) {
                track.clips.push((**clip).clone());
            }
        }
        Undo::DeleteClip { clip_id } => {
            remove_clip_anywhere(timeline, clip_id);
        }
        Undo::RestoreClipStart {
            clip_id,
            start_frame,
        } => {
            for track in &mut timeline.tracks {
                if let Some(slot) = track
                    .clips
                    .iter_mut()
                    .find(|c| c.id.as_deref() == Some(clip_id.as_str()))
                {
                    slot.start_frame = *start_frame;
                    return;
                }
            }
        }
        Undo::RestoreClipState { clip } => {
            let id = clip.id.as_deref().unwrap_or_default();
            for track in &mut timeline.tracks {
                if let Some(slot) = track.clips.iter_mut().find(|c| c.id.as_deref() == Some(id)) {
                    *slot = (**clip).clone();
                    return;
                }
            }
        }
        Undo::MoveClipToTrack { clip_id, track_id } => {
            if let Some(clip) = take_clip_anywhere(timeline, clip_id)
                && let Some(track) = track_by_id_mut(timeline, track_id)
            {
                track.clips.push(clip);
            }
        }
        Undo::RestoreTrack { index, track } => {
            let at = (*index).min(timeline.tracks.len());
            timeline.tracks.insert(at, (**track).clone());
        }
        Undo::DeleteTrack { track_id } => {
            timeline
                .tracks
                .retain(|t| t.id.as_deref() != Some(track_id.as_str()));
        }
        Undo::RestoreTrackState { track_id, track } => {
            if let Some(slot) = track_by_id_mut(timeline, track_id) {
                let clips = std::mem::take(&mut slot.clips);
                *slot = (**track).clone();
                slot.clips = clips;
            }
        }
        Undo::RestoreMarkers { markers } => {
            timeline.markers = markers.clone();
        }
    }
}

pub(crate) fn track_by_id_mut<'a>(timeline: &'a mut Timeline, id: &str) -> Option<&'a mut Track> {
    timeline
        .tracks
        .iter_mut()
        .find(|t| t.id.as_deref() == Some(id))
}

pub(crate) fn remove_clip_anywhere(timeline: &mut Timeline, clip_id: &str) {
    for track in &mut timeline.tracks {
        track.clips.retain(|c| c.id.as_deref() != Some(clip_id));
    }
}

pub(crate) fn take_clip_anywhere(timeline: &mut Timeline, clip_id: &str) -> Option<Clip> {
    for track in &mut timeline.tracks {
        if let Some(index) = track
            .clips
            .iter()
            .position(|c| c.id.as_deref() == Some(clip_id))
        {
            return Some(track.clips.remove(index));
        }
    }
    None
}

pub(crate) fn sort_all(timeline: &mut Timeline) {
    for track in &mut timeline.tracks {
        track.clips.sort_by_key(|c| c.start_frame);
    }
}
