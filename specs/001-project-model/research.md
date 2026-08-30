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

## Still not audited

- Encoder behavior for every type other than `Transform`. All others are synthesized,
  which writes all non-optional keys and omits `nil` optionals, but this is inferred
  from the language rule rather than read from a custom implementation, because none
  exists to read.
---

# Addendum: T005–T007

Appended 2026-08-28 while executing tasks T005, T006, and T007. Completes the
"Not yet audited" section above, which is now empty.

## T005 — `TextStyle`'s three nested decoders

All three (`Outline`/border at line 87, `Background` at 134, `TextStyle` itself at
163) are **fully lenient**: every field is `(try? c.decode(...)) ?? default`, nothing
is required. `TextStyle`'s own decoder carries the comment *"Missing-key-tolerant
decode — older files pick up defaults for fields added later"*, which states the
format's intent directly.

### One field pair cannot be reproduced exactly

`isBold` and `isItalic`, when absent, do not fall back to a constant. They fall back
to **font introspection**:

```swift
let inferredTraits = Self.symbolicTraits(fontName: fontName, size: CGFloat(fontSize))
isBold:   (try? c.decode(Bool.self, forKey: .isBold))   ?? inferredTraits.contains(.traitBold),
isItalic: (try? c.decode(Bool.self, forKey: .isItalic)) ?? inferredTraits.contains(.traitItalic),
```

`symbolicTraits` constructs an `NSFont` and reads `CTFontGetSymbolicTraits`. The
result depends on **which fonts are installed on the machine**, so it is not a pure
function of the document and cannot be reproduced identically on Windows or Linux —
not merely because Core Text is absent, but because the same file can decode
differently on two Macs.

**Decision**: approximate with a font-name token heuristic (`Bold`, `Semibold`,
`Black`, `Italic`, `Oblique`, and the common PostScript suffixes), and document this
as the one corner of the format this project knowingly does not reproduce exactly. It
only applies to files predating the explicit `isBold`/`isItalic` keys; any file
carrying them decodes identically.

## T006 — synthesized `Codable` types

Twenty types with stored fields use synthesized `Codable`. The Swift rule applies
strictly: **`Optional` is optional, non-`Optional` is required, and a default value in
the declaration is NOT applied to a missing key.**

† marks a field that has a default in its declaration which decoding does *not* use.
Every † field is therefore **required in JSON** despite looking optional in the Swift
source — the single most common way to get this wrong.

| Type | File | Required | Optional |
|---|---|---|---|
| `EffectParam` | Effect.swift | — | `value`, `string`, `track` |
| `CurvePoint` | GradeCurve.swift | `x`, `y` | — |
| `GradeCurve` | GradeCurve.swift | `master`†, `red`†, `green`†, `blue`† | — |
| `HueCurves` | HueCurves.swift | `hueVsHue`†, `hueVsSat`†, `hueVsLum`† | — |
| `Keyframe` | Keyframe.swift | `frame`, `value`, `interpolationOut`† | — |
| `KeyframeTrack` | Keyframe.swift | `keyframes`† | — |
| `AnimPair` | Keyframe.swift | `a`, `b` | — |
| `MediaFolder` | MediaFolder.swift | `name` | `parentFolderId` |
| `MediaManifestEntry` | MediaManifest.swift | `name`, `type`, `source`, `duration` | `generationInput`, `sourceWidth`, `sourceHeight`, `sourceFPS`, `hasAudio`, `folderId`, `cachedRemoteURL`, `cachedRemoteURLExpiresAt`, `generationStatus`, `importInput` |
| `MediaImportInput` | MediaManifest.swift | — | `sourceURL`, `sourcePath`, `createdAt` |
| `GenerationInput` | MediaManifest.swift | `prompt`, `model`, `duration`, `aspectRatio` | `resolution`, `upscaleSettings`, `upscaleSourceWidth`, `upscaleSourceHeight`, `upscaleSourceFPS`, `quality`, `imageURLs`, `numImages`, `voice`, `lyrics`, `styleInstructions`, `instrumental`, `targetLanguage`, `multilingual`, `audioInput`, `generateAudio`, `draft`, `usesSourceVideo`, `referenceImageURLs`, `referenceVideoURLs`, `referenceAudioURLs`, `imageURLAssetIds`, `referenceImageAssetIds`, `referenceVideoAssetIds`, `referenceAudioAssetIds`, `createdAt`, `backendJobId`, `outputIndex`, `resultURLs`, `costCredits`, `refundedCredits` |
| `MulticamSource` | MulticamSource.swift | `offsetSeconds`†, `confidence`†, `locked`†, `id`†, `mediaRef`, `kind`, `angleLabel`, `sync`†, `id`†, `name`†, `members`†, `masterMemberId`† | — |
| `SyncMap` | MulticamSource.swift | `offsetSeconds`†, `confidence`†, `locked`† | — |
| `Member` | MulticamSource.swift | `id`†, `mediaRef`, `kind`, `angleLabel`, `sync`† | — |
| `ProjectFile` | ProjectFile.swift | `timelines` | `activeTimelineId`, `openTimelineIds`, `viewStates`, `speakers`, `multicamGroups` |
| `WordTiming` | TextAnimation.swift | `text`, `startFrame`, `endFrame` | — |
| `RGBA` | TextStyle.swift | `r`†, `g`†, `b`†, `a`† | — |
| `Shadow` | TextStyle.swift | `enabled`†, `color`†, `offsetX`†, `offsetY`†, `blur`† | — |
| `TimelineViewState` | Timeline.swift | `playheadFrame`†, `zoomScale`†, `scrollOffsetX`† | — |
| `Crop` | Timeline.swift | `left`†, `top`†, `right`†, `bottom`† | — |

