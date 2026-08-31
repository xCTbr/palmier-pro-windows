# Research: the edit engines

**Feature**: 002-edit-commands · **Date**: 2026-08-30 · **Resolves**: spec Q1, Q2, Q3

## Method

Read `Editor/RippleEngine.swift`, `Editor/OverwriteEngine.swift`,
`Editor/ViewModel/EditorViewModel+ClipMutations.swift`, `+Linking.swift`, and
`Editor/EditorUndo.swift`. Code is authoritative. Nothing was executed.

## The good news: two engines are already pure

`RippleEngine` and `OverwriteEngine` are free functions over plain values with no view,
no view-model, and no framework. They port almost line for line, and they are where the
subtle rules live.

### `RippleEngine`

- **`mergeRanges`** sorts by start and merges when `range.start <= last.end` — so
  *adjacent* ranges merge, not only overlapping ones.
- **`computeRippleShiftsForRanges`** shifts a clip by the summed length of every merged
  range that ends at or before the clip's start. A clip that **overlaps** a removed
  range does not shift at all. Only fully-later clips move.
- **`computeRipplePush`** moves every clip with `startFrame >= insertFrame` forward.
- **`mapFrame`** maps a frame through closing ranges: a range entirely before the frame
  subtracts its full length; a range straddling the frame subtracts `frame - range.start`,
  collapsing the frame onto the range's start. Result is floored at 0.
- **`rippleMarkers(closing:)`** takes **one range list per shifting track** and keeps a
  marker if it survives in **at least one** track's mapping, positioning it at the
  **minimum** surviving start. Point markers inside a removed range are dropped; range
  markers that collapse to zero length are dropped.

### `OverwriteEngine`

`computeOverwrite` clears `[regionStart, regionEnd)` with four actions — remove, trim
end, trim start, split. The source-continuity rule appears twice and is the answer to
FR-011:

```swift
newTrimStart = clip.trimStartFrame + Int((Double(trimAmount) * clip.speed).rounded())
```

Trimming from the left advances the source trim by the trimmed timeline length
**scaled by clip speed**, rounded. A split derives the right half's trim the same way.

## The divergences

Three places where the original's behaviour conflicts with this project's constitution.
Each is a deliberate decision, not an oversight in the port.

### D1 — `moveClips` silently skips invalid targets

The original iterates its move list and `continue`s past any clip it cannot find, any
out-of-range destination track, and any incompatible track type. It then applies
whatever survived. A caller asking to move five clips can have three moved and hear
nothing about the other two.

**Decision: refuse the whole command and name the offending target.** Constitution
principle II says validate-then-apply atomically, and principle VI forbids returning a
success-shaped response for an outcome that was silently adjusted. This resolves **Q2**:
"skipped" is not a legitimate partial outcome for an invalid target. It stays legitimate
only where the contract itself promises it — for example a no-op member inside an
otherwise valid batch.

### D2 — moves clamp to frame 0 instead of refusing

`moveClips` uses `max(0, m.toFrame)`, and `partnerMoves` clamps a linked partner the
same way. Clamping a partner is worse than clamping the lead: it **silently breaks the
link offset** the feature exists to preserve.

**Decision: refuse.** FR-010 already says so. A command that cannot be honoured as asked
is refused with a reason, never quietly turned into a different command.

### D3 — undo is snapshot-based in the original

`withTimelineSwap` captures the whole `Timeline` before and after and registers a swap;
`registerTimelineSwap(undoState:redoState:)` stores both. Grouping into one user-visible
action comes from `NSUndoManager`'s grouping in `EditorUndo`, not from the commands.

The constitution mandates a command journal instead. **This is the one part of the
feature that is not a port.** Resolution is Q1, below.

## Q1 — how the journal reverses a command

**Decision: each journal entry stores the command, its receipt, and an *inverse patch* —
the minimal set of prior values needed to restore what the command touched.**

A ripple delete's patch holds the removed clips and every shifted clip's prior start
frame; a move's patch holds one prior position. The patch is produced by the same apply
pass that computes the change, so it costs one pass, not two.

**Rationale**: it satisfies principle II literally — the journal *is* the undo, and an
entry is one intent — while staying honest that a multi-track ripple is not analytically
invertible from its parameters alone. Restoring is exact because the patch records
values rather than recomputing them.

**Alternatives rejected**:

- *Whole-`Timeline` snapshot per entry*, as the original does. Simple and exact, but it
  makes every entry O(project) in memory and turns the journal into a snapshot stack
  wearing a journal's name — the thing principle II exists to forbid.
- *Analytic inversion from command parameters only.* Purest, and genuinely impossible
  for commands whose effect depends on prior state: an overwrite that trimmed a
  neighbour cannot reconstruct that neighbour's original trim from the command alone.
- *Replay from a base snapshot.* Undo cost grows with session length; unusable.

## Q3 — what counts as a no-op

**Decision: a command is a no-op when applying it produces a project equal to the one
before it.** Determined by the apply pass reporting that it changed nothing, not by
inspecting the command's parameters beforehand.

This makes "move to the frame it already occupies" and "move that resolved to the same
frame" the same thing, which is the behaviour a user expects: nothing visibly happened,
so there is nothing to undo. A no-op produces a receipt saying so and no journal entry.

Refusals and no-ops stay distinct: a refusal means the command *could not* be honoured,
a no-op means it was honoured and changed nothing.

## Behaviour deliberately not ported

Present in the original, out of scope here because it serves its GUI rather than the
domain: debounced property commits, visual refresh and rebuild triggers, selection
state, drag previews, toast presentation, and multicam retime refusals (multicam is
layer 3).
