# Tasks: MCP server and the layer-0 tools

- [ ] T001 Spike: stand up an `rmcp` server on loopback that lists one trivial tool, proving the API shape before ten tools are written against it
- [ ] T002 Implement `session.rs` — the open project, its `EditSession`, and save-on-request only
- [ ] T003 Implement `render.rs` — `Receipt` and `Timeline` to JSON, with default omission and no localized text
- [ ] T004 [P] `manage_project` — open, save, close, describe
- [ ] T005 [P] `get_timeline` — windowed read with stable ids
- [ ] T006 [P] `manage_tracks` — add, remove, set properties
- [ ] T007 `add_clips`, `move_clips`, `remove_clips`
- [ ] T008 `split_clips`, `ripple_delete_ranges`, `set_clip_properties`
- [ ] T009 [P] `undo`
- [ ] T010 Wire the daemon in `crates/palmier/src/main.rs` — `palmier serve`, loopback, port 19789
- [ ] T011 End-to-end tests over HTTP: every tool, verified by reading state back (SC-001)
- [ ] T012 Refusal tests: reported reason plus project unchanged (SC-002)
- [ ] T013 Property test over arbitrary JSON arguments (SC-004)
- [ ] T014 Assert the listener binds loopback only (SC-005)
- [ ] T015 fmt, clippy, full suite, push, CI green on Linux and Windows