`MulticamSource`'s row flattens its nested `SyncMap` and `Member` types; they are
listed separately as well.

### The finding that matters most

**`RGBA` requires all four of `r`, `g`, `b`, `a`** even though all four carry defaults
in the declaration. `Crop`, `Shadow`, `TimelineViewState`, `GradeCurve`, `HueCurves`,
and `KeyframeTrack` are the same shape.

Leniency composes downward, which softens the impact: a malformed `RGBA` inside a
`Background` fails that `RGBA` decode, which the enclosing `(try? ...)` catches, so
the whole `Background` becomes its default rather than failing the load. A malformed
`RGBA` inside a `TimelineMarker.color` — which is `try c.decode` — fails the load.
The same nested type produces opposite outcomes depending on its parent's strictness.

## T007 — explicit `null` versus absent key

**Indistinguishable everywhere in this format.** For a non-`Optional` field,
`try? c.decode(T.self, forKey:)` on an explicit `null` throws and falls to the default,
exactly as an absent key does. `decodeIfPresent` maps `null` to `nil` and then to the
same default. For `Optional` fields the result is `None` either way.

**Implication for the kernel**: treat `Value::Null` as an absent key at extraction
time. No field anywhere needs to distinguish the two.



---

# Addendum: blast radius

Found while implementing T018, when three tests written from the strictness table
failed and the implementation turned out to be right.

**Strictness is almost never observable as a load failure.** Nearly every type is
reached through a lenient parent, so the practical difference between the three levels
is *how much data a malformed value destroys*.

| Malformed value | Original construct | What is lost |
|---|---|---|
| `Clip.speed` | `(try?) ?? 1.0` | that field only; siblings survive |
| `Transform.width` | `decodeIfPresent`, reached via `(try? c.decode(Transform.self, ...)) ?? Transform()` | the **entire `Transform`** — `centerX` and every sibling revert to defaults |
| one `Clip` in a track | `(try? c.decode([Clip].self, ...)) ?? []` | **every clip on that track**, including the valid ones |
| one `Track` | `try c.decode` inside `Timeline` | the whole load fails |

The third row is real data loss in the original format: a single clip missing
`mediaRef` silently empties its track, and the load reports success. It is reproduced
faithfully, because the alternative is diverging from the format.

**Consequence for this project**: a successful decode is not evidence that the document
was well-formed. `validate()` exists for that, and callers are expected to run it —
which is also why the constitution says an MCP tool must never treat a success-shaped
response as proof that something happened.
