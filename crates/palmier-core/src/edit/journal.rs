//! The command journal. Constitution principle II: undo *is* this, not a snapshot stack.

use super::patch::InversePatch;
use super::{EditCommand, Receipt};
use crate::timeline::Timeline;

/// One user intent that actually changed something.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntry {
    pub command: EditCommand,
    pub receipt: Receipt,
    pub patch: InversePatch,
}

/// Applied entries plus a cursor separating undo from redo.
///
/// Entries below the cursor have been applied; entries at or above it have been undone
/// and can be redone until a new command discards them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Journal {
    entries: Vec<JournalEntry>,
    cursor: usize,
}

impl Journal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an applied command, discarding any redo branch (FR-006).
    pub fn record(&mut self, entry: JournalEntry) {
        self.entries.truncate(self.cursor);
        self.entries.push(entry);
        self.cursor = self.entries.len();
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries that have been applied and not undone, oldest first.
    pub fn applied(&self) -> &[JournalEntry] {
        &self.entries[..self.cursor]
    }

    /// Step back one entry, returning the command that was undone.
    ///
    /// Crate-private for the same reason `InversePatch::apply` is: `EditSession::undo`
    /// is the only public entry point.
    pub(crate) fn undo(&mut self, timeline: &mut Timeline) -> Option<&JournalEntry> {
        if !self.can_undo() {
            return None;
        }
        self.cursor -= 1;
        let entry = &self.entries[self.cursor];
        entry.patch.apply(timeline);
        Some(entry)
    }

    /// The command at the cursor, for a caller that wants to reapply it.
    pub fn peek_redo(&self) -> Option<&JournalEntry> {
        self.entries.get(self.cursor)
    }

    /// Advance the cursor past a redone entry, replacing its patch with the fresh one.
    pub(crate) fn commit_redo(&mut self, patch: InversePatch) {
        if self.can_redo() {
            self.entries[self.cursor].patch = patch;
            self.cursor += 1;
        }
    }
}
