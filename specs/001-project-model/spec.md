# Feature Specification: Project model and `.palmier` round-trip fidelity

**Feature Branch**: `001-project-model`

**Created**: 2026-08-26

**Status**: Draft

**Layer**: L0 — MCP daemon

**Input**: First increment of layer 0. Everything else in the project — edit commands,
tools, rendering, and eventually the GUI — operates on the types this feature defines.
Correctness here has an external ground truth: the Swift decoders in
`palmier-macos-codebase/Sources/PalmierPro/Models/`.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — A macOS project opens intact (Priority: P1)

Someone edited a project in Palmier Pro on a Mac and copies the `.palmier` folder to
Windows. Every timeline, track, clip, marker, keyframe track, and effect is read
exactly as the Mac wrote it, including fields this project does not yet understand.

**Why this priority**: Format compatibility is a constitutional principle and the
single strategic reason to build on this codebase. If it is not true from the first
commit, it never becomes true — every later feature would need retrofitting.

**Independent Test**: Load a corpus of `project.json` fixtures covering every model
type and assert the parsed value against expected structures. Delivers value on its
own: the project can read real user data.

**Acceptance Scenarios**:

1. **Given** a `project.json` written by Palmier Pro with multiple timelines, **When** it is loaded, **Then** every timeline, track, clip, and marker is present with the values the file specifies.
2. **Given** a file containing a field this project does not model, **When** it is loaded and saved again, **Then** the unknown field is present in the output with its original value.
3. **Given** a legacy file that is a bare `Timeline` rather than a `ProjectFile`, **When** it is loaded, **Then** it is wrapped into a single-timeline project, matching `ProjectFile.decode`'s fallback.
4. **Given** a file missing an optional key, **When** it is loaded, **Then** the field takes the same default the Swift model uses.
5. **Given** a file whose required keys are absent or of the wrong type, **When** it is loaded, **Then** loading fails with an error naming the offending path, and no partial project is returned.

---

### User Story 2 — A project written here opens on macOS (Priority: P1)

A project created or modified by this application is opened in Palmier Pro on a Mac
and is read without error or data loss.

**Why this priority**: Compatibility that only works in one direction is not
compatibility. This is the half that is easy to get silently wrong, because nothing
in this repository can execute the Swift decoder to catch it.

**Independent Test**: Encode each fixture and assert the output JSON satisfies the
Swift decoders' documented requirements — every key the Swift side requires is
present, every value is in the type it expects.

**Acceptance Scenarios**:

1. **Given** any fixture, **When** it is decoded and re-encoded, **Then** the output is semantically equal to the input: same keys, same values, ordering and whitespace excepted.
2. **Given** a project constructed in memory, **When** it is encoded, **Then** every key the Swift decoder requires is present and no key carries a type the Swift decoder rejects.
3. **Given** a clip with no animation, **When** it is encoded, **Then** absent optional fields are omitted rather than written as null.

---

### User Story 3 — Inspecting a project from the terminal (Priority: P2)

A developer or an agent points the binary at a project folder and gets a readable
summary of what is inside it.

**Why this priority**: Makes the feature demonstrable end to end rather than only
through tests, and becomes the first debugging tool for every later feature. Lower
than P1 because compatibility is correct or not regardless of whether this exists.

**Independent Test**: Run the command against a fixture project and assert the
summary reports the correct timeline count, durations, and track composition.

**Acceptance Scenarios**:

1. **Given** a valid project folder, **When** `palmier inspect <path>` runs, **Then** it prints each timeline with its fps, resolution, duration in frames and timecode, and per-track clip counts, and exits 0.
2. **Given** a path that is not a project, **When** the command runs, **Then** it prints an error naming what was wrong and exits non-zero.

---

### Edge Cases

Each of these gets a test. This list is the definition of done for the negative path.

