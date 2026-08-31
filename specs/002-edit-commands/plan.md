# Implementation Plan: The `EditCommand` layer

**Branch**: `002-edit-commands` | **Date**: 2026-08-30 | **Spec**: [spec.md](./spec.md)

## Summary

One enum of commands, one `apply` function, one journal. `RippleEngine` and
`OverwriteEngine` port from the original almost unchanged because they are already pure;
placement, linking, and undo are rebuilt around them to satisfy the constitution where
the original diverges — see the three decisions in [research.md](./research.md).

## Technical Context

**Language/Version**: Rust 1.97, edition 2024
**Primary Dependencies**: `palmier-core` (spec 001), `thiserror`. `proptest` for SC-002.
**Testing**: `cargo test`; property tests for the apply-then-undo identity.
**Target Platform**: Linux and Windows. macOS builds in CI but is not supported.
**Project Type**: Rust library — a new `edit` module inside `palmier-core`.
**Performance Goals**: A ripple over 10,000 clips resolves in under 50 ms.
**Constraints**: No panics. No mutation outside `apply`. `unsafe` denied.
**Scale/Scope**: ~20 command variants, 3 engines, roughly 2,000 lines plus tests.

## Constitution Check

| Principle | Status |
|---|---|
| I. One mutation path | **This feature is the principle.** One `apply`; `EditCommand` is the only way in |
| II. Undo is the command journal | Satisfied by Q1's inverse-patch design; explicitly *not* the original's snapshot swap |
| III. Format compatibility | Untouched — commands mutate the model spec 001 defined and add no fields |
| IV. Integer frames | Satisfied: every computation goes through `frames` and returns errors on overflow |
| V. Test-first, named failure cases | Satisfied: 13 edge cases, each a named test; SC-003 asserts no partial application |
| VI. Agent contracts | Satisfied ahead of need: the receipt is designed here, so spec 003's tools serialize it rather than invent one |
| VII. Layers ship whole | Satisfied: spec 001 works end to end; this is the next increment of L0 |

No violations. Complexity Tracking is empty.

**Worth flagging**: D1 and D2 in research.md are deliberate divergences *from the
reference implementation* in order to obey principles II and VI. They are behaviour
changes, documented as such, not port errors.

## Design

### Shape

```text
crates/palmier-core/src/edit/
├── mod.rs          EditCommand, apply(), Receipt
├── ripple.rs       ported RippleEngine — pure
├── overwrite.rs    ported OverwriteEngine — pure
├── link.rs         group expansion, partner resolution, offsets
├── place.rs        placement: clear region, drop, sort, prune
├── journal.rs      entries, inverse patches, undo/redo cursor
└── patch.rs        the inverse patch and its application
```

### The command set

Grouped by the layer-0 tool each will serve in spec 003.

| Command | Notes |
|---|---|
| `AddClips` | place new clips; overwrite semantics at the destination |
| `InsertClips` | ripple-push then place, opening a gap |
| `MoveClips` | multi-clip move; expands to link groups |
| `RemoveClips` | with and without ripple |
| `SplitClips` | at points; propagates to link partners and re-groups the right halves |
| `RippleDeleteRanges` | close ranges across sync-locked tracks and remap markers |
| `SetClipProperties` | the non-timing subset layer 0 needs |
| `TrimClip` | left/right edge with source-continuity |
| `AddTrack` / `RemoveTrack` / `SetTrackProperties` | track management |
| `LinkClips` / `UnlinkClips` | group stamping and clearing |

### Apply, in one shape

Every command runs the same four phases, and the phase boundaries are what make
atomicity real rather than aspirational:

1. **Resolve** — look up every id, expand link groups, reject unknown targets (D1).
2. **Validate** — frame bounds, track-type compatibility, overflow. Refuse as a whole.
3. **Plan** — compute shifts, overwrite actions, and the inverse patch, touching nothing.
4. **Commit** — apply the plan. This phase cannot fail.

A refusal in phases 1–3 leaves the project untouched by construction, because nothing
has been written yet. That is the property SC-003 asserts.

## Constitution Check — post-design

Unchanged: no violations. The four-phase shape strengthens principle II — the inverse
patch is produced in phase 3, so an entry cannot exist for a command that never
committed.

One risk carried into tasks: the inverse patch must cover *every* field a command
touches. A command that mutates something the patch does not record produces an undo
that silently loses data. SC-002's property test over generated sequences is the guard,
and it is worth more than any single hand-written undo test.
