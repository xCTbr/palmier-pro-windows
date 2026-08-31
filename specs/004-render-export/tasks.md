# Tasks: Media and render

- [X] T001 Spike: prove a filter_complex graph places clips correctly, by measuring frames
- [X] T002 Fix `MediaSource` and the missing `MediaManifestEntry.id` in palmier-core, with a regression test
- [X] T003 `probe.rs` — ffprobe to `MediaInfo`
- [X] T004 `graph.rs` — timeline to filter_complex, with `ResolvedMedia` so audio chains match reality
- [X] T005 `render.rs` — run the graph; `render_frame` seeds layer 1's preview
- [X] T006 Graph tests asserted as a string, plus real renders whose frames are checked
- [X] T007 Media manifest in the session: load `media.json`, resolve refs, save beside the project
- [X] T008 `import_media`, `get_media`, `export_project`
- [X] T009 End-to-end tests over HTTP for the three tools
- [X] T010 fmt, clippy, full suite, CI green on Linux and Windows
