# Tasks: The `EditCommand` layer

**Feature**: 002-edit-commands | **Plan**: [plan.md](./plan.md)

Tests are mandatory (constitution principle V) and precede the code they cover.

## Phase 1: Foundational — the pure engines

- [X] T001 [P] Port `RippleEngine` to `crates/palmier-core/src/edit/ripple.rs` with `merge_ranges`, `shifts_for_ranges`, `shifts_for_removed`, `push`, `map_frame`, `ripple_markers`
- [X] T002 [P] Write `crates/palmier-core/tests/ripple.rs` covering adjacent-range merging, the overlapping-clip-does-not-shift rule, marker survival across per-track range lists, and floor-at-zero
- [X] T003 [P] Port `OverwriteEngine` to `crates/palmier-core/src/edit/overwrite.rs` with the four actions and the `trim * speed` source-continuity rule
- [X] T004 [P] Write `crates/palmier-core/tests/overwrite.rs` covering all four actions, region boundaries, and source continuity under non-unit speed
- [X] T005 Implement link-group resolution in `crates/palmier-core/src/edit/link.rs`: reverse index, group expansion, partner lookup, timing-propagation partners

## Phase 2: The command layer

- [X] T006 Define `EditCommand`, `Receipt`, and `RefusalReason` in `crates/palmier-core/src/edit/mod.rs`
- [X] T007 Implement the inverse patch in `crates/palmier-core/src/edit/patch.rs` — record prior clip state, prior positions, removed clips, prior markers
- [X] T008 Implement the journal in `crates/palmier-core/src/edit/journal.rs` — entries, cursor, undo, redo, redo-branch discard
- [X] T009 Implement placement in `crates/palmier-core/src/edit/place.rs` — clear region, drop at frame, sort, prune empty tracks
- [X] T010 Implement `apply` in four phases (resolve, validate, plan, commit) so a refusal cannot touch the project

## Phase 3: The commands

- [X] T011 [P] `AddClips`, `InsertClips`
- [X] T012 [P] `MoveClips` — link expansion, D1 whole-command refusal, D2 refuse-not-clamp
- [X] T013 [P] `RemoveClips`, `RippleDeleteRanges` — sync-locked tracks only, markers remapped
- [X] T014 [P] `SplitClips` — link propagation and re-grouping of right halves
- [X] T015 [P] `TrimClip`, `SetClipProperties`
- [X] T016 [P] `AddTrack`, `RemoveTrack`, `SetTrackProperties`, `LinkClips`, `UnlinkClips`

## Phase 4: Tests

- [X] T017 Write `crates/palmier-core/tests/commands.rs` — success, refusal, and no-op path for every command (SC-001)
- [X] T018 Write `crates/palmier-core/tests/edit_edge_cases.rs` — the spec's 13 edge cases
- [X] T019 Write `crates/palmier-core/tests/undo.rs` — undo/redo identity, refusals leave no entry, no-ops leave no entry, redo-branch discard
- [X] T020 Write the SC-002 property test: arbitrary command sequences, apply all, undo all, assert equality with the original
- [X] T021 Write the SC-003 assertion: after every refused command the project is unchanged
- [X] T022 Extend the no-panic property test to cover commands with hostile ids and boundary frames

## Phase 5: Close-out

- [X] T023 Verify SC-006 by searching for any mutation path outside `apply`
- [X] T024 Performance check: a ripple across 10,000 clips resolves under 50 ms
- [ ] T025 `cargo fmt --check`, `cargo clippy -D warnings`, full suite, then push and confirm CI green on Linux and Windows
