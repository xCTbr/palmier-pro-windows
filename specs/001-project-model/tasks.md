# Tasks: Project model and `.palmier` round-trip fidelity

**Feature**: 001-project-model | **Plan**: [plan.md](./plan.md)

**Tests are mandatory here.** Constitution principle V is NON-NEGOTIABLE: a task is
not started until a failing test proves the gap. Test tasks are not optional in this
feature and are ordered before the implementation they cover.

## Format: `[ID] [P?] [Story] Description`

`[P]` = parallelizable (different file, no dependency on an incomplete task).

## Path Conventions

Library code in `crates/palmier-core/src/`, tests in `crates/palmier-core/tests/`,
binary in `crates/palmier/src/`. Reference source is read-only at
`palmier-macos-codebase/Sources/PalmierPro/Models/`.

---

## Phase 1: Setup (Shared Infrastructure)

- [X] T001 Add `uuid` (v4), `thiserror`, `serde_json` with `preserve_order`, and dev-dependencies `insta` and `proptest` to `crates/palmier-core/Cargo.toml`
- [X] T002 [P] Create the module skeleton declared in plan.md — `codec/`, `frames`, `project`, `timeline`, `transform`, `keyframe`, `text`, `effect`, `marker`, `media`, `ids` — as empty modules wired into `crates/palmier-core/src/lib.rs`
- [X] T003 [P] Create `crates/palmier-core/tests/fixtures/` with a README stating that fixtures are hand-authored from the Swift decoders and that fixture fidelity is a review concern, not something a test proves
- [X] T004 Verify `cargo build` and `cargo clippy --workspace --all-targets -- -D warnings` pass on the skeleton before any model code exists

---

## Phase 2: Foundational (Blocking Prerequisites)

**Blocks every user story.** The kernel below is what makes the strictness contract
expressible; nothing else can be written correctly first.

### Audit completion (blocks modelling)

- [X] T005 Audit the three nested decoders in `palmier-macos-codebase/Sources/PalmierPro/Models/TextStyle.swift` (lines 87, 134, 163) and append their strictness tables to `specs/001-project-model/research.md`
- [X] T006 Audit the ~33 synthesized-`Codable` types under `palmier-macos-codebase/Sources/PalmierPro/Models/`, recording required vs optional per field, and append to `research.md`. Standard Swift rule applies: `Optional` is optional, non-`Optional` is required, declaration defaults are NOT applied
- [X] T007 Confirm by reading each type whether an explicit JSON `null` and an absent key are distinguishable anywhere that matters, and record the finding in `research.md` (research.md implication 5)

### Frame arithmetic

- [X] T008 Write failing tests in `crates/palmier-core/tests/frames.rs` for half-open range semantics, `duration = end - start`, zero and negative durations, and `i64` overflow at `startFrame + durationFrames`
- [X] T009 Implement `FrameRange` and checked frame arithmetic in `crates/palmier-core/src/frames.rs` so every operation returns an error rather than panicking or wrapping (FR-006, FR-007)

### The decoding kernel

- [X] T010 Write failing tests in `crates/palmier-core/tests/strictness.rs` asserting each helper's behavior on missing key, wrong type, and null, per the three-level table in plan.md
- [X] T011 Implement `DecodeError` in `crates/palmier-core/src/codec/error.rs` carrying the JSON path of the offending value, so failures name their location (FR-008)
- [X] T012 Implement `take_required`, `take_or_default`, and `take_lenient` in `crates/palmier-core/src/codec/strictness.rs`, each documented with the Swift construct it reproduces
- [X] T013 [P] Implement `coerce_unit_interval` (outside `0..=1` becomes `0`) and `clamp_range` in `crates/palmier-core/src/codec/ranges.rs`, with a test asserting the two are NOT interchangeable (FR-002b)
- [X] T014 Implement the kernel entry point in `crates/palmier-core/src/codec/mod.rs`: object into `serde_json::Map`, known keys extracted at their strictness, remainder captured as `extra` (FR-003)

**Checkpoint**: the kernel and frame math are tested and green. Modelling can begin.

---

## Phase 3: User Story 1 — A macOS project opens intact (Priority: P1) 🎯 MVP

**Goal**: every `project.json` Palmier Pro writes loads here with no loss.

**Independent test**: decode the fixture corpus and assert parsed values against
expected structures. Delivers value alone — the project can read real user data.

### Tests for User Story 1

