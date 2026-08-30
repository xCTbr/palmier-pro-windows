# Feature Specification: The `EditCommand` layer

**Feature Branch**: `002-edit-commands`

**Created**: 2026-08-30

**Status**: Draft

**Layer**: L0 — MCP daemon

**Input**: The single mutation path for the project model. Every MCP tool call and every
future mouse gesture produces an `EditCommand` applied by one function, so ripple,
clamping, linking, placement, and validation exist exactly once.

## Why this exists

This is constitutional principle I, and it is the decision the whole project rests on.
The reason to build it now — before any tool, before any interface — is that a second
mutation path is nearly free to add later and nearly impossible to remove. Layer 2's
manual editing is only additive if it reaches the same commands the agent already uses.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Arranging clips on a timeline (Priority: P1)

Someone building an edit adds clips to a track, moves them, splits one at a point, and
removes another. Each of those is one intent, produces one journal entry, and either
happens completely or does not happen at all.

**Why this priority**: Placement is the smallest set of operations that makes a
timeline editable at all. Everything else in layer 0 composes from it.

**Independent Test**: Apply a sequence of commands to a project and assert the
resulting timeline. Delivers value alone: the project becomes mutable through a single
audited path.

**Acceptance Scenarios**:

1. **Given** an empty video track, **When** a clip is added at frame 100, **Then** it occupies `[100, 100 + duration)` and the journal has exactly one entry.
2. **Given** a clip at `[0, 150)`, **When** another is added overlapping it, **Then** the overlap resolves by the timeline's placement rule and the result is reported, never silently different from what was asked.
3. **Given** a clip at `[0, 150)`, **When** it is split at frame 60, **Then** two clips exist covering `[0, 60)` and `[60, 150)` with source trims adjusted so no frame of source media is lost or repeated.
4. **Given** a clip, **When** a split is requested at its first or last frame, **Then** the command is a no-op, no clip is created, and nothing is journaled.
5. **Given** a move that would place a clip at a negative frame, **When** it is applied, **Then** the command is refused with a reason and the timeline is unchanged.

---

### User Story 2 — Ripple edits that close the gap (Priority: P1)

Removing a range of the timeline pulls everything after it earlier, on every track that
participates, keeping the edit in sync rather than leaving a hole.

**Why this priority**: Ripple is the operation most likely to corrupt a timeline
subtly, and the one where "it looked right" is least trustworthy. It is also the
behaviour with the most intricate rules in the original.

**Independent Test**: Apply a ripple delete over a known arrangement and assert every
clip's resulting position, including on tracks that were not directly targeted.

**Acceptance Scenarios**:

1. **Given** clips at `[0,30)`, `[30,60)`, `[60,90)`, **When** the middle is ripple-deleted, **Then** the third clip moves to `[30,60)` and no gap remains.
2. **Given** a sync-locked track with no clip in the removed range, **When** a ripple delete runs, **Then** that track's later clips shift by the same amount, so the tracks stay aligned.
3. **Given** a track that is not sync-locked, **When** a ripple delete runs, **Then** that track does not shift.
4. **Given** markers positioned after a rippled range, **When** the ripple is applied, **Then** the markers shift with the content they annotate.
5. **Given** two disjoint ranges removed in one command, **When** it is applied, **Then** shifts accumulate correctly and the result equals removing them one at a time in descending order.
6. **Given** a ripple that would move a clip before frame 0, **When** it is applied, **Then** the command is refused and nothing moves.

---

### User Story 3 — Linked video and audio stay together (Priority: P1)

A video clip and its linked audio behave as one thing: moving, removing, or trimming
either moves, removes, or trims both, preserving their relative offset.

**Why this priority**: Link breakage is silent and only discovered when the edit is
watched. It must be correct from the first command, not retrofitted.

**Independent Test**: Link a video and audio clip, operate on one, and assert the
partner followed with its offset intact.

**Acceptance Scenarios**:

1. **Given** a linked video and audio pair, **When** the video is moved by 40 frames, **Then** the audio moves 40 frames and their relative offset is unchanged.
2. **Given** a linked pair, **When** one is removed, **Then** both are removed in the same journal entry.
3. **Given** a linked pair, **When** they are explicitly unlinked and one is moved, **Then** the other does not move.
4. **Given** a selection naming only one member of a link group, **When** a command is applied, **Then** it expands to the whole group before validation, so the refusal-or-apply decision covers every affected clip.

---

### User Story 4 — Undo that matches what the user did (Priority: P1)

Undo steps back exactly one thing the user meant to do. A command that failed, was
refused, or changed nothing leaves no trace to step back through.

**Why this priority**: Constitutional principle II. An undo stack that contains
half-operations or empty entries is worse than no undo, because it destroys trust in
the timeline.

**Independent Test**: Apply a sequence including failures and no-ops, undo everything,
and assert the project is byte-identical to its starting state.

**Acceptance Scenarios**:

1. **Given** a project, **When** three commands are applied and undone three times, **Then** the project equals its original state exactly.
2. **Given** a command that is refused, **When** undo is invoked, **Then** it steps past the refusal to the previous real change.
3. **Given** a command that changes nothing, **When** it is applied, **Then** no journal entry is created.
4. **Given** an undone command, **When** redo is invoked, **Then** the change is reapplied identically.
5. **Given** an undone command, **When** a new command is applied instead of redo, **Then** the redo branch is discarded.
6. **Given** a ripple delete spanning several tracks, **When** it is undone, **Then** every affected clip and marker returns to its exact prior position.

