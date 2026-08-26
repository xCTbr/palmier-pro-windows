# Palmier Cross-Platform

**An AI-native video editor you drive by talking to your coding agent.**

Connect it to Claude Code, Cursor, or any MCP client and edit a real timeline in
conversation — cut, arrange, retime, color, caption, export. Everything runs on your
own machine. A manual editing GUI is in scope and arrives at layer 2.

Windows, Linux, and macOS. Free, and open source under GPLv3.

## Status

**Layer 0, in development.** Nothing is usable yet.

| | | |
|---|---|---|
| **L0** | MCP daemon — project model, edit commands, 13 tools, render via FFmpeg | in progress |
| **L1** | Own compositor on wgpu; color, effects, keyframes | not started |
| **L2** | The app — timeline, preview, inspector, export | not started |
| **L3** | Multicam, local transcription, visual search, BYOK generation | not started |

## Build

Requires a Rust toolchain and the `ffmpeg` binary on `PATH`.

```bash
cargo build
cargo test
```

## Credit

This project is a cross-platform rebuild of
**[Palmier Pro](https://github.com/palmier-io/palmier-pro)** by Palmier, Inc. — a
macOS video editor built for AI, released under GPLv3.

Palmier Pro is the reason this exists. Its timeline model, edit semantics, and MCP
tool contracts are the specification this project is built against, and its Swift
source is vendored in `palmier-macos-codebase/` for reference. Nothing here is
affiliated with or endorsed by Palmier, Inc.

Frame-accurate editing over MCP was their idea. This is that idea, ported to the
machines it did not run on.

## License

GPLv3. See [LICENSE](LICENSE).
