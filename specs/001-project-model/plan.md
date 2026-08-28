# Implementation Plan: Project model and `.palmier` round-trip fidelity

**Branch**: `001-project-model` | **Date**: 2026-08-28 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-project-model/spec.md`

## Summary

Model every persisted type of the `.palmier` format in `palmier-core` so a project
written by Palmier Pro on macOS loads here without loss, and a project written here
loads there. The audit in [research.md](./research.md) established the decoding
contract: three strictness levels that vary within a single type, eleven hand-written
decoders, and two different out-of-range policies.

The technical approach follows from that audit. Every model type deserializes through
one uniform kernel: collect the JSON object into a map, extract each known key at its
audited strictness, and keep the remainder as captured unknown fields. One pattern
resolves the strictness contract (FR-002), unknown-field preservation (FR-003), and
per-field defaults (FR-002a) together, instead of three mechanisms fighting each other.

## Technical Context

**Language/Version**: Rust 1.97, edition 2024

**Primary Dependencies**: `serde` + `serde_json` (model and codec), `uuid` (id
materialization), `thiserror` (typed errors), `insta` (snapshot tests), `clap` (the
`inspect` command). No new dependencies beyond those already in the workspace.

**Storage**: `project.json` inside a `.palmier` folder. Read-only in this feature;
writing to a live package is out of scope.

**Testing**: `cargo test` — unit tests in-crate, fixture-driven integration tests in
`crates/palmier-core/tests/`, plus a property test over arbitrary JSON for SC-004.

**Target Platform**: Windows, Linux, macOS. Verified locally on Linux; the other two
come from CI.

**Project Type**: Rust library (`palmier-core`) plus one binary subcommand.

**Performance Goals**: A 10,000-clip project loads in under 500 ms (SC-005). This is
the one number the map-buffering approach puts at risk, so it is measured, not assumed.

**Constraints**: No filesystem access inside model types (FR-010). No panic on any
input (SC-004). `unsafe` denied workspace-wide.

**Scale/Scope**: ~44 persisted types, 11 with hand-written decoders. Roughly 3,000
lines of model code and a comparable volume of tests and fixtures.

## Constitution Check

*GATE: passed before Phase 0. Re-checked after Phase 1 design — see bottom.*

| Principle | Applies | Status |
|---|---|---|
| I. One mutation path | Not yet — this feature has no mutation | N/A |
| II. Undo is the command journal | Not yet | N/A |
| III. Format compatibility | **This feature is the principle** | Satisfied: FR-001..005, and unknown-field capture is a first-class design element rather than an afterthought |
| IV. Time is integer frames | Yes | Satisfied: FR-006/007. All frame quantities are `i64`; a `frames` module owns checked arithmetic and half-open ranges |
| V. Test-first, named failure cases | Yes | Satisfied: 13 edge cases in the spec each map to a named test; SC-003 requires each guard's test to fail when its guard is removed |
| VI. Agent contracts | Not yet — no MCP surface | N/A |
| VII. Layers ship whole | Yes | Satisfied: this is the first increment of L0 and is usable alone via `palmier inspect` |

No violations. Complexity Tracking is empty.

## Phase 0: Research — complete

[research.md](./research.md) resolves Q1 and Q2 by auditing all eleven custom
`Codable` conformances. Two questions remained; both are decided here.

### Q3 — Unknown-field preservation

**Decision**: each model struct carries an `extra: serde_json::Map<String, Value>`
populated by its hand-written deserializer, and re-emitted on encode.

**Rationale**: nearly every type needs a hand-written `Deserialize` anyway to
reproduce its audited strictness. Capturing leftover keys inside that same impl is
close to free, and the unknown data travels with the entity that owned it — which
matters from spec 002 onward, when clips move between tracks.

**Alternatives rejected**:
- `#[serde(flatten)]` — forces serde's map-buffering path anyway, composes awkwardly
  with custom `Deserialize`, and offers nothing the explicit capture does not.
- Keeping the raw document alongside the typed model and merging by JSON path on
  encode — a single tidy place for the data, but path identity breaks the moment a
  clip is reordered or moved, which is the whole point of the next feature.

### Q4 — Generated ids and round-trip determinism

**Decision**: decode leaves a missing id as `None`. A separate, explicit
`materialize_ids()` pass fills every empty id with a fresh UUID and is called by the
loader, not by the deserializer.