---

### Edge Cases

Each gets a test. This list is the definition of done for the negative path.

- A command naming a clip, track, or timeline id that does not exist.
- A command with an empty target set.
- A move to the track a clip already occupies at the frame it already starts at — a no-op.
- Placement at frame 0, and at a frame beyond the end of all content.
- A split at a clip boundary, outside the clip, and on a zero-duration clip.
- Removing every clip on a track, and removing a track that still has clips.
- A ripple range that covers the entire timeline, and one that covers nothing.
- Overlapping ranges in a single ripple command.
- A link group whose partner is on a removed track.
- A link group that includes a clip on a hidden or muted track.
- Commands touching a nested-sequence clip, which cannot be retimed.
- Undo invoked on an empty journal, and redo invoked with nothing to redo.
- A command that would exceed `i64` frame bounds.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST expose exactly one function that mutates a project, and every mutation MUST pass through it.
- **FR-002**: The system MUST resolve and validate a whole command before applying any part of it. A partially applied command is a defect, not a partial success.
- **FR-003**: The system MUST return a receipt naming what changed, what was skipped, and what was refused, with a reason for each refusal.
- **FR-004**: The system MUST record one journal entry per user intent, and MUST record nothing for a command that fails, is refused, or changes nothing.
- **FR-005**: The system MUST support undo and redo over the journal, restoring the project to its exact prior state.
- **FR-006**: The system MUST discard the redo branch when a new command is applied after an undo.
- **FR-007**: The system MUST expand a target set to its complete link groups before validating, so a command covers every clip it will affect.
- **FR-008**: The system MUST apply ripple shifts to sync-locked tracks and MUST NOT shift tracks that are not sync-locked.
- **FR-009**: The system MUST shift markers along with the content they annotate when a ripple changes that content's position.
- **FR-010**: The system MUST refuse any command that would place content at a negative frame or exceed `i64` bounds, rather than clamping silently.
- **FR-011**: The system MUST preserve source-media continuity across a split: no source frame is lost or duplicated.
- **FR-012**: The system MUST never mutate a project as a side effect of reading it.

### Out of scope

Named explicitly so the boundary is testable:

- MCP, HTTP, and any tool or wire surface. That is spec 003.
- Rendering, export, and media probing. That is spec 004.
- Keyframes, colour grading, effects, text content and styling, captions, and transcript-driven edits.
- Multicam, nesting operations beyond respecting that a sequence clip cannot be retimed.
- Media import and the manifest's relationship to files on disk.
- Persistence: this feature mutates an in-memory project. Saving is the caller's job.

### Key Entities

- **EditCommand**: One user intent, expressed as data. Carries everything needed to validate and apply it, and nothing about who issued it.
- **Receipt**: The structured outcome of applying a command — what changed, what was skipped, what was refused and why.
- **Journal**: The ordered record of applied commands and the state needed to reverse them, with a cursor separating undo from redo.
- **Link group**: A set of clips that move, trim, and delete as one, preserving relative offsets.
- **Ripple plan**: The computed set of shifts a ripple produces across tracks and markers, resolved before anything moves.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Every command in the enum has a test for its success path, its refusal path, and its no-op path.
- **SC-002**: Applying any sequence of commands then undoing all of them returns the project to a state equal to the original, verified by property test over generated command sequences.
- **SC-003**: No command, on any input, leaves the project partially modified. Verified by asserting the project is unchanged after every refused command.
- **SC-004**: No command panics on any input, including hostile ids, empty sets, and boundary frames.
- **SC-005**: Ripple behaviour matches the audited original for every case named in the edge-case list, each traceable to the Swift source it was ported from.
- **SC-006**: A search of the codebase finds no mutation of a project outside the single apply function.
- **SC-007**: Tests pass on Linux and Windows in CI; `clippy` and `fmt` clean.

## Assumptions

- The Swift edit engines in `palmier-macos-codebase/` are the authoritative description of ripple, overwrite, and linking behaviour, audited the way spec 001 audited the decoders. Where code and comments disagree, the code wins.
- Behaviour that exists in the original only to serve its GUI — debounced commits, visual refresh, selection state, drag previews — is out of scope. This feature ports the domain rules those wrap.
- Commands operate on one timeline at a time. Cross-timeline operations are a later concern.
- The project is held in memory by the caller; this feature neither reads nor writes disk.

## Open questions

- **Q1**: The original's undo is **snapshot-based** — `registerTimelineSwap(undoState:redoState:)` stores whole `Timeline` values and swaps them. The constitution mandates a command journal instead. A journal needs each command to be reversible, which is straightforward for a move and intricate for a multi-track ripple delete. Resolve during planning: store an inverse patch per entry, store a bounded snapshot per entry, or make every command analytically invertible. This is the single largest design decision in the feature.
- **Q2**: When a command targets several clips and some are valid while others are not, does it refuse wholly or apply the valid subset and report the rest as skipped? FR-002 demands atomicity per command; the question is whether "skipped" is a legitimate partial outcome or a disguised partial application. Resolve by auditing what the original's tools promise.
- **Q3**: What exactly makes a command a no-op versus a change? A move to the same frame is clearly a no-op; a move that is clamped to the same frame is less clear. The answer determines what lands in the journal, so it must be defined before the journal is built.
