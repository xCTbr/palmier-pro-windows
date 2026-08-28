# Research: decoding contract of the `.palmier` format

**Feature**: 001-project-model
**Date**: 2026-08-28
**Resolves**: spec.md Q1, Q2

## Method

Read every `Codable` conformance under
`palmier-macos-codebase/Sources/PalmierPro/Models/`. The code is authoritative;
declarations and comments are not. Swift cannot be executed in this environment, so
nothing here was confirmed at runtime — this is a code audit, and fixture fidelity
remains a review concern.

## Headline finding

**Q1 is answered, and against the initial assumption.** The hypothesis was that
Swift's synthesized `Decodable` would make most `Clip` fields mandatory, because
Swift does not apply a property's default value to a missing key.

That never applies here: **`Clip` has a hand-written decoder** in an `extension`
(`Models/Timeline.swift:500`), invisible to a grep of the struct body. So do `Track`,
`Timeline`, `Transform`, `Effect`, `TextAnimation`, `TimelineMarker`, `MediaManifest`,
and three nested types in `TextStyle` — 11 custom decoders in total.

Only **three** `Clip` keys are required: `mediaRef`, `startFrame`, `durationFrames`.

## The contract has three strictness levels

Which one a field uses is the single most important fact about it, and it varies
*within* the same type.

| Construct | Missing key | Wrong type | Used by |
|---|---|---|---|
| `try c.decode(...)` | **throws** | **throws** | required fields |
| `try c.decodeIfPresent(...) ?? d` | default | **throws** | `Transform`, `MediaManifest`, `TimelineMarker.status` |
| `(try? c.decode(...)) ?? d` | default | **default, silently** | `Clip`, `Track`, `Timeline`, `Effect`, `TextAnimation` |

The third row is the trap. `speed: "fast"` in a `Clip` does not fail — it silently
becomes `1.0`. The same malformed value inside a `Transform` throws. Any Rust
implementation that treats "has a default" as one concept will diverge from the
original on malformed input.

## Per-type audit

### `Clip` — `Models/Timeline.swift:500`

- **Required**: `mediaRef`, `startFrame`, `durationFrames`
- **`try?` + default**: every other key. `mediaType`/`sourceClipType` → `.video`,
  `speed`/`volume`/`opacity` → `1.0`, all frame fields → `0`, interpolations →
  `.linear`, `transform`/`crop` → default-constructed, all optionals → `nil`
- **`id` missing → a freshly generated UUID.** Decoding is not deterministic.
- **`edgeRounding` / `edgeSoftness`**: decoded as `Double` defaulting to `0`, then
  **any value outside `0...1` becomes `0`** — coerced, not clamped. `1.5` → `0`.

### `Track` — `Models/Timeline.swift:146`

- **Required**: `type`
- `id` missing → fresh UUID; `clips` → `[]`; `muted`/`hidden` → `false`
- **`syncLocked` defaults to `true`**, not `false`
- `name` runs through `TrackName.normalized`, which rejects control characters,
  newlines, and names over 80 characters — wrapped in `try?`, so **an invalid name is
  silently dropped to `nil`** rather than failing the load
- **`displayHeight` is clamped** to `[minHeight, maxHeight]` — a real clamp, unlike
  `edgeRounding`'s coercion. Two different out-of-range policies coexist in this format.

### `Timeline` — `Models/Timeline.swift:75`

- **Required**: `fps`, `width`, `height`, `tracks`
- `id` → fresh UUID, `name` → `"Timeline 1"`, `settingsConfigured` → `false`,
  `markers` → `[]`, `folderId` → `nil`

### `Transform` — `Models/Timeline.swift:612`, and the **only custom encoder**

- Strict: uses `decodeIfPresent`, so a wrong type throws
- **Legacy migration**: when `centerX` is absent but `x` is present,
  `centerX = x + width - 0.5`. Same for `y`/`centerY`. Replicate the formula exactly —
  it is the format's history, not a candidate for correction.
- The encoder writes all nine modern keys and **never** emits the legacy `x`/`y`

### `TimelineMarker` — strict

Everything required — `id`, `name`, `startFrame`, `durationFrames`, `color`,
`comment` — except `status`, which is `decodeIfPresent` defaulting to `.open`. The
strictest type in the format.

### `Effect`

`type` required. `id` → fresh UUID, `enabled` → `true`, `params` → `[:]`, all `try?`.

### `TextAnimation`

Nothing required. `preset` → `.none`, `perWordFrames` → `6`, `highlight` → `nil`.

### `MediaManifest`

Nothing required, all `decodeIfPresent`. `version` → `1`, `entries` → `[]`,
`folders` → `[]`.

### `ProjectFile` — `Models/ProjectFile.swift`

Decodes as `ProjectFile`; on any failure retries as a bare `Timeline` and wraps it
into a single-timeline project. A decoded project with zero timelines is rejected.

## Implications for the Rust implementation

1. **`#[serde(default)]` is not enough.** It covers a missing key only. Replicating
   `try?` needs a forgiving deserializer that also swallows type errors and falls back.
   This must be a deliberate, named helper applied per field, not a blanket policy —
   the strictness level is part of the format contract.
2. **`id` materialization breaks round-trip determinism.** A file without `id` decodes
   to a different value each run, so a naive decode/encode/compare test is flaky.
   Decide in planning whether to match the original's behavior and exclude generated
   ids from equality, or record that the id was absent.
3. **Two out-of-range policies.** `edgeRounding`/`edgeSoftness` coerce to `0`;
   `displayHeight` clamps to a range. Do not unify them.
4. **Unknown-field preservation interacts with custom decoders.** Every type above
   needs the capture, including those whose decoders ignore unrecognized keys today.
5. **`try?` also swallows `null`.** Confirm during implementation whether an explicit
   `null` and an absent key are distinguishable anywhere that matters.

## Not yet audited

- The three nested decoders in `Models/TextStyle.swift` (lines 87, 134, 163).
- The roughly 33 types that use synthesized `Codable`. These follow the standard rule
  — `Optional` is optional, non-`Optional` is required, no default applied — but each
  needs a field-by-field pass before it is called done.
- Encoder behavior for every type other than `Transform`. All others are synthesized,
  which writes all non-optional keys and omits `nil` optionals, but this was inferred
  from the language rule rather than read.
