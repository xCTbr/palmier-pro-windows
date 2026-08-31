//! Image generation through Google Stitch's MCP endpoint.
//!
//! Everything non-obvious here was learned the hard way in a working generator, not
//! guessed. Each one is a comment where it bites:
//!
//! - A project belongs to the **account that created it**, so a project made with one key
//!   is invisible to another. One project per key, created on demand.
//! - Failure is ambiguous — exhausted quota or a transient stumble — so keys are rotated
//!   before anything is retried, rather than hammering the one that just failed.
//! - Without an explicit instruction Stitch draws app furniture over the picture. The
//!   prompt has to ask for a screen that *is* the photograph.
//! - `downloadUrl` serves 512px unless you append `=s2048`.
//! - When the backend trips it answers with plain text where JSON is expected, and
//!   parsing that blindly hides the real message.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

use crate::keys::KeyRing;
use crate::{GenError, GeneratedImage};

const ENDPOINT: &str = "https://stitch.googleapis.com/mcp";
/// Generation is slow — minutes, not seconds — and a short timeout looks like a failure.
const GENERATE_TIMEOUT: Duration = Duration::from_secs(600);
const PROJECT_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Stitch {
    keys: KeyRing,
    http: reqwest::Client,
    /// A project id per key. Never shared: they are bound to the account that made them.
    projects: Mutex<HashMap<usize, String>>,
    /// The key that last worked. Rotation is sticky, not round-robin.
    current: Mutex<usize>,
    project_title: String,
    next_id: Mutex<u64>,
}

impl std::fmt::Debug for Stitch {
    /// Deliberately shallow: a key must not reach a log or a panic message.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stitch")
            .field("keys", &self.keys.len())
            .field("project_title", &self.project_title)
            .finish()
    }
}

