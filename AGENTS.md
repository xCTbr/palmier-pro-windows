# Palmier Cross-Platform

An AI-native video editor for Windows, Linux, and macOS. The agent is the primary
interface: you edit video by talking to Claude Code (or any MCP client) while the
project runs entirely on your own machine. A manual editing GUI is in scope and
arrives at layer 2.

Rust workspace. GPLv3. Rebuilt from [Palmier Pro](https://github.com/palmier-io/palmier-pro)
(macOS/Swift), whose source is vendored here as reference.

## Current state

**Layer 0 is complete.** `palmier serve` exposes 15 MCP tools over loopback HTTP, or
over stdio with `--stdio` for a desktop client that spawns the process itself; a
project can be opened, edited, rendered, and saved. Specs 001–004 under `specs/` record
what was built, what was audited from the original, and where this project deliberately
diverges from it. Layer 1 is next; read the active spec before writing code.

## Build

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Rendering shells out to the `ffmpeg` binary at layer 0 — no `libav*` linkage, no
`pkg-config`, no bindgen. Do not add FFmpeg FFI bindings before layer 1.

**Linux and Windows are the supported platforms.** Development happens in WSL2, so a
local build targets Linux; the Windows binary comes from CI on a Windows runner. Never
claim Windows verification from a local build.

macOS is built and tested in CI because it is free and catches portability bugs early,
but it is **not a supported target** and no one on this project can run the app there.
A macOS-only failure is worth fixing, never worth blocking on.

## Layers

Each layer must be usable on its own. Do not start a layer before the one below it
works end to end.

- **L0** — MCP daemon: project model, `EditCommand` layer, 15 tools, render via
  `ffmpeg filter_complex`. No GPU, no UI. **Done.**
- **L1** — own compositor on wgpu; the 12 CIKernel shaders ported to WGSL; color,
  effects, keyframes; single-frame render on demand.
- **L2** — the app: Tauri shell, canvas timeline, preview, inspector, export.
- **L3** — multicam, whisper.cpp transcription, ONNX visual search, BYOK generation.

## Invariants

These are the decisions the project is built on. Changing one is an architecture
change, not an implementation detail.

- **One mutation path.** Every MCP tool call and every future mouse gesture produces
  an `EditCommand` applied by one function. Ripple, clamping, linking, and validation
  exist exactly once. Never add a second way to mutate a project.
- **Undo is the command journal.** Not snapshots, not per-feature undo stacks. One
  user intent is one journal entry; failed, refused, and no-op commands add nothing.
- **`project.json` stays compatible with the original `.palmier` format.** A project
  written by Palmier Pro on macOS must open here, and the reverse. Extend with
  optional fields; never repurpose or drop an existing one.
- **Time is integer frames.** Keep frame counts as `i64` end to end. Convert to
  seconds only at an FFmpeg or UI boundary, and never store the result.
  Clip ranges are half-open: `[start, end)`, `duration = end - start`.
- **Tool contracts are machine-facing and stable.** Never serialize localized text,
  UI labels, or internal type names into an MCP response.
- **Generation is optional everywhere.** Providers are BYOK and pluggable behind a
  trait. Nothing in the project model may know a provider exists.
- **Validate before mutating.** Resolve the whole request, then apply atomically. A
  partially applied command is a bug, not a partial success.

## Testable objectives

Every spec states what "done" means as something a test can assert. A task without a
failing test that proves it is not started, and a claim of completion without the
test output is not a completion.

- Unit-test domain logic against the `EditCommand` layer, not through the MCP wire.
- Test the MCP surface end to end over HTTP, and verify the outcome by reading state
  back — never trust a success response as proof.
- Every bug fix gets a regression test that fails before the fix.
- Cover the negative paths the spec names: validation, no-op, boundary frames, empty
  timeline, missing media, and cancellation.
- Tests run in parallel. Use unique temporary directories; never share fixed paths,
  ports, or global state.

## Reference codebase

`palmier-macos-codebase/` is the original Swift source, vendored read-only.

**Never edit anything under it.** It is a specification, not a dependency, and it
does not build here.

What it is good for: the timeline data model (`Sources/PalmierPro/Models/`), the edit
engines (`Sources/PalmierPro/Editor/ViewModel/`), the 52 MCP tool contracts
(`Sources/PalmierPro/Agent/Tools/ToolDefinitions.swift`), the 12 shaders (`Metal/`),
and the design tokens (`Sources/PalmierPro/UI/AppTheme.swift`).

Port behavior and contracts. Do not port Swift structure, naming, or class layout
into Rust — write Rust that reads like Rust.

Upstream is still active. The `macos-reference` branch is pristine upstream, and the
`upstream` remote points at the original repo, so contract drift stays visible:

```bash
git fetch upstream
git diff macos-reference upstream/main -- Sources/PalmierPro/Agent/Tools/
```

Read reference files without leaving the working tree with `git show` and `git grep`.

## Style

- Choose the simplest implementation that fully meets the current requirement. No
  speculative abstraction, configuration, or indirection.
- No backward-compatibility layers. This project has no released version to be
  compatible with — the sole exception is the `.palmier` format above.
- Comments explain why, never what. One line. No narration, no docstring paragraphs
  on internal functions, no commented-out code.
- `unsafe` is denied workspace-wide. Removing that lint requires a stated reason.
- Errors are typed with `thiserror` in libraries and surfaced with context in the
  binary. Never swallow an error that changes the outcome.
- Remove dead code and temporary diagnostics before finishing.

## Reporting

Report exactly what ran and what did not. If a build, test, or verification was
skipped or blocked, say so plainly and name the blocker. Do not describe a subset
as a full run.
