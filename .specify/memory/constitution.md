# Palmier Cross-Platform Constitution

An AI-native video editor driven by a coding agent over MCP, running entirely on the
user's machine, on Windows, Linux, and macOS. Rebuilt in Rust from Palmier Pro
(macOS/Swift, GPLv3), whose source is vendored as reference in
`palmier-macos-codebase/`.

## Core Principles

### I. One mutation path (NON-NEGOTIABLE)

Every MCP tool call and every future mouse gesture produces an `EditCommand` applied
by one function. Ripple, clamping, linking, placement, and validation exist exactly
once in the codebase. There is never a second way to mutate a project, and no caller
is special — not a test, not a preview, not the GUI.

A change that adds a parallel mutation path is rejected regardless of how convenient
it is at the call site.

### II. Undo is the command journal

Undo is the journal of applied `EditCommand`s. Not snapshots, not per-feature undo
stacks. One user intent is one journal entry. Commands that fail validation, are
refused, or change nothing add no entry and leave no trace.

### III. Format compatibility with the original

`project.json` stays readable by and writable for Palmier Pro's `.palmier` format. A
project written on macOS opens here, and the reverse. Fields we do not model are
preserved verbatim through a round trip — silently dropping an unknown field corrupts
the user's project on the other platform.

Extend with optional fields. Never repurpose, rename, or remove an existing one.

**This invariant is audited, not verified.** The project targets Linux and Windows and
has no macOS access, so no test here can prove a real Mac accepts what this writes.
Uphold it from the decoder audit in `specs/001-project-model/research.md`, and never
describe it as verified compatibility.

### IV. Time is integer frames

Frame counts are `i64` end to end. Clip ranges are half-open — `[start, end)`,
`duration = end - start`. Conversion to seconds happens only at an FFmpeg or UI
boundary and the result is never stored.

Every frame computation validates before arithmetic: no overflow, no negative
duration, no index past the end. A panic on user or agent input is a defect.

### V. Test-first with named failure cases (NON-NEGOTIABLE)

A task is not started until a failing test proves the gap, and is not complete until
that test passes and its output is reported. "Done" always means something a test
asserts.

Every spec enumerates its negative cases explicitly — validation failure, no-op,
boundary frames, empty timeline, missing media, malformed input — and each one gets a
test. Coverage of the happy path alone is an incomplete task, not a complete one.

Domain logic is tested against the `EditCommand` layer. The MCP surface is tested end
to end over HTTP, and the outcome is verified by reading state back; a success
response is never accepted as proof that the requested change happened.

### VI. Agent contracts are the product

Tool contracts are machine-facing and stable. They express filmmaking intent, not
internal APIs. They use stable entity IDs, never positional indexes, as durable
identity.

A tool returns a structured receipt naming what changed, what was skipped, and what
was refused. It never returns a success-shaped response for an outcome that was
adjusted or not achieved, and never silently clamps, retargets, or substitutes unless
the contract promises that behavior and reports it.

Localized text, UI labels, and internal type names are never serialized into an MCP
response.

### VII. Layers ship whole

Each layer is usable on its own before the next begins. Work does not start on a
layer whose predecessor does not work end to end. Within a layer, prefer the smallest
implementation that fully meets the current requirement — no speculative abstraction,
configuration, or indirection, and no backward-compatibility layer for a version that
was never released.

## Constraints

- **Rust workspace**, `unsafe` denied workspace-wide.
- **Layer 0 shells out to the `ffmpeg` binary.** No `libav*` linkage, no bindgen, no
  `pkg-config` before layer 1.
- **Development happens in WSL2, so a local build targets Linux.** Windows and macOS
  binaries come from CI runners. A local build is never reported as Windows
  verification.
- **`palmier-macos-codebase/` is read-only.** It is a specification, not a
  dependency. Port behavior and contracts from it; never port Swift structure or
  naming into Rust.
- **Generation is optional everywhere.** Providers are BYOK behind a trait. Nothing
  in the project model may know a provider exists.

## Governance

This constitution supersedes convenience, velocity, and habit. Where a spec, plan, or
task conflicts with it, the constitution wins and the artifact is corrected.

Amending a principle requires stating what changed, why the original reasoning no
longer holds, and what existing code must be migrated. Principles marked
NON-NEGOTIABLE may not be waived for a single feature — they are amended for the
whole project or not at all.

Runtime development guidance lives in `AGENTS.md`, which must stay consistent with
this document.

**Version**: 1.0.0 | **Ratified**: 2026-08-26 | **Last Amended**: 2026-08-26
