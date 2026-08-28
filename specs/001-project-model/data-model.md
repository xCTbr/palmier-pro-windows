# Data Model: `.palmier` project

**Feature**: 001-project-model | **Source of truth**: [research.md](./research.md)

Strictness column values: **R** required · **D** default on missing, error on wrong
type · **L** lenient, default on missing *or* wrong type. Every entity additionally
carries `extra` — unmatched JSON keys, preserved verbatim.

## ProjectFile

Root of `project.json`.

| Field | Type | Strictness | Default |
|---|---|---|---|
| `timelines` | `Vec<Timeline>` | R | — |
| `activeTimelineId` | `Option<String>` | L | `None` |
| `openTimelineIds` | `Option<Vec<String>>` | L | `None` |
| `viewStates` | `Option<Map<String, TimelineViewState>>` | L | `None` |
| `speakers` | `Option<Vec<SpeakerRegistryEntry>>` | L | `None` |
| `multicamGroups` | `Option<Vec<MulticamSource>>` | L | `None` |

**Legacy fallback**: if the document does not parse as `ProjectFile`, retry as a bare
`Timeline` and wrap it as a single-timeline project with that timeline active.
**Invariant**: a project with zero timelines is rejected.

## Timeline

| Field | Type | Strictness | Default |
|---|---|---|---|
| `id` | `String` | L | generated (see ids) |
| `name` | `String` | L | `"Timeline 1"` |
| `fps` | `i64` | **R** | — |
| `width` | `i64` | **R** | — |
| `height` | `i64` | **R** | — |
| `settingsConfigured` | `bool` | L | `false` |
| `folderId` | `Option<String>` | L | `None` |
| `tracks` | `Vec<Track>` | **R** | — |
| `markers` | `Vec<TimelineMarker>` | L | `[]` |

Derived, never persisted: `totalFrames` is the maximum `endFrame` across tracks;
`displayFrames` additionally accounts for markers extending past the last clip.

**Nesting**: a clip whose `sourceClipType` is `sequence` references another timeline
by id through `mediaRef`. Cycles must be detected rather than recursed into.

## Track

| Field | Type | Strictness | Default |
|---|---|---|---|
| `id` | `String` | L | generated |
| `type` | `ClipType` | **R** | — |
| `name` | `Option<String>` | L | `None` |
| `muted` | `bool` | L | `false` |
| `hidden` | `bool` | L | `false` |
| `syncLocked` | `bool` | L | **`true`** |
| `clips` | `Vec<Clip>` | L | `[]` |
| `displayHeight` | `f64` | L | default height, **clamped** to `[min, max]` |

`name` is normalized: rejected if it contains control characters or newlines, or
exceeds 80 characters — and rejection means the name becomes `None`, not a load error.

## Clip

Occupies `[startFrame, startFrame + durationFrames)` on its track.

| Field | Type | Strictness | Default |
|---|---|---|---|
| `id` | `String` | L | generated |
| `mediaRef` | `String` | **R** | — |
| `mediaType` | `ClipType` | L | `video` |
| `sourceClipType` | `ClipType` | L | `video` |
| `startFrame` | `i64` | **R** | — |
| `durationFrames` | `i64` | **R** | — |
| `trimStartFrame`, `trimEndFrame` | `i64` | L | `0` |
| `speed`, `volume`, `opacity` | `f64` | L | `1.0` |
| `fadeInFrames`, `fadeOutFrames` | `i64` | L | `0` |
| `fadeInInterpolation`, `fadeOutInterpolation` | `Interpolation` | L | `linear` |
| `transform` | `Transform` | L | default |
| `crop` | `Crop` | L | default |
| `edgeRounding`, `edgeSoftness` | `f64` | L | `0`, **coerced to 0 if outside `0..=1`** |
| `linkGroupId`, `captionGroupId`, `multicamGroupId` | `Option<String>` | L | `None` |
| `textContent`, `textStyle`, `textAnimation`, `wordTimings`, `textFillMode` | optional | L | `None` |
| `opacityTrack`, `positionTrack`, `scaleTrack`, `rotationTrack`, `cropTrack`, `volumeTrack` | `Option<KeyframeTrack<_>>` | L | `None` |
| `effects` | `Option<Vec<Effect>>` | L | `None` |
| `blendMode` | `Option<BlendMode>` | L | `None` |

Note the two different out-of-range policies in this table: `edgeRounding` coerces to
zero, `Track::displayHeight` clamps. They are not interchangeable.

## Transform

The only type with a custom **encoder**, and the only one carrying legacy keys.

| Field | Type | Strictness | Default |
|---|---|---|---|
| `centerX`, `centerY` | `f64` | D | `0.5`, or migrated (below) |
| `width`, `height` | `f64` | D | `1.0` |
| `rotation`, `rotationX`, `rotationY` | `f64` | D | `0.0` |
| `flipHorizontal`, `flipVertical` | `bool` | D | `false` |

**Legacy migration**: when `centerX` is absent and `x` is present,
`centerX = x + width - 0.5`; likewise `y` → `centerY`. Reproduce the formula exactly.
**Encoding** writes all nine modern keys and never emits `x` or `y`.

## TimelineMarker

The strictest type in the format: `id`, `name`, `startFrame`, `durationFrames`,
`color`, and `comment` are all **required**. Only `status` is defaulted (**D**,
`open`). A point marker has `durationFrames == 0`; a range marker spans
`[startFrame, endFrame)`.

## Effect

`type` is **R**. `id` is L (generated), `enabled` is L (`true`), `params` is L (`{}`),
a map of named typed parameters. Colour grades and curves are effects.

## KeyframeTrack

An ordered set of keyframes for one animatable property, with interpolation. Must
tolerate empty, single-point, out-of-order, and duplicate-frame inputs without
panicking; ordering is normalized on load.

## MediaManifest

Nothing required; all **D**. `version` → `1`, `entries` → `[]`, `folders` → `[]`.
Modeled and round-tripped only — no file on disk is resolved or validated in this
feature.

## Cross-entity invariants

Checked on load and surfaced as errors or diagnostics, never silently repaired:

- A project has at least one timeline.
- Clip, track, and timeline ids are unique within their scope.
- `durationFrames >= 0`; `startFrame + durationFrames` does not overflow `i64`.
- `speed` is finite. JSON cannot express `NaN` or `±Infinity`, so any such literal is
  a parse error rather than a poisoned value.
- Nested-timeline references resolve, and the nesting graph is acyclic.

## Identifier materialization

Decoding leaves an absent id as `None`. `materialize_ids()` runs at the loader
boundary and fills every empty id with a fresh UUID. Decoding therefore stays a pure
function and the round-trip comparison in SC-002 is deterministic.