- Empty timeline: no tracks, and tracks with no clips.
- A project with zero timelines — the Swift decoder rejects this; so must this one.
- Clip at frame 0, and a clip whose `startFrame + durationFrames` is `i64::MAX`.
- Zero-duration and negative-duration clips.
- `speed` of 0, negative, and non-finite (`NaN`, `±Infinity`) — JSON permits none of the last three, so parsing must reject them rather than produce a poisoned value.
- Duplicate clip, track, or timeline IDs within one project.
- A clip referencing a `mediaRef` that no manifest entry describes.
- A nested-timeline clip pointing at a timeline ID that does not exist, and a cycle of nested timelines.
- Keyframe tracks that are empty, single-point, out of order, or have duplicate frames.
- Unicode, emoji, and embedded newlines in timeline names, track names, and text content.
- A file that is empty, truncated mid-object, or not JSON at all.
- A file large enough to matter — 10,000 clips across 20 tracks — loads without stack overflow or quadratic behavior.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST model every persisted type in the original's `Models/` directory with its exact JSON key names.
- **FR-002**: The system MUST apply the same default value as the Swift model for every key absent from the input, and MUST reject input missing a key the Swift model requires.
- **FR-003**: The system MUST preserve fields it does not model through a decode/encode round trip, at every level of nesting.
- **FR-004**: The system MUST accept the legacy bare-`Timeline` document shape and wrap it into a single-timeline project.
- **FR-005**: The system MUST omit absent optional fields on encode rather than emitting null.
- **FR-006**: The system MUST represent all frame quantities as `i64` and expose clip ranges as half-open `[start, end)`.
- **FR-007**: The system MUST validate before arithmetic on frame values, returning an error rather than panicking or overflowing on any input.
- **FR-008**: The system MUST report load failures with the JSON path of the offending value and MUST NOT return a partially constructed project.
- **FR-009**: The system MUST provide `palmier inspect <path>` printing a timeline summary, exiting non-zero with a diagnostic on failure.
- **FR-010**: The system MUST perform all filesystem access outside the model types themselves, so the model can be tested without touching disk.

### Out of scope

Named explicitly so the boundary is testable:

- Mutating a project. `EditCommand` is spec 002.
- MCP, HTTP, or any tool surface.
- Media decoding, probing, rendering, or thumbnails.
- The media manifest's relationship to files on disk — the manifest is modeled and round-tripped, but no file is resolved or validated.
- Writing into a live `.palmier` package, staging, atomic replacement, or save coordination.

### Key Entities

- **ProjectFile**: Root of `project.json`. Holds timelines, the active and open timeline IDs, per-timeline view state, speakers, and multicam groups.
- **Timeline**: A sequence — fps, resolution, tracks, markers. Identified by a stable string ID and referenceable as nested media by another timeline's clip.
- **Track**: An ordered lane of clips of one type, with mute, hide, lock, and display state.
- **Clip**: A placement of media on a track over `[startFrame, startFrame + durationFrames)`, carrying trim, speed, volume, fades, opacity, transform, crop, link and group IDs, optional text content, optional per-property keyframe tracks, effects, and blend mode.
- **KeyframeTrack**: Animation for one property — an ordered set of keyframes with interpolation.
- **Effect**: A named effect with typed parameters, including color grades and curves.
- **TimelineMarker**: A review note at a point or over a range.
- **MediaManifest**: The project's record of its media assets and folders.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Every type in the original's `Models/` directory that participates in persistence is modeled, verified by an audit checklist naming each type and the test that covers it.
- **SC-002**: 100% of fixtures decode, re-encode, and compare semantically equal to their input.
- **SC-003**: Every edge case listed above has a test that asserts the specified behavior; the negative-path tests fail if the corresponding guard is removed.
- **SC-004**: No input — valid, malformed, hostile, or truncated — causes a panic. Verified by a fuzz or property test over arbitrary JSON and arbitrary field values.
- **SC-005**: A 10,000-clip project loads in under 500 ms on the development machine.
- **SC-006**: `cargo clippy --workspace --all-targets` and `cargo fmt --check` pass, and `cargo test` passes on Linux, Windows, and macOS in CI.

## Assumptions

- The vendored Swift source in `palmier-macos-codebase/` is the authoritative specification of the format. Where its behavior and its comments disagree, the code wins.
- Fixtures must be hand-authored from reading the Swift decoders, because Palmier Pro cannot be executed in this environment to generate them. Fixture fidelity is therefore a review concern, not something a test can prove on its own.
- The upstream format will keep changing. Drift is detected by diffing `macos-reference` against `upstream/main` under `Sources/PalmierPro/Models/`, not by any runtime check.
- `project.json` is the only file this feature reads. Media files, caches, and thumbnails inside the package are out of scope.

## Open questions

- **Q1**: Which `Clip` keys does Swift's synthesized `Decodable` actually require versus default? Swift does not apply property defaults to missing keys in synthesized decoding, so several fields that look optional may be mandatory. Resolve by auditing each type's decoder before implementing, not by assuming.
- **Q2**: Does any type rely on a custom `init(from:)` with `try?` fallbacks like `Timeline` does? Each such type has looser decoding than its declaration suggests and must be matched exactly.
- **Q3**: How is unknown-field preservation represented without polluting every struct? Decide during planning; the requirement is behavioral, not structural.
