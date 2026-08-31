# Feature Specification: MCP server and the layer-0 tools

**Feature Branch**: `003-mcp-tools` | **Created**: 2026-08-31 | **Status**: Draft
**Layer**: L0 — MCP daemon

**Input**: Expose the `EditCommand` layer over MCP so Claude Code, Cursor, or any MCP
client can read and edit a project by conversation, entirely on the user's machine.

## Why this exists

This is the feature the project was started for. Specs 001 and 002 built a model and a
mutation path with no way to reach them; this one puts the agent in front of them.

## Scope: ten tools

Layer 0's thirteen tools split by what they need. These ten need neither media probing
nor rendering, so they land here:

`manage_project` · `get_timeline` · `manage_tracks` · `add_clips` · `move_clips` ·
`remove_clips` · `split_clips` · `ripple_delete_ranges` · `set_clip_properties` · `undo`

`import_media`, `get_media`, and `export_project` need FFmpeg and move to spec 004.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Editing by conversation (Priority: P1)

Someone runs the daemon, points Claude Code at it, and edits a real project by talking:
"open this project", "show me the timeline", "cut the first three seconds", "move that
clip to the second track", "undo that".

**Why this priority**: it is the product.

**Independent Test**: drive the server over HTTP end to end, then verify by reading
state back — never by trusting a success response.

**Acceptance Scenarios**:

1. **Given** the daemon is running, **When** a client initializes, **Then** it receives the ten tools with their schemas.
2. **Given** a project is open, **When** `get_timeline` is called, **Then** it returns fps, resolution, duration, tracks with stable ids, and clips with half-open frame ranges.
3. **Given** a timeline, **When** `add_clips` places a clip and `get_timeline` is called again, **Then** the clip is present at the frames requested.
4. **Given** an applied edit, **When** `undo` is called, **Then** reading the timeline back shows the prior state.
5. **Given** a tool call that the command layer refuses, **When** it is made, **Then** the response says it was refused and why, and reading state back shows nothing changed.

---

### User Story 2 — Contracts an agent can trust (Priority: P1)

The agent gets the same answer from a tool's receipt as it would from reading state
back, so it does not need to re-read after every call to know what happened.

**Why this priority**: constitution principle VI. A tool that reports success for an
adjusted outcome teaches the agent to distrust every response.

**Acceptance Scenarios**:

1. **Given** a command that changes nothing, **When** the tool is called, **Then** the response says so explicitly rather than reporting success.
2. **Given** a batch where one entry is invalid, **When** the tool is called, **Then** the whole call is rejected with no partial state, as the original's own contract promises.
3. **Given** any tool response, **When** it is inspected, **Then** it contains no localized text, UI label, or internal type name.

---

### User Story 3 — Running safely on the user's machine (Priority: P2)

The daemon binds loopback only, so nothing on the network can reach the user's projects.

**Acceptance Scenarios**:

1. **Given** the daemon is running, **When** its listening address is inspected, **Then** it is bound to `127.0.0.1` and not to any routable interface.
2. **Given** the port is already in use, **When** the daemon starts, **Then** it fails with a clear diagnostic and a non-zero exit rather than binding elsewhere silently.

### Edge Cases

- A tool call before any project is open.
- A tool called with a missing required argument, a wrong-typed argument, and an unknown argument.
- `undo` with an empty journal.
- A frame window where `endFrame <= startFrame`.
- A timeline with no tracks, and a track with no clips.
- Two clients connected at once.
- A client that disconnects mid-call.
- Arguments containing hostile strings — very long, unicode, embedded newlines.

## Requirements *(mandatory)*

- **FR-001**: The system MUST serve MCP over HTTP on `127.0.0.1`, defaulting to port 19789 to match the original.
- **FR-002**: The system MUST expose the ten tools with JSON Schema input contracts.
- **FR-003**: Every mutating tool MUST produce exactly one `EditCommand` and apply it through the single mutation path. No tool may mutate a project directly.
- **FR-004**: Tool responses MUST be derived from the `Receipt` the command layer returns, not composed independently.
- **FR-005**: A refused command MUST produce a response naming the refusal and its reason, and MUST leave the project unchanged.
- **FR-006**: A no-op MUST be reported as a no-op, never as a success.
- **FR-007**: Responses MUST NOT contain localized text, UI labels, or internal type names.
- **FR-008**: Tools MUST accept stable ids for tracks and clips. `trackIndex` is accepted as a compatibility alias where the original used it, and is resolved to an id immediately.
- **FR-009**: The system MUST validate arguments against the schema and refuse malformed calls with a diagnostic naming the offending argument.
- **FR-010**: The system MUST NOT panic on any input from a client.
- **FR-011**: Project state MUST persist to disk only when `manage_project` is asked to save, never as a side effect of an edit.

### Out of scope

Media import, probing, and the manifest's relation to files on disk; rendering and
export; keyframes, colour, text, captions, transcripts; generation; multicam; the
in-app agent chat; authentication.

## Success Criteria *(mandatory)*

- **SC-001**: Every tool has an end-to-end test over HTTP that verifies the outcome by reading state back.
- **SC-002**: Every tool has a refusal test asserting both the reported reason and that the project is unchanged.
- **SC-003**: A real MCP client can list the tools and call them without a custom adapter.
- **SC-004**: No client input causes a panic, verified by a property test over arbitrary JSON arguments.
- **SC-005**: The daemon binds loopback only, asserted by a test.
- **SC-006**: Tests pass on Linux and Windows in CI; clippy and fmt clean.

## Assumptions

- `ToolDefinitions.swift` is the authoritative source for tool names, argument shapes, and description prose. Descriptions are ported and trimmed to this spec's scope, never rewritten from scratch — they are years of contract iteration.
- One project is open at a time. Multi-project session binding is a later concern.
- No authentication: the server is loopback-only and single-user, as the original is.

## Open questions

- **Q1**: The original's schemas take `trackIndex`, a positional index, while `get_timeline` returns a stable `trackId`. Indexes shift when empty tracks are pruned, so an agent holding one across a mutation can address the wrong track. Constitution principle VI requires stable ids. Resolve in planning: accept both, prefer id.
- **Q2**: How much of each description to port. The full `get_timeline` text describes caption groups, colour grades, and keyframes that this layer does not implement. Porting it verbatim would document behaviour that does not exist.
