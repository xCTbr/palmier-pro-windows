# Palmier Cross-Platform

**An AI-native video editor you drive by talking to your coding agent.**

Connect it to Claude Code, Cursor, or any MCP client and edit a real timeline in
conversation — cut, arrange, retime, color, caption, export. Everything runs on your
own machine. A manual editing GUI is in scope and arrives at layer 2.

Windows, Linux, and macOS. Free, and open source under GPLv3.

## Status

**Layer 0 works.** You can edit video by talking to your agent and watch the result.

| | | |
|---|---|---|
| **L0** | MCP daemon — project model, edit commands, 15 tools, render via FFmpeg | **done** |
| **L1** | Own compositor on wgpu; color, effects, keyframes | not started |
| **L2** | The app — timeline, preview, inspector, export | not started |
| **L3** | Multicam, local transcription, visual search, BYOK generation | not started |

Linux and Windows are supported. macOS builds and tests in CI but is not a target.

## Use it

**Download a build.** Every push to `main` publishes binaries for Windows and Linux —
grab one from the [Actions tab](https://github.com/xCTbr/palmier-pro-windows/actions/workflows/release.yml),
open the most recent run, and download the artifact for your platform.

`palmier` is a command-line program, not an installer, and it needs `ffmpeg` and
`ffprobe` on `PATH`. On Windows:

```powershell
winget install Gyan.FFmpeg     # then open a new terminal so PATH updates
.\palmier.exe serve
```

Windows will warn that the binary is unrecognised. It is unsigned — this project has no
code-signing certificate — so SmartScreen has nothing to check it against.

## Connect an agent

**Claude Code, Cursor, Codex** — anything that speaks MCP over HTTP. Start
`palmier serve`, leave it running, and in another terminal:

```bash
claude mcp add --transport http palmier http://127.0.0.1:19789/mcp
```

**Claude Desktop** does not accept an `http://` connector — its custom connectors
require HTTPS, which a loopback server cannot offer and should not need. Use stdio
instead: Claude Desktop starts the process itself. Add this to
`%APPDATA%\Claude\claude_desktop_config.json` on Windows, or
`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS:

```json
{
  "mcpServers": {
    "palmier": {
      "command": "C:\\path\\to\\palmier.exe",
      "args": ["serve", "--stdio"]
    }
  }
}
```

Restart Claude Desktop afterwards. Do not run `palmier serve` yourself for this — the
app spawns and manages its own copy.

Or build it yourself:

```bash
cargo build --release
./target/release/palmier serve
```

```bash
claude mcp add --transport http palmier http://127.0.0.1:19789/mcp
```

Then talk to your agent: open a project, import footage, cut it, export it.

```bash
palmier inspect path/to/project.palmier
```

## Build

Requires a Rust toolchain, and `ffmpeg` and `ffprobe` on `PATH`.

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
