# Feature Specification: Media and render

**Feature Branch**: `004-render-export` | **Created**: 2026-08-31 | **Status**: Complete
**Layer**: L0 — MCP daemon (final increment)

## Why this exists

Specs 001–003 let an agent build a timeline it could never watch. This closes the loop:
import real files, render the cut, get an MP4.

## User Scenarios

### US1 — Importing footage (P1)
`import_media` probes each file for duration, resolution, frame rate, and whether it
carries audio, and returns a `mediaRef` that `add_clips` accepts. One unreadable file
rejects the whole call with no partial state.

### US2 — Watching the cut (P1)
`export_project` renders the timeline to a video file: clips composited bottom track up,
gaps black, audio mixed. Media that vanished from disk is named in the response rather
than silently omitted.

### US3 — Knowing what is there (P2)
`get_media` lists the project's assets and says whether each still resolves on disk.

## Requirements

- **FR-001**: Rendering MUST shell out to the `ffmpeg` binary. No `libav*` linkage, no bindgen, before layer 1.
- **FR-002**: A missing `ffmpeg` or `ffprobe` MUST be reported as such, before a render starts.
- **FR-003**: A clip MUST be positioned in time with `setpts`, never with `overlay=enable=`.
- **FR-004**: A render MUST be exactly the timeline's duration, including leading and trailing gaps.
- **FR-005**: Media that does not resolve MUST be reported, never silently dropped.
- **FR-006**: A file without an audio stream MUST NOT produce an audio chain in the graph.
- **FR-007**: Import MUST validate every file before recording any of them.
- **FR-008**: Numbers in the filter graph MUST NOT use exponential notation.
- **FR-009**: `media.json` MUST round-trip like `project.json`, preserving unknown fields.

### Out of scope
Colour, effects, keyframes, transitions, text rendering, per-clip transform and crop,
hardware encoders, HDR, ProRes, and preview streaming. Layer 1 owns the compositor.

## Success Criteria

- **SC-001**: A two-clip cut renders and its frames actually change across the second clip's span — proving placement, not merely that ffmpeg exited zero.
- **SC-002**: A timeline with a leading gap renders black there and keeps the full duration.
- **SC-003**: Every tool has an end-to-end test over HTTP verified by reading state back.
- **SC-004**: The graph is asserted as a string, without decoding a frame.
- **SC-005**: Tests pass on Linux and Windows in CI; clippy and fmt clean.

## What the spike established

The approach was proved before it was specified, and it changed the design twice.

1. **`overlay=enable=` does not place a clip.** It gates visibility while the clip's
   frames still start at PTS 0, so a clip placed later shows its last frame frozen for
   its whole span. Three sampled timestamps returned byte-identical frames. Placement is
   `setpts=PTS-STARTPTS+<offset>/TB`.
2. **A video file may have no audio stream.** Emitting `[N:a]` for such a file makes
   ffmpeg reject the entire filtergraph, not just that chain — so the graph builder must
   be told what each source contains.

## Bugs this feature found in spec 001

Both sat in the gap `coverage.md` had already named — *"only the type's decoding shape is
tested, not its use"*.

- **`MediaSource`** was modelled as a plain string enum. It is a Swift enum with
  associated values, so it is `{"external":{"absolutePath":"…"}}` on the wire. No real
  `media.json` could have been decoded.
- **`MediaManifestEntry.id`** is required by the original and was missing from the model
  entirely — and it is the `mediaRef` every clip carries.

Both are fixed with a regression test in `decoding_contract.rs`.