- [X] T015 [P] [US1] Author fixture `crates/palmier-core/tests/fixtures/full.json` exercising every modeled type with non-default values
- [X] T016 [P] [US1] Author fixture `legacy-bare-timeline.json` (a bare `Timeline` document) and `legacy-transform-xy.json` (carrying `Transform`'s `x`/`y` keys)
- [X] T017 [P] [US1] Author fixtures for the spec's edge cases: empty timeline, zero timelines, clip at frame 0, `i64::MAX` boundary, zero and negative duration, duplicate ids, dangling `mediaRef`, dangling and cyclic nested timelines, unordered and duplicate keyframes, unicode and newline text, empty and truncated files
- [X] T018 [US1] Write failing tests in `crates/palmier-core/tests/decoding_contract.rs` asserting per-field strictness from `data-model.md` — including that a `Clip` with `"speed": "fast"` yields `1.0` while a `Transform` with `"width": "wide"` fails
- [X] T019 [P] [US1] Write failing tests in `crates/palmier-core/tests/edge_cases.rs` covering all 13 spec edge cases, each asserting the specified outcome rather than merely "does not crash"
- [X] T020 [P] [US1] Write a failing test asserting unknown fields survive decode at every nesting depth (FR-003)

### Implementation for User Story 1

- [X] T021 [P] [US1] Implement `ClipType`, `BlendMode`, `Interpolation`, and `TextFillMode` enums in their modules, decoding leniently to their audited defaults
- [X] T022 [P] [US1] Implement `Transform` and `Crop` in `crates/palmier-core/src/transform.rs`, including the legacy `x`/`y` migration `centerX = x + width - 0.5` reproduced exactly (FR-002c), using `take_or_default` because this type is strict on wrong types
- [X] T023 [P] [US1] Implement `Keyframe`, `KeyframeTrack`, and `AnimPair` in `crates/palmier-core/src/keyframe.rs`, normalizing order on load and tolerating empty, single-point, unordered, and duplicate-frame input
- [X] T024 [P] [US1] Implement `TextStyle` and its nested types in `crates/palmier-core/src/text.rs` per the T005 audit
- [X] T025 [P] [US1] Implement `TextAnimation`, `WordTiming`, and text layout types in `crates/palmier-core/src/text.rs` — `preset` → `none`, `perWordFrames` → `6`, all lenient
- [X] T026 [P] [US1] Implement `Effect`, `EffectParam`, `GradeCurve`, `HueCurves`, and `CurvePoint` in `crates/palmier-core/src/effect.rs` — `type` required, `enabled` → `true`, `params` → `{}`
- [X] T027 [P] [US1] Implement `TimelineMarker` in `crates/palmier-core/src/marker.rs` — the strictest type: everything required except `status`
- [X] T028 [P] [US1] Implement `MediaManifest`, `MediaManifestEntry`, and `MediaFolder` in `crates/palmier-core/src/media.rs`, all `take_or_default`, resolving no file on disk
- [X] T029 [US1] Implement `Clip` in `crates/palmier-core/src/timeline.rs` — only `mediaRef`, `startFrame`, `durationFrames` required; `edgeRounding` and `edgeSoftness` through `coerce_unit_interval`
- [X] T030 [US1] Implement `Track` in `crates/palmier-core/src/timeline.rs` — only `type` required, `syncLocked` defaulting to `true`, `displayHeight` through `clamp_range`, and name normalization where an invalid name becomes `None` rather than an error
- [X] T031 [US1] Implement `Timeline` and `TimelineViewState` in `crates/palmier-core/src/timeline.rs` — `fps`, `width`, `height`, `tracks` required
- [X] T032 [US1] Implement `ProjectFile`, `MulticamSource`, and `SpeakerRegistryEntry` in `crates/palmier-core/src/project.rs`, including the bare-`Timeline` fallback and rejection of a zero-timeline project (FR-004)
- [X] T033 [US1] Implement `materialize_ids()` in `crates/palmier-core/src/ids.rs` as a pass separate from decoding, with a test proving decode alone leaves absent ids as `None` (plan.md Q4)
- [X] T034 [US1] Implement the loader at the crate boundary in `crates/palmier-core/src/lib.rs` — reads a path, decodes, materializes ids — keeping all filesystem access out of model types (FR-010)
- [X] T035 [US1] Implement cross-entity validation from `data-model.md`: at least one timeline, unique ids per scope, non-negative durations, finite `speed`, resolvable and acyclic nested timelines
- [X] T036 [US1] Verify every test from T018–T020 passes and report the actual `cargo test` output

**Checkpoint**: a real macOS project loads intact. US1 is independently shippable.

---

## Phase 4: User Story 2 — A project written here opens on macOS (Priority: P1)

**Goal**: what this writes, Palmier Pro reads.

**Independent test**: encode each fixture and assert the output satisfies the Swift
decoders' requirements — every required key present, every value in the expected type.

**Depends on US1**: encoding needs the types US1 defines. This is a real dependency,
not an organizational one.

### Tests for User Story 2

- [ ] T037 [US2] Write failing tests in `crates/palmier-core/tests/roundtrip.rs` asserting decode → encode → decode is semantically equal for every fixture, ignoring key order and whitespace (SC-002)
- [ ] T038 [P] [US2] Write a failing test asserting absent optional fields are omitted rather than emitted as `null` (FR-005)
- [ ] T039 [P] [US2] Write a failing test asserting `Transform` encodes all nine modern keys and never emits legacy `x`/`y`
- [ ] T040 [P] [US2] Write a failing test asserting preserved unknown keys are re-emitted at the position they were captured
- [ ] T041 [P] [US2] Write a failing test asserting frame values encode as JSON integers, never in exponential notation (contracts/project-json.md)

### Implementation for User Story 2

- [ ] T042 [US2] Implement `Serialize` for every model type, omitting `None` optionals and re-emitting `extra`
- [ ] T043 [US2] Implement `Transform`'s custom encoder — all nine modern keys, never the legacy pair
- [ ] T044 [US2] Implement the writer at the crate boundary producing `project.json` bytes, with no filesystem coupling in model types
- [ ] T045 [US2] Verify every test from T037–T041 passes and report the actual `cargo test` output

**Checkpoint**: round trip is stable and self-consistent. Interop with a real Mac is
still unproven — see T060.

---

## Phase 5: User Story 3 — Inspecting a project from the terminal (Priority: P2)

**Goal**: point the binary at a project and see what is inside it.

**Independent test**: run against a fixture project and assert the summary reports the
correct timeline count, durations, and track composition.

### Tests for User Story 3

- [ ] T046 [P] [US3] Write a failing test in `crates/palmier/tests/inspect.rs` asserting the summary for a fixture project reports correct fps, resolution, duration in frames and timecode, and per-track clip counts, with exit code 0
- [ ] T047 [P] [US3] Write failing tests for the failure paths — path not found, not a project, malformed JSON, rejected document — each asserting a non-zero exit and no partial summary

### Implementation for User Story 3

- [ ] T048 [US3] Implement timecode formatting from frames and fps in `crates/palmier-core/src/frames.rs`
- [ ] T049 [US3] Implement the `inspect` subcommand in `crates/palmier/src/inspect.rs` per the CLI contract
- [ ] T050 [US3] Wire `clap` argument parsing and exit codes in `crates/palmier/src/main.rs`
- [ ] T051 [US3] Verify T046–T047 pass and report the actual output

**Checkpoint**: the feature is demonstrable end to end without reading test code.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [ ] T052 [P] Write the SC-004 property test in `crates/palmier-core/tests/no_panic.rs` — arbitrary JSON and arbitrary field values never panic, only error
- [ ] T053 Write the SC-005 performance gate as an ignored test generating a 10,000-clip project and asserting load under 500 ms; run it in release and record the measured number
- [ ] T054 If T053 fails, replace the map-buffering kernel's hot path with a streaming extraction, keeping the strictness helpers' behavior identical — the design changes, the contract does not
- [ ] T055 [P] Produce the SC-001 audit checklist mapping every persisted type to the test that covers it, and store it at `specs/001-project-model/coverage.md`
- [ ] T056 [P] Verify SC-003 by removing each guard in turn and confirming its test fails, then restoring it
- [ ] T057 [P] Add rustdoc to the public API of `palmier-core`, and to each strictness helper naming the Swift construct it reproduces
- [ ] T058 Run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` and fix everything
- [ ] T059 Push and confirm CI is green on Linux, Windows, and macOS. A local WSL2 run proves Linux only and must not be reported as more (SC-006)
- [ ] T060 **Manual, cannot be automated here**: open a project written by this code in Palmier Pro on a real Mac and confirm it loads. Record the result in `specs/001-project-model/coverage.md`. Until this is done, US2 is verified only for self-consistency

---

## Dependencies & Execution Order

### Phase Dependencies

Setup (T001–T004) → Foundational (T005–T014) → US1 (T015–T036) → US2 (T037–T045) →
US3 (T046–T051) → Polish (T052–T060).

Foundational blocks everything. T005–T007 block modelling specifically: writing a type
before its decoder is audited is how the strictness contract gets silently wrong.

### User Story Dependencies

- **US1** depends only on Foundational. It is the MVP.
- **US2** depends on US1 for the types it serializes.
- **US3** depends on US1 for loading; it does not need US2.

### Within Each User Story

Tests precede the implementation they cover. Model types precede the aggregates that
contain them: enums → `Transform`/`Crop`/`Keyframe`/`Effect`/`TextStyle` → `Clip` →
`Track` → `Timeline` → `ProjectFile`.

### Parallel Opportunities

- T015–T017 — fixture authoring, one file each
- T021–T028 — eight leaf model types, one module each, after the kernel is green
- T038–T041 — four independent encoding tests
- T052, T055, T056, T057 — independent polish tasks

Serial by nature: T029–T032 (each aggregate contains the previous), T053 → T054.

## Parallel Example: User Story 1

```text
# Fixtures together:
T015, T016, T017

# Then the leaf model types together, once T014 is green:
T021, T022, T023, T024, T025, T026, T027, T028

# Then the aggregates in order:
T029 → T030 → T031 → T032
```

## Implementation Strategy

### MVP First (User Story 1 only)

Setup → Foundational → US1. That alone delivers reading a real macOS project, is
independently testable, and is the foundation every later spec builds on.

### Incremental Delivery

US1 makes projects readable. US2 makes them writable and closes the compatibility
loop. US3 makes the whole thing observable from a terminal and becomes the debugging
tool for spec 002 onward.

### Notes

- The likeliest silent failure in this feature is a field decoded at the wrong
  strictness. It looks correct on well-formed input and only diverges on malformed
  input, which is exactly why T018 and T019 exist and why every helper choice is
  justified against `research.md` in review.
- T060 is the one thing the test suite structurally cannot prove. Do not let a green
  suite be mistaken for verified interoperability.
