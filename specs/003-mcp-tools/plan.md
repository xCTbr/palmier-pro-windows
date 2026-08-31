# Implementation Plan: MCP server and the layer-0 tools

**Branch**: `003-mcp-tools` | **Date**: 2026-08-31 | **Spec**: [spec.md](./spec.md)

## Summary

A `palmier-mcp` crate holding ten tools, each of which resolves its arguments into
exactly one `EditCommand` and renders the resulting `Receipt`. The tools own no editing
logic — that is the point. `rmcp` provides the protocol and the streamable HTTP
transport; the binary wires a session and serves loopback.

## Technical Context

**Language**: Rust 1.97, edition 2024
**Dependencies**: `rmcp` 3.1 (`server`, `transport-streamable-http-server`, `schemars`),
`tokio`, `axum` via rmcp's transport, `serde_json`, `palmier-core`.
**Testing**: `cargo test` — unit tests per tool, plus end-to-end tests that drive a real
server over HTTP and verify by reading state back.
**Target**: Linux and Windows.
**Constraints**: loopback only; no panics; no mutation outside `EditSession::apply`.

## Constitution Check

| Principle | Status |
|---|---|
| I. One mutation path | Enforced structurally: `EditSession` is the only public mutator, so a tool physically cannot bypass it |
| II. Undo is the journal | `undo` calls `EditSession::undo`; the tool adds nothing |
| III. Format compatibility | Untouched |
| IV. Integer frames | Frames cross the wire as JSON integers and stay `i64` |
| V. Test-first | Every tool gets a success, refusal, and no-op test over HTTP |
| VI. Agent contracts | **This feature is the principle.** Responses are rendered from `Receipt`; ids are stable; no localized text |
| VII. Layers ship whole | Spec 002 works end to end; this is the next increment |

No violations.

## Q1 — track identity

**Decision: `trackId` is the contract; `trackIndex` is accepted as an alias and resolved
to an id before anything else happens.**

The original returns a stable `trackId` from `get_timeline` and then takes `trackIndex`
in every mutation. Indexes shift the moment an empty track is pruned, so an agent that
reads a timeline, thinks, and then acts can address the wrong track — a silent
wrong-target bug, which is the worst kind for an agent.

Accepting both keeps an agent trained on the original's schema working, while the
response always hands back ids, so correct usage is the path of least resistance.

## Q2 — how much description to port

**Decision: port the prose that describes behaviour this layer actually has, and cut the
rest.** Constitution principle VI says a contract must not promise what it does not do,
and describing caption groups, colour grades, or keyframes here would do exactly that.
The cut paragraphs return with the features in later specs.

What is ported verbatim wherever it still holds: the frame semantics (`[start, end)`,
end exclusive, `duration = end − start`), the overwrite-on-same-track rule, the
default-omission rule, and the atomicity promise — *"each entry is validated up front;
one bad entry rejects the whole call with no partial state"* — which this project now
honours in the implementation as well as the contract.

## Design

```text
crates/palmier-mcp/src/
├── lib.rs        the server type and its tool router
├── session.rs    the open project and its EditSession, behind a lock
├── render.rs     Receipt and Timeline -> JSON, one place
└── tools/
    ├── mod.rs
    ├── project.rs    manage_project
    ├── timeline.rs   get_timeline, manage_tracks
    ├── clips.rs      add_clips, move_clips, remove_clips, split_clips,
    │                 ripple_delete_ranges, set_clip_properties
    └── undo.rs       undo
```

Every mutating tool is the same four lines: resolve arguments to ids, build one
`EditCommand`, `apply`, render the `Receipt`. A tool that needs more than that is a tool
doing work that belongs in the command layer.

## Risk carried into tasks

`rmcp`'s API surface is the one unknown — it is a young crate and the version pinned
here may not match its documentation. The first task is a spike that stands a server up
and lists tools, before any tool is written against an assumed API.