impl Stitch {
    pub fn new(keys: KeyRing, project_title: impl Into<String>) -> Result<Self, GenError> {
        if keys.is_empty() {
            return Err(GenError::NoKeys { provider: "stitch" });
        }
        Ok(Self {
            keys,
            http: reqwest::Client::new(),
            projects: Mutex::new(HashMap::new()),
            current: Mutex::new(0),
            project_title: project_title.into(),
            next_id: Mutex::new(1),
        })
    }

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// One JSON-RPC `tools/call` against the MCP endpoint.
    ///
    /// No handshake and no session: Stitch answers a bare `tools/call` over plain HTTP.
    async fn call(
        &self,
        tool: &str,
        arguments: Value,
        timeout: Duration,
        key: &str,
    ) -> Result<Value, GenError> {
        let id = {
            let mut next = self.next_id.lock().expect("id counter");
            let id = *next;
            *next += 1;
            id
        };

        let response = self
            .http
            .post(ENDPOINT)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("x-goog-api-key", key)
            .timeout(timeout)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": { "name": tool, "arguments": arguments },
            }))
            .send()
            .await
            .map_err(|e| GenError::Transport(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| GenError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(GenError::Provider(format!(
                "HTTP {status}: {}",
                body.chars().take(200).collect::<String>()
            )));
        }

        let envelope: Value = serde_json::from_str(&body)
            .map_err(|_| GenError::Provider(format!("unreadable response: {}", head(&body))))?;
        if let Some(error) = envelope.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or(&error.to_string())
                .to_string();
            return Err(GenError::Provider(message));
        }

        let text = envelope
            .pointer("/result/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // The backend answers with plain prose when it stumbles ("Request contains an
        // invalid argument"). Parsing that blindly buries the reason under a JSON error.
        serde_json::from_str(text)
            .map_err(|_| GenError::Provider(format!("non-JSON reply: {}", head(text))))
    }

    /// The project belonging to `index`, created the first time that key is used.
    async fn project_for(&self, index: usize, key: &str) -> Result<String, GenError> {
        if let Some(id) = self.projects.lock().expect("projects").get(&index) {
            return Ok(id.clone());
        }
        let created = self
            .call(
                "create_project",
                json!({ "title": self.project_title }),
                PROJECT_TIMEOUT,
                key,
            )
            .await?;
        let name = created
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| GenError::Provider("create_project returned no name".into()))?;
        let id = name.trim_start_matches("projects/").to_string();
        self.projects
            .lock()
            .expect("projects")
            .insert(index, id.clone());
        Ok(id)
    }

    async fn generate_once(&self, prompt: &str, index: usize) -> Result<GeneratedImage, GenError> {
        let key = self
            .keys
            .get(index)
            .ok_or(GenError::NoKeys { provider: "stitch" })?
            .to_string();
        let project_id = self.project_for(index, &key).await?;

        let result = self
            .call(
                "generate_screen_from_text",
                json!({
                    "projectId": project_id,
                    "prompt": photo_prompt(prompt),
                    "deviceType": "DESKTOP",
                }),
                GENERATE_TIMEOUT,
                &key,
            )
            .await?;

        let screen = result
            .pointer("/outputComponents/0/design/screens/0")
            .ok_or_else(|| GenError::Provider("no screen in the reply".into()))?;
        let url = screen
            .pointer("/screenshot/downloadUrl")
            .and_then(Value::as_str)
            .ok_or_else(|| GenError::Provider("no downloadUrl in the reply".into()))?;

        // The bare URL serves 512px; the suffix asks for the native size.
        let bytes = self
            .http
            .get(format!("{url}=s2048"))
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await
            .map_err(|e| GenError::Transport(e.to_string()))?
            .error_for_status()
            .map_err(|e| GenError::Transport(e.to_string()))?
            .bytes()
            .await
            .map_err(|e| GenError::Transport(e.to_string()))?
            .to_vec();

        if !is_jpeg(&bytes) {
            return Err(GenError::Provider("the download is not a JPEG".into()));
        }
        Ok(GeneratedImage {
            bytes,
            width: screen.get("width").and_then(Value::as_i64),
            height: screen.get("height").and_then(Value::as_i64),
            extension: "jpg",
        })
    }

    /// Generate, rotating keys on failure.
    ///
    /// A failure may be exhausted quota, which only another account fixes, or a transient
    /// stumble, which the same prompt survives on a second pass. Rotating through every
    /// key before sleeping covers both without hammering the one that just failed.
    pub async fn generate(&self, prompt: &str, rounds: usize) -> Result<GeneratedImage, GenError> {
        let count = self.keys.len();
        let start = *self.current.lock().expect("current");
        let mut last: Option<GenError> = None;

        for round in 1..=rounds.max(1) {
            for step in 0..count {
                let index = (start + step) % count;
                match self.generate_once(prompt, index).await {
                    Ok(image) => {
                        // Stay on the key that worked.
                        *self.current.lock().expect("current") = index;
                        return Ok(image);
                    }
                    Err(error) => last = Some(error),
                }
            }
            tokio::time::sleep(Duration::from_millis(4000 * round as u64)).await;
        }
        Err(last.unwrap_or(GenError::NoKeys { provider: "stitch" }))
    }
}

/// Ask for a screen that *is* the photograph.
///
/// Without this Stitch returns app layout over the image — it is a UI design tool, and
/// the instruction is what makes it mark the screen as a plain image instead.
pub fn photo_prompt(description: &str) -> String {
    format!(
        "{description}\n\n\
         The screen must contain ONLY this single photograph, edge to edge. \
         No UI, no navigation, no buttons, no captions and no text of any kind over the image."
    )
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() > 2 && bytes[0] == 0xFF && bytes[1] == 0xD8
}

fn head(text: &str) -> String {
    text.chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_forbids_interface_furniture() {
        let prompt = photo_prompt("a bowl of soup");
        assert!(prompt.starts_with("a bowl of soup"));
        assert!(prompt.contains("ONLY this single photograph"));
        assert!(prompt.contains("No UI"));
    }

    #[test]
    fn jpeg_is_recognised_by_its_magic_bytes() {
        assert!(is_jpeg(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!is_jpeg(&[0x89, 0x50, 0x4E, 0x47]));
        assert!(!is_jpeg(&[]));
    }

    #[test]
    fn no_keys_is_refused_at_construction() {
        let error = Stitch::new(KeyRing::default(), "test").unwrap_err();
        assert!(matches!(error, GenError::NoKeys { .. }));
    }
}