**Rationale**: reproduces the original's observable behavior — after a load, every
entity has an id — while keeping decoding a pure, deterministic function. The
round-trip test then compares decoded-and-re-encoded output without materialization
and is exactly reproducible, and id materialization gets its own focused test rather
than contaminating every other assertion.

**Alternatives rejected**:
- Generating inside the deserializer, matching the Swift structure literally — makes
  SC-002's comparison nondeterministic and every fixture test order-dependent.
- Threading an id-generator through serde as deserializer context — serde has no
  clean seam for it, and the two-phase split is simpler and more testable.

## Phase 1: Design

### The decoding kernel

One pattern, used by every model type:

1. Deserialize the object into `serde_json::Map<String, Value>`.
2. Pull each known key out of the map at its audited strictness.
3. Whatever remains is `extra`.

Three extraction helpers, named for the Swift construct each reproduces, so the audit
maps onto the code one-to-one:

| Helper | Reproduces | Missing key | Wrong type |
|---|---|---|---|
| `take_required` | `try c.decode` | error | error |
| `take_or_default` | `try c.decodeIfPresent ?? d` | default | **error** |
| `take_lenient` | `(try? c.decode) ?? d` | default | **default, silently** |

Choosing the wrong helper is the most likely way this feature fails, and the failure
is silent on well-formed input. Every field's helper choice is therefore justified by
a `research.md` reference in review, and the malformed-value tests exist specifically
to catch a `take_lenient` that should have been `take_or_default`.

### Out-of-range policies

Two distinct behaviors, kept distinct (FR-002b):

- `coerce_unit_interval` — outside `0...1` becomes `0`. Used by `edgeRounding` and
  `edgeSoftness` only.
- `clamp_range` — clamped to the bounds. Used by `displayHeight` only.

### Documentation (this feature)

```text
specs/001-project-model/
├── plan.md              # This file
├── research.md          # Decoding-contract audit (Phase 0)
├── data-model.md        # Phase 1
├── quickstart.md        # Phase 1
├── contracts/
│   └── project-json.md  # The format contract, field by field
└── tasks.md             # Created by /speckit-tasks, not here
```

### Source code (repository root)

```text
crates/palmier-core/
├── src/
│   ├── lib.rs
│   ├── codec/
│   │   ├── mod.rs           # the kernel: object → typed value + extra
│   │   ├── strictness.rs    # take_required / take_or_default / take_lenient
│   │   ├── ranges.rs        # coerce_unit_interval / clamp_range
│   │   └── error.rs         # DecodeError carrying the JSON path
│   ├── frames.rs            # i64 frame math, FrameRange [start, end)
│   ├── project.rs           # ProjectFile, legacy bare-Timeline fallback
│   ├── timeline.rs          # Timeline, Track, Clip
│   ├── transform.rs         # Transform (legacy x/y migration), Crop
│   ├── keyframe.rs          # KeyframeTrack, Keyframe, AnimPair, Interpolation
│   ├── text.rs              # TextStyle, TextAnimation, WordTiming, layout
│   ├── effect.rs            # Effect, EffectParam, grades and curves
│   ├── marker.rs            # TimelineMarker
│   ├── media.rs             # MediaManifest, MediaFolder, entries
│   └── ids.rs               # materialize_ids
└── tests/
    ├── fixtures/            # hand-authored project.json corpus
    ├── decoding_contract.rs # strictness per field, per research.md
    ├── roundtrip.rs         # SC-002
    ├── edge_cases.rs        # the spec's 13 cases
    └── no_panic.rs          # SC-004 property test

crates/palmier/
└── src/
    ├── main.rs
    └── inspect.rs           # palmier inspect <path>
```

**Structure Decision**: one module per coherent group of the original's `Models/`
types, plus a `codec` module owning the kernel that every type routes through. File
I/O lives in the loader at the crate boundary, never in a model type (FR-010), so the
whole model is testable from a string.

## Constitution Check — post-design

Re-evaluated after the design above. Still no violations.

Principle III is strengthened rather than merely satisfied: unknown-field capture is
in the kernel every type uses, so a type added later cannot silently forget it.
Principle IV is owned by one `frames` module rather than spread across call sites.
Principle V is served by the strictness table making each field's expected behavior
independently assertable.

One risk to carry into tasks: the map-buffering kernel is the simplest way to satisfy
three requirements at once, but it allocates per object. SC-005 (10,000 clips under
500 ms) is the gate, and it is measured before the feature is called done — not
assumed from the design being clean.

## Complexity Tracking

No constitutional violations. Section intentionally empty.
