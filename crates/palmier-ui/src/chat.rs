//! The chat panel, driven by the Claude Code CLI.
//!
//! Not a second agent implementation: it spawns `claude` in print mode with this
//! server's own MCP endpoint configured, so the conversation in the window and the one
//! in a terminal reach the same 29 tools and the same open project. It also spends the
//! subscription the user already has rather than metering API credits per message.
//!
//! `claude` writes newline-delimited JSON: `system/init` carries the session id,
//! `assistant` carries content blocks, and `result` closes the turn. Those are relayed
//! to the browser as server-sent events.

use std::process::Stdio;

use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_stream::wrappers::ReceiverStream;

use crate::Ui;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ask {
    pub prompt: String,
    /// Continue an earlier conversation instead of starting one.
    #[serde(default)]
    pub session_id: Option<String>,
}

/// Is the CLI on PATH at all?
pub fn cli_available() -> bool {
    std::process::Command::new(claude_binary())
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn claude_binary() -> &'static str {
    "claude"
}

/// Only this project's own tools are pre-approved.
///
/// The panel is for editing video. Handing it Bash and Write as well would make a chat
/// box into a shell, which is not what anyone asked for by typing "trim the intro".
const ALLOWED: &str = "mcp__palmier";

pub async fn ask(State(ui): State<Ui>, Json(body): Json<Ask>) -> Response {
    if body.prompt.trim().is_empty() {
        return crate::problem("the message is empty");
    }
    if !cli_available() {
        return crate::problem(
            "Claude Code is not on PATH — install it, or talk to the project from a terminal",
        );
    }

    // A config file pointing the CLI back at this very server.
    let config = match write_mcp_config(ui.port) {
        Ok(path) => path,
        Err(error) => return crate::problem(error),
    };

    let mut command = Command::new(claude_binary());
    command
        .arg("--print")
        .args(["--output-format", "stream-json"])
        .arg("--verbose")
        .arg("--mcp-config")
        .arg(&config)
        .args(["--allowedTools", ALLOWED]);
    if let Some(session) = &body.session_id {
        command.args(["--resume", session]);
    }
    command
        .arg(&body.prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return crate::problem(format!("cannot start Claude Code: {error}")),
    };
    let Some(stdout) = child.stdout.take() else {
        return crate::problem("Claude Code produced no output stream");
    };
    let stderr = child.stderr.take();

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, std::convert::Infallible>>(64);

    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if let Some(event) = translate(&value)
                && tx
                    .send(Ok(Event::default().data(event.to_string())))
                    .await
                    .is_err()
            {
                // The browser went away; stop the run rather than let it finish alone.
                let _ = child.start_kill();
                return;
            }
        }

        // A failure to launch or authenticate shows up here, not on stdout.
        if let Some(stderr) = stderr {
            let mut lines = BufReader::new(stderr).lines();
            let mut collected = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                collected.push_str(&line);
                collected.push('\n');
            }
            if !collected.trim().is_empty() {
                let _ = tx
                    .send(Ok(Event::default().data(
                        json!({ "kind": "error", "text": collected.trim() }).to_string(),
                    )))
                    .await;
            }
        }
        let _ = child.wait().await;
        let _ = tx
            .send(Ok(
                Event::default().data(json!({ "kind": "end" }).to_string())
            ))
            .await;
        let _ = std::fs::remove_file(&config);
    });

    Sse::new(ReceiverStream::new(rx)).into_response()
}

/// Reduce a CLI event to what the panel needs to draw.
///
/// The raw stream carries a great deal the panel has no use for; passing it through
/// whole would make the browser responsible for understanding Claude Code's schema.
fn translate(value: &Value) -> Option<Value> {
    match value.get("type").and_then(Value::as_str)? {
        "system" if value.get("subtype").and_then(Value::as_str) == Some("init") => Some(json!({
            "kind": "start",
            "sessionId": value.get("session_id"),
            "model": value.get("model"),
            "mcpServers": value.get("mcp_servers"),
        })),
        "assistant" => {
            let blocks = value.pointer("/message/content")?.as_array()?;
            let mut text = String::new();
            let mut tools = Vec::new();
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        text.push_str(
                            block
                                .get("text")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                        );
                    }
                    Some("tool_use") => {
                        tools.push(json!({
                            "name": block.get("name"),
                            "input": block.get("input"),
                        }));
                    }
                    _ => {}
                }
            }
            if text.trim().is_empty() && tools.is_empty() {
                return None;
            }
            Some(json!({ "kind": "say", "text": text, "tools": tools }))
        }
        "result" => Some(json!({
            "kind": "done",
            "text": value.get("result"),
            "isError": value.get("is_error"),
            "turns": value.get("num_turns"),
            "costUsd": value.get("total_cost_usd"),
            "sessionId": value.get("session_id"),
        })),
        _ => None,
    }
}

fn write_mcp_config(port: u16) -> Result<std::path::PathBuf, String> {
    let dir = std::env::temp_dir().join("palmier-chat");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("mcp-{port}.json"));
    let config = json!({
        "mcpServers": {
            "palmier": { "type": "http", "url": format!("http://127.0.0.1:{port}/mcp") }
        }
    });
    std::fs::write(&path, config.to_string()).map_err(|e| e.to_string())?;
    Ok(path)
}
