//! `palmier serve --stdio`, driven the way a desktop client drives it: spawn the
//! process, write one request, read its response, then write the next.
//!
//! Claude Desktop's custom connectors require HTTPS, which a loopback server cannot
//! offer and should not need, so stdio is the supported transport for a local server
//! there. This test exists because that path has no other coverage.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

struct Stdio_ {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
    dir: std::path::PathBuf,
}

impl Stdio_ {
    fn start(tag: &str, project: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "palmier-stdio-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("project.json"), project).unwrap();

        let mut child = Command::new(env!("CARGO_BIN_EXE_palmier"))
            .args(["serve", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the binary must start");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
            dir,
        }
    }

    /// Send one message and, when it expects a reply, read exactly one back.
    fn send(
        &mut self,
        message: serde_json::Value,
        expect_reply: bool,
    ) -> Option<serde_json::Value> {
        writeln!(self.stdin, "{message}").expect("write");
        self.stdin.flush().expect("flush");
        if !expect_reply {
            return None;
        }
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("read");
        Some(serde_json::from_str(&line).unwrap_or_else(|e| panic!("bad frame `{line}`: {e}")))
    }

    fn initialize(&mut self) -> serde_json::Value {
        let reply = self
            .send(
                serde_json::json!({
                    "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": { "protocolVersion": "2025-06-18", "capabilities": {},
                                "clientInfo": { "name": "desktop", "version": "1" } }
                }),
                true,
            )
            .unwrap();
        self.send(
            serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            false,
        );
        reply
    }

    fn call(&mut self, name: &str, arguments: serde_json::Value) -> serde_json::Value {
        let reply = self
            .send(
                serde_json::json!({
                    "jsonrpc": "2.0", "id": 9, "method": "tools/call",
                    "params": { "name": name, "arguments": arguments }
                }),
                true,
            )
            .unwrap();
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text content: {reply}"));
        serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({ "raw": text }))
    }
}

impl Drop for Stdio_ {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const PROJECT: &str = r#"{"timelines":[{"id":"tl","name":"Test","fps":30,"width":1920,"height":1080,
  "tracks":[{"id":"v1","type":"video","clips":[
    {"id":"c1","mediaRef":"x","startFrame":0,"durationFrames":90}]}]}],"activeTimelineId":"tl"}"#;

#[test]
fn stdio_completes_a_handshake_and_lists_its_tools() {
    let mut server = Stdio_::start("hello", PROJECT);
    let hello = server.initialize();
    assert_eq!(hello["result"]["serverInfo"]["name"], "palmier");

    let listed = server
        .send(
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            true,
        )
        .unwrap();
    let tools = listed["result"]["tools"].as_array().expect("tools");
    assert!(tools.len() >= 15, "got {} tools", tools.len());
    assert!(tools.iter().any(|t| t["name"] == "get_timeline"));
}

#[test]
fn stdio_opens_and_edits_a_project_like_the_http_transport() {
    let mut server = Stdio_::start("edit", PROJECT);
    server.initialize();

    let path = server.dir.to_string_lossy().into_owned();
    let opened = server.call(
        "manage_project",
        serde_json::json!({ "action": "open", "path": path }),
    );
    assert_eq!(opened["status"], "open");

    let before = server.call("get_timeline", serde_json::json!({}));
    assert_eq!(before["totalFrames"], 90);

    let cut = server.call(
        "ripple_delete_ranges",
        serde_json::json!({ "ranges": [{ "startFrame": 0, "endFrame": 30 }] }),
    );
    assert_eq!(cut["status"], "applied");

    // Verify by reading state back, not by trusting the receipt.
    let after = server.call("get_timeline", serde_json::json!({}));
    assert_eq!(after["totalFrames"], 60);

    let undone = server.call("undo", serde_json::json!({}));
    assert_eq!(undone["status"], "undone");
    assert_eq!(
        server.call("get_timeline", serde_json::json!({}))["totalFrames"],
        90
    );
}

#[test]
fn stdout_carries_only_protocol_frames() {
    // A stray println! on stdout breaks the session. Every line must parse as JSON-RPC.
    let mut server = Stdio_::start("clean", PROJECT);
    server.initialize();
    let path = server.dir.to_string_lossy().into_owned();
    server.call(
        "manage_project",
        serde_json::json!({ "action": "open", "path": path }),
    );
    let reply = server
        .send(
            serde_json::json!({"jsonrpc":"2.0","id":7,"method":"tools/list"}),
            true,
        )
        .unwrap();
    assert_eq!(reply["jsonrpc"], "2.0");
    assert_eq!(reply["id"], 7);
}
