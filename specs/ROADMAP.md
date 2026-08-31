# Roadmap

Where the project is, and what each remaining spec has to answer. Written 2026-08-31,
at the pause after layer 0 and the first interface.

## Done

| Spec | What it established |
|---|---|
| [001](001-project-model/spec.md) | The `.palmier` model. Three decoding strictness levels; compatibility is **audited, not verified** — no Mac to check against |
| [002](002-edit-commands/spec.md) | The `EditCommand` layer. One mutation path, undo as a command journal with inverse patches |
| [003](003-mcp-tools/spec.md) | MCP over loopback HTTP, and stdio for desktop clients |
| [004](004-render-export/spec.md) | Render through `ffmpeg filter_complex`; import, probe, export |
| — | 29 tools, generation through Stitch with sticky key rotation, a job queue, and an interface with preview and a chat panel |

**256 tests. CI green on Linux, Windows, and macOS.** Supported targets are Linux and
Windows; macOS builds only as a portability check.

## Next, in order

### 005 — Editing with the mouse

The interface reads the timeline; it cannot change it. Dragging a clip, trimming an
edge, and dropping media are the operations that make it an editor rather than a viewer.

Every gesture must produce an `EditCommand` — the same ones the agent uses. That is the
whole reason principle I exists, and this spec is where it gets paid or exposed.

**Open questions**: how a drag previews before it commits, without a second mutation
path for the preview state; whether snapping belongs in the command layer or the UI;
what a multi-clip selection means for a command that refuses atomically.

### 006 — The compositor (layer 1)

`filter_complex` cannot do colour, effects, keyframes, per-clip transform, or text.
Eight tools are waiting on it: `apply_color`, `inspect_color`, `apply_effect`,
`set_keyframes`, `apply_layout`, `add_texts`, `update_text`, `add_captions`.

wgpu, WGSL, and the twelve CIKernel shaders in `palmier-macos-codebase/Metal/` ported
across. Preview stops being one ffmpeg call per frame and becomes live.

**Risks worth naming now**: text rendering is the lowest-fidelity item on the whole
substitution map — Core Text has no equal, and the title tool depends on it. And a
wgpu spike should prove three 1080p tracks composite in real time *before* the shaders
are ported, not after.

### 007 — The installer, finished

The Tauri shell builds and bundles, but nothing has been installed and run by a person
yet. Until someone double-clicks the `.msi` and reaches a working window, this is
unverified — it cannot be checked from here.

**Also**: the binary is unsigned, so SmartScreen will keep warning. Signing costs money
and needs a certificate; worth deciding rather than drifting.

### 008 — Transcription, and cutting by words

`detect_silence` finds where nobody spoke. It cannot find where someone stumbled over a
sentence. whisper.cpp locally gives `get_transcript`, `remove_words`, and the
transcript-driven `remove_silence` the original has.

This is the feature that turns "cut the dead air" into "cut the part where I repeated
myself".

### Later

- **Codex in the chat panel** — `claude_binary()` is the seam; a second CLI slots in beside it
- **A BYOK chat fallback** for people without any agent CLI
- **`manage_exports`** — the job queue exists; export still blocks by choice, because 20s does not need a queue
- **Multicam, visual search, beat detection** — layer 3, and none of it blocks anything above

## Two things that will not resolve themselves

**Format compatibility is unverifiable here.** Every claim that a Mac can open what this
writes is a code-reading claim. Spec 001's T060 stays deferred until someone has a Mac.

**Nothing has been used by a second person.** Every bug that mattered so far surfaced
the moment the tool met real footage, and none of them were on my list beforehand. The
next real session is worth more than the next feature.
