//! The single mutation path.
//!
//! Constitution principle I: every MCP tool call and every future mouse gesture becomes
//! an [`EditCommand`] applied by [`apply`]. Ripple, clamping, linking, placement, and
//! validation exist exactly once, here.
//!
//! `apply` runs four phases — resolve, validate, plan, commit. Only commit writes, so a
//! refusal leaves the project untouched by construction rather than by discipline.

pub mod journal;
pub mod link;
pub mod overwrite;
pub mod patch;
pub mod place;
pub mod ripple;

pub use journal::{Journal, JournalEntry};
pub use patch::InversePatch;

use std::collections::BTreeSet;

use crate::project::ProjectFile;
use crate::timeline::{Clip, ClipType, Timeline};

/// One user intent, as data. Carries nothing about who issued it.
#[derive(Debug, Clone, PartialEq)]
pub enum EditCommand {
    AddClips {
        track_id: String,
        clips: Vec<Clip>,
    },
    InsertClips {
        track_id: String,
        at_frame: i64,
        clips: Vec<Clip>,
    },
    MoveClips {
        moves: Vec<ClipMove>,
    },
    RemoveClips {
        clip_ids: Vec<String>,
        ripple: bool,
    },
    SplitClips {
        points: Vec<SplitPoint>,
    },
    RippleDeleteRanges {
        ranges: Vec<(i64, i64)>,
    },
    TrimClip {
        clip_id: String,
        edge: TrimEdge,
        delta_frames: i64,
    },
    SetClipProperties {
        clip_ids: Vec<String>,
        properties: ClipProperties,
    },
    AddTrack {
        track_type: ClipType,
        at_index: Option<usize>,
    },
    RemoveTrack {
        track_id: String,
    },
    SetTrackProperties {
        track_id: String,
        properties: TrackProperties,
    },
    LinkClips {
        clip_ids: Vec<String>,
    },
    UnlinkClips {
        clip_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipMove {
    pub clip_id: String,
    pub to_track_id: String,
    pub to_frame: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitPoint {
    pub track_id: String,
    pub at_frame: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEdge {
    Left,
    Right,
}

/// The non-timing clip properties layer 0 needs. `None` leaves a field alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClipProperties {
    pub opacity: Option<f64>,
    pub volume: Option<f64>,
    pub speed: Option<f64>,
    pub fade_in_frames: Option<i64>,
    pub fade_out_frames: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackProperties {
    pub name: Option<Option<String>>,
    pub muted: Option<bool>,
    pub hidden: Option<bool>,
    pub sync_locked: Option<bool>,
}

/// Why a command could not be honoured as asked.
///
/// A refusal always means nothing changed. It is never a partial outcome.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RefusalReason {
    #[error("no timeline is active")]
    NoActiveTimeline,
    #[error("unknown clip `{0}`")]
    UnknownClip(String),
    #[error("unknown track `{0}`")]
    UnknownTrack(String),
    #[error("clip `{clip}` is a {from_type} clip and cannot move to a {to_type} track")]
    IncompatibleTrackType {
        clip: String,
        from_type: &'static str,
        to_type: &'static str,
    },
    #[error("`{0}` would place content before frame 0")]
    NegativeFrame(String),
    #[error("`{0}` would exceed the representable frame range")]
    FrameOverflow(String),
    #[error("command names no targets")]
    EmptyTargets,
    #[error("clip `{0}` is a nested sequence and cannot be retimed")]
    SequenceNotRetimable(String),
    #[error("track `{0}` still holds clips")]
    TrackNotEmpty(String),
    #[error("{0}")]
    Invalid(String),
}

/// What applying a command actually did.
///
/// Designed here rather than in the tool layer so spec 003 serializes this instead of
/// inventing its own shape (constitution principle VI).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Receipt {
    pub created_clip_ids: Vec<String>,
    pub removed_clip_ids: Vec<String>,
    pub changed_clip_ids: Vec<String>,
    pub created_track_ids: Vec<String>,
    pub removed_track_ids: Vec<String>,
    pub changed_track_ids: Vec<String>,
    /// Targets the command legitimately did nothing to, with the reason.
    pub skipped: Vec<(String, String)>,
    pub markers_changed: bool,
}

impl Receipt {
    /// A command that changed nothing produces no journal entry (FR-004).
    pub fn is_no_op(&self) -> bool {
        self.created_clip_ids.is_empty()
            && self.removed_clip_ids.is_empty()
            && self.changed_clip_ids.is_empty()
            && self.created_track_ids.is_empty()
            && self.removed_track_ids.is_empty()
            && self.changed_track_ids.is_empty()
            && !self.markers_changed
    }

    fn skip(&mut self, target: impl Into<String>, why: impl Into<String>) {
        self.skipped.push((target.into(), why.into()));
    }

    /// One id per list, in first-touched order. A clip can be touched twice by one
    /// command — cleared then shifted — and the caller should hear about it once.
    fn dedupe(&mut self) {
        for list in [
            &mut self.created_clip_ids,
            &mut self.removed_clip_ids,
            &mut self.changed_clip_ids,
            &mut self.created_track_ids,
            &mut self.removed_track_ids,
            &mut self.changed_track_ids,
        ] {
            let mut seen = std::collections::HashSet::new();
            list.retain(|id| seen.insert(id.clone()));
        }
        // A clip that was created and then removed by the same command never existed
        // as far as the caller is concerned.
        let removed: std::collections::HashSet<String> =
            self.removed_clip_ids.iter().cloned().collect();
        self.changed_clip_ids.retain(|id| !removed.contains(id));
    }
}

/// The outcome of a plan phase: what to write, and how to reverse it.
pub(crate) struct Plan {
    pub receipt: Receipt,
    pub patch: InversePatch,
}

/// A project plus its journal. The only thing in the crate that mutates a project.
#[derive(Debug, Clone)]
pub struct EditSession {
    pub project: ProjectFile,
    pub journal: Journal,
}

impl EditSession {
    pub fn new(project: ProjectFile) -> Self {
        Self {
            project,
            journal: Journal::new(),
        }
    }

    fn active_timeline_index(&self) -> Result<usize, RefusalReason> {
        if self.project.timelines.is_empty() {
            return Err(RefusalReason::NoActiveTimeline);
        }
        match &self.project.active_timeline_id {
            Some(id) => self
                .project
                .timelines
                .iter()
                .position(|t| t.id.as_deref() == Some(id.as_str()))
                .ok_or_else(|| RefusalReason::UnknownTrack(id.clone())),
            None => Ok(0),
        }
    }

    /// Apply one command.
    ///
    /// Resolve, validate, and plan touch nothing; only commit writes. A refusal
    /// therefore leaves the project byte-identical, and a no-op leaves no journal entry.
    pub fn apply(&mut self, command: EditCommand) -> Result<Receipt, RefusalReason> {
        let index = self.active_timeline_index()?;
        let timeline = &mut self.project.timelines[index];

        let mut plan = plan_command(timeline, &command)?;
        plan.receipt.dedupe();
        let receipt = plan.receipt.clone();

        if receipt.is_no_op() {
            return Ok(receipt);
        }
        self.journal.record(JournalEntry {
            command,
            receipt: receipt.clone(),
            patch: plan.patch,
        });
        Ok(receipt)
    }

    /// Step back one journal entry.
    pub fn undo(&mut self) -> Option<Receipt> {
        let index = self.active_timeline_index().ok()?;
        let timeline = &mut self.project.timelines[index];
        self.journal.undo(timeline).map(|e| e.receipt.clone())
    }

    /// Reapply the entry the cursor sits on.
    pub fn redo(&mut self) -> Option<Receipt> {
        let index = self.active_timeline_index().ok()?;
        let command = self.journal.peek_redo()?.command.clone();
        let timeline = &mut self.project.timelines[index];
        let mut plan = plan_command(timeline, &command).ok()?;
        plan.receipt.dedupe();
        let receipt = plan.receipt.clone();
        self.journal.commit_redo(plan.patch);
        Some(receipt)
    }
}

/// Resolve, validate, plan, and commit one command against one timeline.
pub(crate) fn plan_command(
    timeline: &mut Timeline,
    command: &EditCommand,
) -> Result<Plan, RefusalReason> {
    match command {
        EditCommand::AddClips { track_id, clips } => {
            place::add_clips(timeline, track_id, clips, None)
        }
        EditCommand::InsertClips {
            track_id,
            at_frame,
            clips,
        } => place::add_clips(timeline, track_id, clips, Some(*at_frame)),
        EditCommand::MoveClips { moves } => place::move_clips(timeline, moves),
        EditCommand::RemoveClips { clip_ids, ripple } => {
            place::remove_clips(timeline, clip_ids, *ripple)
        }
        EditCommand::SplitClips { points } => place::split_clips(timeline, points),
        EditCommand::RippleDeleteRanges { ranges } => place::ripple_delete(timeline, ranges),
        EditCommand::TrimClip {
            clip_id,
            edge,
            delta_frames,
        } => place::trim_clip(timeline, clip_id, *edge, *delta_frames),
        EditCommand::SetClipProperties {
            clip_ids,
            properties,
        } => place::set_clip_properties(timeline, clip_ids, properties),
        EditCommand::AddTrack {
            track_type,
            at_index,
        } => place::add_track(timeline, *track_type, *at_index),
        EditCommand::RemoveTrack { track_id } => place::remove_track(timeline, track_id),
        EditCommand::SetTrackProperties {
            track_id,
            properties,
        } => place::set_track_properties(timeline, track_id, properties),
        EditCommand::LinkClips { clip_ids } => place::link_clips(timeline, clip_ids, true),
        EditCommand::UnlinkClips { clip_ids } => place::link_clips(timeline, clip_ids, false),
    }
}

/// Targets, expanded to whole link groups, in a stable order (FR-007).
pub(crate) fn resolve_targets(
    timeline: &Timeline,
    ids: &[String],
    expand_links: bool,
) -> Result<BTreeSet<String>, RefusalReason> {
    if ids.is_empty() {
        return Err(RefusalReason::EmptyTargets);
    }
    let known: BTreeSet<String> = timeline
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .filter_map(|c| c.id.clone())
        .collect();
    for id in ids {
        if !known.contains(id) {
            return Err(RefusalReason::UnknownClip(id.clone()));
        }
    }
    let requested: BTreeSet<String> = ids.iter().cloned().collect();
    Ok(if expand_links {
        link::expand_to_link_groups(timeline, &requested)
    } else {
        requested
    })
}
