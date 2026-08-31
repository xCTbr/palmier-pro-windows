//! Drive a real server over HTTP and verify every outcome by reading state back.
//! A success response is never accepted as proof that something happened (SC-001).

use std::net::{Ipv4Addr, SocketAddr};

use serde_json::{Value, json};

/// A live server on an ephemeral port, plus an initialized MCP session.
struct Client {
    base: String,
    session: String,
    http: reqwest::Client,
    dir: std::path::PathBuf,
}

impl Client {
    async fn start(fixture: &str, tag: &str) -> Self {
        // Unique directory per test: the suite runs in parallel.
        let dir = std::env::temp_dir().join(format!(
            "palmier-mcp-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("project.json"), fixture).unwrap();

        // Port 0 lets the OS pick, so parallel tests never collide.
        let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, palmier_mcp::http_router()).await;
        });

        let http = reqwest::Client::new();
        let base = format!("http://{address}/mcp");
        let mut client = Self {
            base,
            session: String::new(),
            http,
            dir,
        };
        client.initialize().await;
        client
    }

    async fn post(&self, body: Value) -> (Option<String>, String) {
        let mut request = self
            .http
            .post(&self.base)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if !self.session.is_empty() {
            request = request.header("mcp-session-id", &self.session);
        }
        let response = request.json(&body).send().await.expect("request failed");
        let id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        (id, response.text().await.unwrap_or_default())
    }

    async fn initialize(&mut self) {
        let (id, _) = self
            .post(json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": { "name": "test", "version": "1" }
                }
            }))
            .await;
        self.session = id.expect("server must issue a session id");
        self.post(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await;
    }

    /// Parse the SSE body down to the JSON-RPC envelope.
    fn envelope(body: &str) -> Value {
        for line in body.lines() {
            if let Some(payload) = line.strip_prefix("data: ")
                && let Ok(value) = serde_json::from_str::<Value>(payload)
                && (value.get("result").is_some() || value.get("error").is_some())
            {
                return value;
            }
        }
        panic!("no JSON-RPC result in body: {body}");
    }

    async fn list_tools(&self) -> Vec<String> {
        let (_, body) = self
            .post(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
            .await;
        Self::envelope(&body)["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .map(|t| t["name"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    /// Call a tool and parse its JSON payload.
    async fn call(&self, name: &str, arguments: Value) -> Value {
        let (_, body) = self
            .post(json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": name, "arguments": arguments }
            }))
            .await;
        let envelope = Self::envelope(&body);
        let text = envelope["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text content: {envelope}"));
        serde_json::from_str(text).unwrap_or_else(|_| json!({ "raw": text }))
    }

    async fn open(&self) -> Value {
        self.call(
            "manage_project",
            json!({ "action": "open", "path": self.dir.to_str().unwrap() }),
        )
        .await
    }

    async fn timeline(&self) -> Value {
        self.call("get_timeline", json!({})).await
    }
}

const FIXTURE: &str = r#"{"timelines":[{"id":"tl","name":"Main","fps":30,"width":1920,"height":1080,
  "tracks":[
    {"id":"v1","type":"video","clips":[
      {"id":"a","mediaRef":"m","startFrame":0,"durationFrames":30},
      {"id":"b","mediaRef":"m","startFrame":30,"durationFrames":30},
      {"id":"c","mediaRef":"m","startFrame":60,"durationFrames":30}]},
    {"id":"a1","type":"audio","clips":[
      {"id":"d","mediaRef":"m","mediaType":"audio","startFrame":0,"durationFrames":90}]}],
  "markers":[{"id":"mk","name":"note","startFrame":75,"durationFrames":0,
              "color":{"r":1,"g":1,"b":1,"a":1},"comment":""}]}],
  "activeTimelineId":"tl"}"#;

#[tokio::test]
async fn the_server_lists_its_tools() {
    let client = Client::start(FIXTURE, "list").await;
    let tools = client.list_tools().await;
    for expected in [
        "manage_project",
        "get_timeline",
        "manage_tracks",
        "add_clips",
        "move_clips",
        "remove_clips",
        "split_clips",
        "ripple_delete_ranges",
        "set_clip_properties",
        "trim_clip",
        "undo",
    ] {
        assert!(
            tools.contains(&expected.to_string()),
            "missing `{expected}` in {tools:?}"
        );
    }
}

#[tokio::test]
async fn get_timeline_reports_frames_ids_and_gaps() {
    let client = Client::start(FIXTURE, "read").await;
    client.open().await;
    let tl = client.timeline().await;

    assert_eq!(tl["fps"], 30);
    assert_eq!(tl["totalFrames"], 90);
    assert_eq!(tl["durationSeconds"], 3.0);
    let video = &tl["tracks"][0];
    assert_eq!(video["trackId"], "v1");
    assert_eq!(video["type"], "video");
    let first = &video["clips"][0];
    assert_eq!(first["startFrame"], 0);
    assert_eq!(first["endFrame"], 30, "end is exclusive");
    assert_eq!(first["durationFrames"], 30);
    assert!(first.get("speed").is_none(), "defaults are omitted");
    assert_eq!(tl["markers"][0]["markerId"], "mk");
}

#[tokio::test]
async fn a_window_limits_clips_and_reports_the_total() {
    let client = Client::start(FIXTURE, "window").await;
    client.open().await;
    let tl = client
        .call("get_timeline", json!({ "startFrame": 0, "endFrame": 31 }))
        .await;
    let video = &tl["tracks"][0];
    assert_eq!(
        video["clips"].as_array().unwrap().len(),
        2,
        "clips intersecting [0,31)"
    );
    assert_eq!(video["totalClips"], 3, "the caller is told some are hidden");
}

#[tokio::test]
async fn an_inverted_window_is_rejected() {
    let client = Client::start(FIXTURE, "badwindow").await;
    client.open().await;
    let out = client
        .call("get_timeline", json!({ "startFrame": 50, "endFrame": 10 }))
        .await;
    assert_eq!(
        out["status"], "refused",
        "a well-formed but impossible call is a refusal"
    );
    assert!(
        out["reason"].as_str().unwrap().contains("greater than"),
        "{out}"
    );
}

#[tokio::test]
async fn adding_a_clip_is_visible_when_the_timeline_is_read_back() {
    let client = Client::start(FIXTURE, "add").await;
    client.open().await;
    let receipt = client
        .call(
            "add_clips",
            json!({ "entries": [{ "mediaRef": "new", "trackId": "v1",
                                  "startFrame": 200, "endFrame": 260 }] }),
        )
        .await;
    assert_eq!(receipt["status"], "applied");

    let tl = client.timeline().await;
    let clips = tl["tracks"][0]["clips"].as_array().unwrap();
    let added = clips
        .iter()
        .find(|c| c["startFrame"] == 200)
        .expect("clip not in the timeline");
    assert_eq!(added["endFrame"], 260);
}

#[tokio::test]
async fn a_ripple_delete_closes_the_gap_and_moves_markers() {
    let client = Client::start(FIXTURE, "ripple").await;
    client.open().await;
    let before = client.timeline().await;
    assert_eq!(before["totalFrames"], 90);

    let receipt = client
        .call(
            "ripple_delete_ranges",
            json!({ "ranges": [{ "startFrame": 30, "endFrame": 60 }] }),
        )
        .await;
    assert_eq!(receipt["status"], "applied");
    assert_eq!(receipt["markersChanged"], true);

    let after = client.timeline().await;
    assert_eq!(after["totalFrames"], 60, "the gap closed");
    assert_eq!(
        after["markers"][0]["startFrame"], 45,
        "the marker moved with its content"
    );
}

#[tokio::test]
async fn undo_restores_what_the_timeline_reports() {
    let client = Client::start(FIXTURE, "undo").await;
    client.open().await;
    let before = client.timeline().await;

    client
        .call(
            "ripple_delete_ranges",
            json!({ "ranges": [{ "startFrame": 0, "endFrame": 30 }] }),
        )
        .await;
    assert_ne!(client.timeline().await, before);

    let undone = client.call("undo", json!({})).await;
    assert_eq!(undone["status"], "undone");
    assert_eq!(
        client.timeline().await,
        before,
        "state must match exactly, not approximately"
    );
}

#[tokio::test]
async fn redo_reapplies_what_undo_reverted() {
    let client = Client::start(FIXTURE, "redo").await;
    client.open().await;
    client
        .call("remove_clips", json!({ "clipIds": ["b"], "ripple": true }))
        .await;
    let after = client.timeline().await;
    client.call("undo", json!({})).await;
    let redone = client.call("undo", json!({ "redo": true })).await;
    assert_eq!(redone["status"], "redone");
    assert_eq!(client.timeline().await, after);
}

#[tokio::test]
async fn undo_with_nothing_to_undo_reports_a_no_op() {
    let client = Client::start(FIXTURE, "undonothing").await;
    client.open().await;
    let out = client.call("undo", json!({})).await;
    assert_eq!(out["status"], "no_op");
    assert_eq!(out["detail"], "nothing to undo");
}

#[tokio::test]
async fn a_refusal_says_why_and_changes_nothing() {
    let client = Client::start(FIXTURE, "refuse").await;
    client.open().await;
    let before = client.timeline().await;

    let out = client
        .call(
            "move_clips",
            json!({ "moves": [{ "clipId": "ghost", "toTrackId": "v1", "toFrame": 0 }] }),
        )
        .await;
    assert_eq!(out["status"], "refused");
    assert!(out["reason"].as_str().unwrap().contains("ghost"), "{out}");
    assert_eq!(client.timeline().await, before, "a refusal must not mutate");
}

#[tokio::test]
async fn a_partly_invalid_batch_is_refused_whole() {
    let client = Client::start(FIXTURE, "atomic").await;
    client.open().await;
    let before = client.timeline().await;

    let out = client
        .call(
            "move_clips",
            json!({ "moves": [
                { "clipId": "a", "toTrackId": "v1", "toFrame": 500 },
                { "clipId": "ghost", "toTrackId": "v1", "toFrame": 0 }
            ]}),
        )
        .await;
    assert_eq!(out["status"], "refused");
    assert_eq!(
        client.timeline().await,
        before,
        "the valid move must not have landed"
    );
}

#[tokio::test]
async fn a_no_op_is_reported_as_a_no_op_not_a_success() {
    let client = Client::start(FIXTURE, "noop").await;
    client.open().await;
    let out = client
        .call(
            "split_clips",
            json!({ "points": [{ "trackId": "v1", "atFrame": 0 }] }),
        )
        .await;
    assert_eq!(out["status"], "no_op");
}

#[tokio::test]
async fn a_track_index_is_accepted_and_resolved_to_an_id() {
    let client = Client::start(FIXTURE, "index").await;
    client.open().await;
    let receipt = client
        .call(
            "add_clips",
            json!({ "entries": [{ "mediaRef": "m", "trackIndex": 0,
                                  "startFrame": 300, "endFrame": 330 }] }),
        )
        .await;
    assert_eq!(receipt["status"], "applied");
    let tl = client.timeline().await;
    assert!(
        tl["tracks"][0]["clips"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["startFrame"] == 300)
    );
}

#[tokio::test]
async fn tracks_can_be_added_and_removed() {
    let client = Client::start(FIXTURE, "tracks").await;
    client.open().await;
    let added = client
        .call(
            "manage_tracks",
            json!({ "action": "add", "trackType": "audio" }),
        )
        .await;
    let new_id = added["createdTrackIds"][0].as_str().unwrap().to_string();

    let tl = client.timeline().await;
    assert_eq!(tl["tracks"].as_array().unwrap().len(), 3);

    let removed = client
        .call(
            "manage_tracks",
            json!({ "action": "remove", "trackId": new_id }),
        )
        .await;
    assert_eq!(removed["status"], "applied");
    assert_eq!(
        client.timeline().await["tracks"].as_array().unwrap().len(),
        2
    );
}

#[tokio::test]
async fn a_track_with_clips_refuses_removal() {
    let client = Client::start(FIXTURE, "trackbusy").await;
    client.open().await;
    let out = client
        .call(
            "manage_tracks",
            json!({ "action": "remove", "trackId": "v1" }),
        )
        .await;
    assert_eq!(out["status"], "refused");
    assert!(out["reason"].as_str().unwrap().contains("clips"), "{out}");
}

#[tokio::test]
async fn editing_never_writes_to_disk_until_asked() {
    let client = Client::start(FIXTURE, "save").await;
    client.open().await;
    let path = client.dir.join("project.json");
    let original = std::fs::read_to_string(&path).unwrap();

    client
        .call(
            "ripple_delete_ranges",
            json!({ "ranges": [{ "startFrame": 0, "endFrame": 30 }] }),
        )
        .await;
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "an edit must not touch disk"
    );

    let saved = client
        .call("manage_project", json!({ "action": "save" }))
        .await;
    assert_eq!(saved["status"], "saved");
    assert_ne!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "save must write"
    );
}

#[tokio::test]
async fn tools_refuse_before_a_project_is_open() {
    let client = Client::start(FIXTURE, "unopened").await;
    let out = client.call("get_timeline", json!({})).await;
    assert_eq!(out["status"], "refused");
    assert!(
        out["reason"]
            .as_str()
            .unwrap()
            .contains("no project is open"),
        "{out}"
    );
}

#[tokio::test]
async fn hostile_arguments_do_not_crash_the_server() {
    let client = Client::start(FIXTURE, "hostile").await;
    client.open().await;
    let hostile = "x".repeat(4096);
    for arguments in [
        json!({ "clipIds": [hostile.clone()] }),
        json!({ "clipIds": ["\u{1F3AC}\nnewline\ttab"] }),
        json!({ "clipIds": [] }),
        json!({ "clipIds": ["a"], "speed": -1.0 }),
        json!({ "clipIds": ["a"], "opacity": 1e308 }),
    ] {
        let _ = client.call("set_clip_properties", arguments).await;
    }
    // The server is still alive and consistent.
    assert_eq!(client.timeline().await["totalFrames"], 90);
}

#[tokio::test]
async fn responses_carry_no_internal_type_names() {
    let client = Client::start(FIXTURE, "vocab").await;
    client.open().await;
    let text = client.timeline().await.to_string();
    // `sourceClipType` is a contract field name, so match internal identifiers only.
    for leak in [
        "EditCommand",
        "RefusalReason",
        "InversePatch",
        "EditSession",
        "palmier_core",
    ] {
        assert!(!text.contains(leak), "response leaks `{leak}`");
    }
}

/// SC-005: the daemon must never be reachable from the network.
#[tokio::test]
async fn the_listener_binds_loopback_only() {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
        .await
        .unwrap();
    let bound = listener.local_addr().unwrap();
    assert!(
        bound.ip().is_loopback(),
        "bound to {bound}, which is routable"
    );

    // And the daemon's own binding, as `palmier serve` constructs it.
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, palmier_mcp::DEFAULT_PORT));
    assert!(address.ip().is_loopback());
    assert_eq!(palmier_mcp::DEFAULT_PORT, 19789, "the original's port");
}

// ---------------------------------------------------- media and export

fn ffmpeg_available() -> bool {
    palmier_media::require_tool("ffmpeg").is_ok() && palmier_media::require_tool("ffprobe").is_ok()
}

/// A short synthetic source file inside the client's working directory.
fn make_source(dir: &std::path::Path, name: &str, pattern: &str) -> String {
    let path = dir.join(name);
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y"])
        .args([
            "-f",
            "lavfi",
            "-i",
            &format!("{pattern}=size=320x240:rate=30:duration=2"),
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
        .args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&path)
        .output()
        .expect("ffmpeg must run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    path.to_string_lossy().into_owned()
}

const EMPTY: &str = r#"{"timelines":[{"id":"tl","name":"Cut","fps":30,"width":640,"height":360,
  "tracks":[{"id":"v1","type":"video","clips":[]}]}],"activeTimelineId":"tl"}"#;

#[tokio::test]
async fn importing_probes_the_file_and_returns_a_usable_ref() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let client = Client::start(EMPTY, "import").await;
    client.open().await;
    let source = make_source(&client.dir, "take.mp4", "testsrc");

    let out = client
        .call("import_media", json!({ "paths": [source] }))
        .await;
    assert_eq!(out["status"], "imported");
    let entry = &out["media"][0];
    assert_eq!(entry["name"], "take.mp4");
    assert_eq!(entry["width"], 320);
    assert_eq!(entry["hasAudio"], true);
    assert!((entry["durationSeconds"].as_f64().unwrap() - 2.0).abs() < 0.1);

    // The ref works: a clip placed with it lands on the timeline.
    let media_ref = entry["mediaRef"].as_str().unwrap();
    let placed = client
        .call(
            "add_clips",
            json!({ "entries": [{ "mediaRef": media_ref, "trackId": "v1",
                                  "startFrame": 0, "endFrame": 30 }] }),
        )
        .await;
    assert_eq!(placed["status"], "applied");
}

#[tokio::test]
async fn one_unreadable_file_rejects_the_whole_import() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let client = Client::start(EMPTY, "importbad").await;
    client.open().await;
    let good = make_source(&client.dir, "good.mp4", "testsrc");
    let junk = client.dir.join("junk.mp4");
    std::fs::write(&junk, b"not a video").unwrap();

    let out = client
        .call(
            "import_media",
            json!({ "paths": [good, junk.to_string_lossy()] }),
        )
        .await;
    assert_eq!(out["status"], "refused");

    // No partial state: the good file was not recorded either.
    let media = client.call("get_media", json!({})).await;
    assert!(media["media"].as_array().unwrap().is_empty(), "{media}");
}

#[tokio::test]
async fn get_media_reports_whether_a_file_is_still_there() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let client = Client::start(EMPTY, "getmedia").await;
    client.open().await;
    let source = make_source(&client.dir, "take.mp4", "testsrc");
    client
        .call("import_media", json!({ "paths": [&source] }))
        .await;

    let listed = client.call("get_media", json!({})).await;
    assert_eq!(listed["media"][0]["resolved"], true);

    std::fs::remove_file(&source).unwrap();
    let after = client.call("get_media", json!({})).await;
    assert_eq!(
        after["media"][0]["resolved"], false,
        "a vanished file must be reported"
    );
}

#[tokio::test]
async fn exporting_produces_a_file_the_timeline_describes() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let client = Client::start(EMPTY, "export").await;
    client.open().await;
    let a = make_source(&client.dir, "a.mp4", "testsrc");
    let b = make_source(&client.dir, "b.mp4", "testsrc2");
    let imported = client
        .call("import_media", json!({ "paths": [a, b] }))
        .await;
    let (ra, rb) = (
        imported["media"][0]["mediaRef"]
            .as_str()
            .unwrap()
            .to_string(),
        imported["media"][1]["mediaRef"]
            .as_str()
            .unwrap()
            .to_string(),
    );

    client
        .call(
            "add_clips",
            json!({ "entries": [
                { "mediaRef": ra, "trackId": "v1", "startFrame": 0, "endFrame": 30 },
                { "mediaRef": rb, "trackId": "v1", "startFrame": 30, "endFrame": 60 }
            ]}),
        )
        .await;

    let output = client.dir.join("out.mp4");
    let out = client
        .call(
            "export_project",
            json!({ "output": output.to_string_lossy() }),
        )
        .await;
    assert_eq!(out["status"], "exported", "{out}");
    assert_eq!(out["durationSeconds"], 2.0, "60 frames at 30fps");
    assert_eq!(out["width"], 640);

    let info = palmier_media::probe(&output).expect("the export must be a real video");
    assert!(
        (info.duration_seconds - 2.0).abs() < 0.2,
        "got {}s",
        info.duration_seconds
    );
    assert_eq!(info.width, Some(640));
}

#[tokio::test]
async fn exporting_reports_media_that_vanished_rather_than_hiding_it() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let client = Client::start(EMPTY, "exportmissing").await;
    client.open().await;
    let a = make_source(&client.dir, "a.mp4", "testsrc");
    let b = make_source(&client.dir, "b.mp4", "testsrc2");
    let imported = client
        .call("import_media", json!({ "paths": [&a, &b] }))
        .await;
    let (ra, rb) = (
        imported["media"][0]["mediaRef"]
            .as_str()
            .unwrap()
            .to_string(),
        imported["media"][1]["mediaRef"]
            .as_str()
            .unwrap()
            .to_string(),
    );
    client
        .call(
            "add_clips",
            json!({ "entries": [
                { "mediaRef": ra, "trackId": "v1", "startFrame": 0, "endFrame": 30 },
                { "mediaRef": rb, "trackId": "v1", "startFrame": 30, "endFrame": 60 }
            ]}),
        )
        .await;
    std::fs::remove_file(&b).unwrap();

    let out = client
        .call(
            "export_project",
            json!({ "output": client.dir.join("out.mp4").to_string_lossy() }),
        )
        .await;
    assert_eq!(out["status"], "exported");
    assert_eq!(
        out["missingMedia"],
        json!([rb]),
        "a shorter film must never be silent about why"
    );
}

#[tokio::test]
async fn exporting_an_empty_timeline_is_refused() {
    let client = Client::start(EMPTY, "exportempty").await;
    client.open().await;
    let out = client
        .call(
            "export_project",
            json!({ "output": client.dir.join("out.mp4").to_string_lossy() }),
        )
        .await;
    assert_eq!(out["status"], "refused");
    assert!(out["reason"].as_str().unwrap().contains("empty"), "{out}");
}

#[tokio::test]
async fn an_unsupported_codec_or_crf_is_refused() {
    let client = Client::start(EMPTY, "exportbad").await;
    client.open().await;
    let output = client.dir.join("out.mp4").to_string_lossy().into_owned();
    let bad_codec = client
        .call(
            "export_project",
            json!({ "output": &output, "codec": "prores" }),
        )
        .await;
    assert_eq!(bad_codec["status"], "refused");
    let bad_crf = client
        .call("export_project", json!({ "output": &output, "crf": 99 }))
        .await;
    assert_eq!(bad_crf["status"], "refused");
}

// ------------------------------------------------------ inspect_timeline

impl Client {
    /// Raw content blocks, for tools that return more than one.
    async fn call_blocks(&self, name: &str, arguments: Value) -> Vec<Value> {
        let (_, body) = self
            .post(json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": { "name": name, "arguments": arguments }
            }))
            .await;
        Self::envelope(&body)["result"]["content"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

/// A timeline with two visually distinct clips back to back.
async fn two_shot_client(tag: &str) -> (Client, String, String) {
    let client = Client::start(EMPTY, tag).await;
    client.open().await;
    let a = make_source(&client.dir, "a.mp4", "testsrc");
    let b = make_source(&client.dir, "b.mp4", "smptebars");
    let imported = client
        .call("import_media", json!({ "paths": [a, b] }))
        .await;
    let (ra, rb) = (
        imported["media"][0]["mediaRef"]
            .as_str()
            .unwrap()
            .to_string(),
        imported["media"][1]["mediaRef"]
            .as_str()
            .unwrap()
            .to_string(),
    );
    client
        .call(
            "add_clips",
            json!({ "entries": [
                { "mediaRef": ra, "trackId": "v1", "startFrame": 0, "endFrame": 30 },
                { "mediaRef": rb, "trackId": "v1", "startFrame": 30, "endFrame": 60 }
            ]}),
        )
        .await;
    (client, ra, rb)
}

fn decode_png(block: &Value) -> Vec<u8> {
    assert_eq!(block["type"], "image");
    assert_eq!(block["mimeType"], "image/png");
    let data = block["data"].as_str().expect("base64 payload");
    let bytes = base64_decode(data);
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    bytes
}

fn png_size(bytes: &[u8]) -> (u32, u32) {
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (w, h)
}

fn base64_decode(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (i, c) in ALPHABET.iter().enumerate() {
        lookup[*c as usize] = i as u8;
    }
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    for byte in text.bytes() {
        if byte == b'=' {
            break;
        }
        let value = lookup[byte as usize];
        if value == 255 {
            continue;
        }
        acc = (acc << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

#[tokio::test]
async fn inspecting_returns_a_real_image_paired_with_its_metadata() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let (client, first_ref, _) = two_shot_client("inspect").await;
    let clips = client.timeline().await["tracks"][0]["clips"].clone();
    let first_id = clips[0]["id"].as_str().unwrap().to_string();
    let _ = first_ref;

    let blocks = client
        .call_blocks("inspect_timeline", json!({ "startFrame": 5 }))
        .await;
    assert_eq!(blocks.len(), 2, "one text block and one image");

    let meta: Value = serde_json::from_str(blocks[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(meta["frame"], 5);
    assert_eq!(meta["canvas"]["width"], 640);
    assert_eq!(
        meta["visibleClipIds"],
        json!([first_id]),
        "the metadata maps the picture back to the clip that made it"
    );

    let png = decode_png(&blocks[1]);
    let (w, h) = png_size(&png);
    assert_eq!(w, 640, "downscaled to the readable width");
    assert!(h > 0);
}

#[tokio::test]
async fn sampling_a_range_returns_one_pair_per_frame_and_the_picture_changes() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let (client, _, _) = two_shot_client("inspectrange").await;
    let blocks = client
        .call_blocks(
            "inspect_timeline",
            json!({ "startFrame": 0, "endFrame": 60, "maxFrames": 3 }),
        )
        .await;
    assert_eq!(
        blocks.len(),
        6,
        "three frames, each a text block plus an image"
    );

    let frames: Vec<i64> = blocks
        .iter()
        .step_by(2)
        .map(|b| {
            serde_json::from_str::<Value>(b["text"].as_str().unwrap()).unwrap()["frame"]
                .as_i64()
                .unwrap()
        })
        .collect();
    assert_eq!(frames, vec![0, 20, 40], "samples spread across the range");

    // The last sample is in the second clip, so it must not look like the first.
    let first = decode_png(&blocks[1]);
    let last = decode_png(&blocks[5]);
    assert_ne!(
        first, last,
        "the two shots render identically — the cut is invisible"
    );
}

#[tokio::test]
async fn the_visible_clip_list_follows_the_cut() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let (client, _, _) = two_shot_client("inspectvisible").await;
    let clips = client.timeline().await["tracks"][0]["clips"].clone();
    let (a, b) = (
        clips[0]["id"].as_str().unwrap().to_string(),
        clips[1]["id"].as_str().unwrap().to_string(),
    );

    let before = client
        .call_blocks("inspect_timeline", json!({ "startFrame": 10 }))
        .await;
    let after = client
        .call_blocks("inspect_timeline", json!({ "startFrame": 40 }))
        .await;
    let meta = |blocks: &[Value]| -> Value {
        serde_json::from_str(blocks[0]["text"].as_str().unwrap()).unwrap()
    };
    assert_eq!(meta(&before)["visibleClipIds"], json!([a]));
    assert_eq!(meta(&after)["visibleClipIds"], json!([b]));
}

#[tokio::test]
async fn a_frame_past_the_end_renders_black_rather_than_failing() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let (client, _, _) = two_shot_client("inspectpast").await;
    let blocks = client
        .call_blocks("inspect_timeline", json!({ "startFrame": 5000 }))
        .await;
    let meta: Value = serde_json::from_str(blocks[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        meta["visibleClipIds"],
        json!([]),
        "nothing is on screen there"
    );
    assert!(!decode_png(&blocks[1]).is_empty());
}

#[tokio::test]
async fn inspecting_refuses_a_bad_range_or_a_closed_project() {
    let closed = Client::start(EMPTY, "inspectclosed").await;
    let out = closed.call("inspect_timeline", json!({})).await;
    assert_eq!(out["status"], "refused");

    let open = Client::start(EMPTY, "inspectbadrange").await;
    open.open().await;
    for arguments in [
        json!({ "startFrame": -5 }),
        json!({ "startFrame": 50, "endFrame": 10 }),
    ] {
        let out = open.call("inspect_timeline", arguments).await;
        assert_eq!(out["status"], "refused", "{out}");
    }
}

// ------------------------------------------- bootstrap, silence, packaging
//
// Everything below came from one real editing session. An agent cut the dead air out of
// a talking-head recording and reported what it had to do outside the tool to manage it:
// detect silence with ffmpeg by hand, hand-write a `.palmier` because nothing could
// create one, and convert seconds to frames itself.

/// A source that speaks, pauses, speaks, pauses, speaks.
fn make_speech_with_gaps(dir: &std::path::Path, name: &str) -> String {
    let path = dir.join(name);
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y"])
        .args(["-f", "lavfi", "-i", "sine=frequency=300:duration=2"])
        .args(["-f", "lavfi", "-i", "anullsrc=duration=1.5"])
        .args(["-f", "lavfi", "-i", "sine=frequency=400:duration=2"])
        .args(["-f", "lavfi", "-i", "anullsrc=duration=1.5"])
        .args(["-f", "lavfi", "-i", "sine=frequency=500:duration=2"])
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=30:duration=9",
        ])
        .args([
            "-filter_complex",
            "[0:a][1:a][2:a][3:a][4:a]concat=n=5:v=0:a=1[a]",
        ])
        .args(["-map", "[a]", "-map", "5:v"])
        .args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&path)
        .output()
        .expect("ffmpeg must run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    path.to_string_lossy().into_owned()
}

#[tokio::test]
async fn a_project_can_be_created_from_nothing() {
    // Without this the whole "edit my video" flow was unreachable: every tool needs an
    // open project, and the only way to get one was to hand-write a .palmier first.
    let client = Client::start(EMPTY, "create").await;
    let created = client
        .call(
            "manage_project",
            json!({ "action": "create", "fps": 24, "width": 1280, "height": 720 }),
        )
        .await;
    assert_eq!(created["status"], "created");
    assert_eq!(created["fps"], 24);
    assert_eq!(
        created["path"],
        Value::Null,
        "nothing is written until you save"
    );

    let tracks = created["tracks"].as_array().unwrap();
    assert_eq!(
        tracks.len(),
        2,
        "a new project is usable immediately: one video, one audio"
    );
    assert_eq!(tracks[0]["type"], "video");
    assert_eq!(tracks[1]["type"], "audio");

    // And it is genuinely open: the next tool works without an `open` call.
    let timeline = client.timeline().await;
    assert_eq!(timeline["fps"], 24);
    assert_eq!(timeline["totalFrames"], 0);
}

#[tokio::test]
async fn creating_with_a_path_writes_a_package_directory() {
    let client = Client::start(EMPTY, "createat").await;
    let target = client.dir.join("new.palmier");
    let created = client
        .call(
            "manage_project",
            json!({ "action": "create", "path": target.to_string_lossy() }),
        )
        .await;
    assert_eq!(created["status"], "created");

    // A `.palmier` is a folder holding project.json — not a file named `.palmier`.
    assert!(
        target.is_dir(),
        "expected a package directory at {}",
        target.display()
    );
    assert!(target.join("project.json").is_file());
}

#[tokio::test]
async fn invalid_creation_settings_are_rejected() {
    let client = Client::start(EMPTY, "createbad").await;
    for bad in [
        json!({ "action": "create", "fps": 0 }),
        json!({ "action": "create", "fps": -30 }),
        json!({ "action": "create", "width": 1 }),
        json!({ "action": "create", "height": 0 }),
    ] {
        let out = client.call("manage_project", bad.clone()).await;
        assert!(out.to_string().contains("invalid"), "{bad} gave {out}");
    }
}

#[tokio::test]
async fn silence_is_reported_in_frames_ready_to_cut_with() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let client = Client::start(EMPTY, "silence").await;
    client
        .call("manage_project", json!({ "action": "create", "fps": 30 }))
        .await;
    let source = make_speech_with_gaps(&client.dir, "speech.mp4");
    let imported = client
        .call("import_media", json!({ "paths": [source] }))
        .await;
    let media_ref = imported["media"][0]["mediaRef"]
        .as_str()
        .unwrap()
        .to_string();

    let found = client
        .call("detect_silence", json!({ "mediaRef": media_ref }))
        .await;
    assert_eq!(found["fps"], 30);
    let silences = found["silences"].as_array().unwrap();
    assert_eq!(silences.len(), 2, "two pauses were recorded: {found}");
    assert_eq!(
        found["speech"].as_array().unwrap().len(),
        3,
        "three spans of sound"
    );

    // The padding keeps a cut off the edge of a word, so a 1.5s pause is reported as
    // 1.2s — shrunk by 0.15s at each end.
    let first = &silences[0];
    let duration = first["durationSeconds"].as_f64().unwrap();
    assert!(
        (duration - 1.2).abs() < 0.15,
        "expected ~1.2s after padding, got {duration}"
    );

    // Frames, not seconds: they go straight into ripple_delete_ranges.
    assert!(first["startFrame"].is_i64() && first["endFrame"].is_i64());
    let expected = (first["startSeconds"].as_f64().unwrap() * 30.0).round() as i64;
    assert_eq!(first["startFrame"].as_i64().unwrap(), expected);
}

#[tokio::test]
async fn detected_silence_cuts_without_any_conversion() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let client = Client::start(EMPTY, "tighten").await;
    let created = client
        .call("manage_project", json!({ "action": "create", "fps": 30 }))
        .await;
    let video_track = created["tracks"][0]["trackId"]
        .as_str()
        .unwrap()
        .to_string();

    let source = make_speech_with_gaps(&client.dir, "speech.mp4");
    let imported = client
        .call("import_media", json!({ "paths": [source] }))
        .await;
    let media_ref = imported["media"][0]["mediaRef"]
        .as_str()
        .unwrap()
        .to_string();
    client
        .call(
            "add_clips",
            json!({ "entries": [{ "mediaRef": media_ref, "trackId": video_track,
                                  "startFrame": 0, "endFrame": 270 }] }),
        )
        .await;

    let found = client
        .call("detect_silence", json!({ "mediaRef": media_ref }))
        .await;
    let ranges: Vec<Value> = found["silences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| json!({ "startFrame": s["startFrame"], "endFrame": s["endFrame"] }))
        .collect();

    let cut = client
        .call("ripple_delete_ranges", json!({ "ranges": ranges }))
        .await;
    assert_eq!(cut["status"], "applied");

    let after = client.timeline().await["totalFrames"].as_i64().unwrap();
    assert!(
        (190..=205).contains(&after),
        "270 frames minus two 1.2s pauses is about 198; got {after}"
    );
}

#[tokio::test]
async fn detect_silence_refuses_media_it_cannot_analyse() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let client = Client::start(EMPTY, "silencebad").await;
    client
        .call("manage_project", json!({ "action": "create" }))
        .await;

    let unknown = client
        .call("detect_silence", json!({ "mediaRef": "ghost" }))
        .await;
    assert_eq!(unknown["status"], "refused");

    // A file with no audio track cannot have silence detected in it.
    let silent = make_source(&client.dir, "mute.mp4", "testsrc");
    let stripped = client.dir.join("noaudio.mp4");
    let out = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i", &silent, "-an", "-c:v", "copy"])
        .arg(&stripped)
        .output()
        .unwrap();
    assert!(out.status.success());
    let imported = client
        .call(
            "import_media",
            json!({ "paths": [stripped.to_string_lossy()] }),
        )
        .await;
    let media_ref = imported["media"][0]["mediaRef"]
        .as_str()
        .unwrap()
        .to_string();
    let no_audio = client
        .call("detect_silence", json!({ "mediaRef": media_ref }))
        .await;
    assert_eq!(no_audio["status"], "refused");
    assert!(
        no_audio["reason"].as_str().unwrap().contains("no audio"),
        "{no_audio}"
    );
}

#[tokio::test]
async fn saving_to_a_palmier_path_writes_a_package_that_reopens() {
    let client = Client::start(EMPTY, "package").await;
    client
        .call("manage_project", json!({ "action": "create", "fps": 30 }))
        .await;

    let target = client.dir.join("edit.palmier");
    let saved = client
        .call(
            "manage_project",
            json!({ "action": "save", "path": target.to_string_lossy() }),
        )
        .await;
    assert_eq!(saved["status"], "saved");
    assert!(target.is_dir(), "a .palmier must be a directory");
    assert!(target.join("project.json").is_file());
    assert!(
        target.join("media.json").is_file(),
        "the manifest travels with the project"
    );

    // The round trip that matters: what was written opens again.
    let reopened = client
        .call(
            "manage_project",
            json!({ "action": "open", "path": target.to_string_lossy() }),
        )
        .await;
    assert_eq!(reopened["status"], "open");
    assert_eq!(client.timeline().await["fps"], 30);
}

#[tokio::test]
async fn an_explicit_json_path_still_writes_that_exact_file() {
    let client = Client::start(EMPTY, "explicitjson").await;
    client
        .call("manage_project", json!({ "action": "create" }))
        .await;
    let target = client.dir.join("custom").join("mine.json");
    let saved = client
        .call(
            "manage_project",
            json!({ "action": "save", "path": target.to_string_lossy() }),
        )
        .await;
    assert_eq!(saved["status"], "saved");
    assert!(target.is_file(), "an explicit .json path means write here");
}

#[tokio::test]
async fn a_refusal_names_the_way_forward() {
    // The old message said only "call open first", when there was no way to create a
    // project at all. A refusal that does not name a next step is a dead end.
    let client = Client::start(EMPTY, "deadend").await;
    for tool in ["get_timeline", "get_media", "detect_silence"] {
        let out = client.call(tool, json!({ "mediaRef": "x" })).await;
        let text = out.to_string();
        assert!(
            text.contains("create"),
            "{tool} does not mention create: {text}"
        );
    }
}

// -------------------------------------------------- the rest of the layer-0 surface

#[tokio::test]
async fn clips_can_be_linked_and_unlinked() {
    // The commands existed and were tested from the first day of the edit layer; the
    // tool did not, so an agent could not link anything.
    let client = Client::start(FIXTURE, "links").await;
    client.open().await;

    let linked = client
        .call(
            "manage_clip_links",
            json!({ "action": "link", "clipIds": ["a", "d"] }),
        )
        .await;
    assert_eq!(linked["status"], "applied");

    let tl = client.timeline().await;
    let a = &tl["tracks"][0]["clips"][0];
    let d = &tl["tracks"][1]["clips"][0];
    assert!(a["linkGroupId"].is_string());
    assert_eq!(
        a["linkGroupId"], d["linkGroupId"],
        "both sides share one group"
    );

    // Proof it means something: moving one now moves the other.
    client
        .call(
            "move_clips",
            json!({ "moves": [{ "clipId": "a", "toTrackId": "v1", "toFrame": 200 }] }),
        )
        .await;
    let moved = client.timeline().await;
    assert_eq!(
        moved["tracks"][1]["clips"][0]["startFrame"], 200,
        "the partner followed"
    );

    client
        .call(
            "manage_clip_links",
            json!({ "action": "unlink", "clipIds": ["a"] }),
        )
        .await;
    let after = client.timeline().await;
    assert!(after["tracks"][0]["clips"][0]["linkGroupId"].is_null());
}

#[tokio::test]
async fn linking_one_clip_alone_is_refused() {
    let client = Client::start(FIXTURE, "linkone").await;
    client.open().await;
    let out = client
        .call(
            "manage_clip_links",
            json!({ "action": "link", "clipIds": ["a"] }),
        )
        .await;
    assert_eq!(out["status"], "refused");
}

#[tokio::test]
async fn markers_survive_the_ripple_that_moves_their_content() {
    let client = Client::start(FIXTURE, "markers").await;
    client.open().await;

    let added = client
        .call(
            "manage_markers",
            json!({ "action": "add", "name": "check this", "startFrame": 75, "comment": "reshoot?" }),
        )
        .await;
    assert_eq!(added["status"], "applied");
    let marker_id = added["createdMarkerIds"][0].as_str().unwrap().to_string();

    let listed = client.timeline().await;
    let mine = listed["markers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["markerId"] == marker_id.as_str())
        .expect("the marker is on the timeline");
    assert_eq!(mine["name"], "check this");
    assert_eq!(mine["startFrame"], 75);

    // The point of a marker: it stays on the shot it was about.
    client
        .call(
            "ripple_delete_ranges",
            json!({ "ranges": [{ "startFrame": 0, "endFrame": 30 }] }),
        )
        .await;
    let after = client.timeline().await;
    let moved = after["markers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["markerId"] == marker_id.as_str())
        .expect("still there");
    assert_eq!(
        moved["startFrame"], 45,
        "the marker moved back with its content"
    );

    client
        .call(
            "manage_markers",
            json!({ "action": "update", "markerId": marker_id, "name": "fixed" }),
        )
        .await;
    let renamed = client.timeline().await;
    assert!(
        renamed["markers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "fixed")
    );

    client
        .call(
            "manage_markers",
            json!({ "action": "remove", "markerId": marker_id }),
        )
        .await;
    let gone = client.timeline().await;
    assert!(
        !gone["markers"]
            .as_array()
            .map(|m| m.iter().any(|x| x["markerId"] == marker_id.as_str()))
            .unwrap_or(false)
    );
}

#[tokio::test]
async fn a_marker_at_a_negative_frame_is_refused() {
    let client = Client::start(FIXTURE, "markerbad").await;
    client.open().await;
    for bad in [
        json!({ "action": "add", "startFrame": -1 }),
        json!({ "action": "add", "startFrame": 0, "durationFrames": -5 }),
        json!({ "action": "update", "markerId": "ghost" }),
        json!({ "action": "remove", "markerId": "ghost" }),
    ] {
        let out = client.call("manage_markers", bad.clone()).await;
        assert_eq!(out["status"], "refused", "{bad} gave {out}");
    }
}

#[tokio::test]
async fn project_settings_change_without_retiming_the_cut() {
    let client = Client::start(FIXTURE, "settings").await;
    client.open().await;
    let before = client.timeline().await;
    assert_eq!(before["fps"], 30);
    assert_eq!(before["totalFrames"], 90);

    let out = client
        .call(
            "set_project_settings",
            json!({ "fps": 24, "width": 1280, "height": 720, "name": "Cut B" }),
        )
        .await;
    assert_eq!(out["status"], "applied");

    let after = client.timeline().await;
    assert_eq!(after["fps"], 24);
    assert_eq!(after["width"], 1280);
    assert_eq!(after["name"], "Cut B");
    assert_eq!(after["totalFrames"], 90, "frame counts are not rescaled");
    assert_eq!(
        after["durationSeconds"], 3.75,
        "so the cut now lasts longer"
    );
}

#[tokio::test]
async fn invalid_project_settings_are_refused() {
    let client = Client::start(FIXTURE, "settingsbad").await;
    client.open().await;
    for bad in [
        json!({ "fps": 0 }),
        json!({ "fps": -1 }),
        json!({ "width": 1 }),
    ] {
        assert_eq!(
            client.call("set_project_settings", bad).await["status"],
            "refused"
        );
    }
}

#[tokio::test]
async fn a_second_timeline_can_be_created_and_switched_to() {
    let client = Client::start(FIXTURE, "timelines").await;
    client.open().await;

    let created = client
        .call("create_timeline", json!({ "name": "Alternate", "fps": 60 }))
        .await;
    assert_eq!(created["status"], "applied");
    let new_id = created["createdTimelineIds"][0]
        .as_str()
        .unwrap()
        .to_string();

    // Creating does not switch: get_timeline still reports the original.
    assert_eq!(client.timeline().await["fps"], 30);

    let switched = client
        .call("set_active_timeline", json!({ "timelineId": new_id }))
        .await;
    assert_eq!(switched["status"], "applied");
    let now = client.timeline().await;
    assert_eq!(now["fps"], 60);
    assert_eq!(now["name"], "Alternate");
    assert_eq!(now["totalFrames"], 0, "the new timeline is empty");

    // And undo puts the session back on the first one.
    client.call("undo", json!({})).await;
    assert_eq!(client.timeline().await["fps"], 30);
}

#[tokio::test]
async fn switching_to_an_unknown_timeline_is_refused() {
    let client = Client::start(FIXTURE, "badtimeline").await;
    client.open().await;
    let out = client
        .call("set_active_timeline", json!({ "timelineId": "ghost" }))
        .await;
    assert_eq!(out["status"], "refused");
}

#[tokio::test]
async fn media_can_be_swapped_under_a_clip() {
    let client = Client::start(FIXTURE, "swap").await;
    client.open().await;
    let before = client.timeline().await["tracks"][0]["clips"][0].clone();

    let out = client
        .call(
            "swap_clip_media",
            json!({ "clipIds": ["a"], "mediaRef": "replacement" }),
        )
        .await;
    assert_eq!(out["status"], "applied");

    let after = client.timeline().await["tracks"][0]["clips"][0].clone();
    assert_eq!(after["mediaRef"], "replacement");
    assert_eq!(
        after["startFrame"], before["startFrame"],
        "position is untouched"
    );
    assert_eq!(
        after["durationFrames"], before["durationFrames"],
        "so is duration"
    );
}

#[tokio::test]
async fn a_clips_look_can_be_copied_without_its_timing() {
    let client = Client::start(FIXTURE, "copysettings").await;
    client.open().await;
    client
        .call(
            "set_clip_properties",
            json!({ "clipIds": ["a"], "opacity": 0.4, "volume": 0.2 }),
        )
        .await;

    let target_before = client.timeline().await["tracks"][0]["clips"][1].clone();
    let out = client
        .call(
            "copy_clip_settings",
            json!({ "fromClipId": "a", "toClipIds": ["b"] }),
        )
        .await;
    assert_eq!(out["status"], "applied");

    let target = client.timeline().await["tracks"][0]["clips"][1].clone();
    assert_eq!(target["opacity"], 0.4, "the look came across");
    assert_eq!(target["volume"], 0.2);
    assert_eq!(
        target["startFrame"], target_before["startFrame"],
        "the timing did not"
    );
    assert_eq!(target["durationFrames"], target_before["durationFrames"]);
    assert_eq!(target["id"], target_before["id"]);
}

#[tokio::test]
async fn copying_a_clip_onto_itself_changes_nothing() {
    let client = Client::start(FIXTURE, "copyself").await;
    client.open().await;
    let out = client
        .call(
            "copy_clip_settings",
            json!({ "fromClipId": "a", "toClipIds": ["a"] }),
        )
        .await;
    assert_eq!(out["status"], "no_op");
}

#[tokio::test]
async fn inspect_media_probes_the_file_rather_than_trusting_the_manifest() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let client = Client::start(EMPTY, "inspectmedia").await;
    client
        .call("manage_project", json!({ "action": "create" }))
        .await;
    let source = make_source(&client.dir, "take.mp4", "testsrc");
    let imported = client
        .call("import_media", json!({ "paths": [&source] }))
        .await;
    let media_ref = imported["media"][0]["mediaRef"]
        .as_str()
        .unwrap()
        .to_string();

    let detail = client
        .call("inspect_media", json!({ "mediaRef": &media_ref }))
        .await;
    assert_eq!(detail["name"], "take.mp4");
    assert_eq!(detail["width"], 320);
    assert_eq!(detail["hasAudio"], true);
    assert_eq!(detail["resolved"], true);

    // A file that vanished is reported as gone, not as its remembered self.
    std::fs::remove_file(&source).unwrap();
    let after = client
        .call("inspect_media", json!({ "mediaRef": media_ref }))
        .await;
    assert_eq!(after["resolved"], false);
}

#[tokio::test]
async fn a_frame_can_be_captured_to_a_file() {
    if !ffmpeg_available() {
        eprintln!("skipped: ffmpeg not on PATH");
        return;
    }
    let (client, _, _) = two_shot_client("capture").await;
    let output = client.dir.join("still.png");
    let out = client
        .call(
            "capture_frame",
            json!({ "frame": 10, "output": output.to_string_lossy() }),
        )
        .await;
    assert_eq!(out["status"], "captured", "{out}");
    assert_eq!(
        out["width"], 640,
        "a capture is full canvas, not the reading size"
    );

    let bytes = std::fs::read(&output).expect("the file must exist");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let (w, _) = png_size(&bytes);
    assert_eq!(w, 640);
}

#[tokio::test]
async fn capture_refuses_a_negative_frame() {
    let client = Client::start(EMPTY, "capturebad").await;
    client
        .call("manage_project", json!({ "action": "create" }))
        .await;
    let out = client
        .call(
            "capture_frame",
            json!({ "frame": -1, "output": client.dir.join("x.png").to_string_lossy() }),
        )
        .await;
    assert_eq!(out["status"], "refused");
}

// ------------------------------------------------- jobs, keys, and generation

#[tokio::test]
async fn generation_is_refused_without_keys_rather_than_hanging() {
    let client = Client::start(EMPTY, "genkeyless").await;
    client
        .call(
            "manage_project",
            json!({ "action": "create", "path": client.dir.join("p.palmier").to_string_lossy() }),
        )
        .await;

    // No keys are configured in the test environment, so this must say so at once.
    let out = client
        .call("generate_image", json!({ "prompt": "a bowl of soup" }))
        .await;
    assert_eq!(out["status"], "refused");
    assert!(out["reason"].as_str().unwrap().contains("keys"), "{out}");
}

#[tokio::test]
async fn generation_needs_somewhere_to_put_the_file() {
    let client = Client::start(EMPTY, "genunsaved").await;
    // Created in memory and never saved: there is no package to write into.
    client
        .call("manage_project", json!({ "action": "create" }))
        .await;
    let out = client
        .call("generate_image", json!({ "prompt": "a cat" }))
        .await;
    assert_eq!(out["status"], "refused");
    assert!(out["reason"].as_str().unwrap().contains("save"), "{out}");
}

#[tokio::test]
async fn an_empty_prompt_is_refused() {
    let client = Client::start(EMPTY, "genblank").await;
    client
        .call("manage_project", json!({ "action": "create" }))
        .await;
    let out = client
        .call("generate_image", json!({ "prompt": "   " }))
        .await;
    assert_eq!(out["status"], "refused");
}

#[tokio::test]
async fn the_job_list_starts_empty_and_reports_what_it_knows() {
    let client = Client::start(EMPTY, "jobs").await;
    let listed = client.call("manage_jobs", json!({})).await;
    assert_eq!(listed["running"], 0);
    assert_eq!(listed["jobs"].as_array().unwrap().len(), 0);

    let unknown = client
        .call(
            "manage_jobs",
            json!({ "action": "status", "jobId": "ghost" }),
        )
        .await;
    assert_eq!(unknown["status"], "refused");

    let no_id = client
        .call("manage_jobs", json!({ "action": "status" }))
        .await;
    assert_eq!(no_id["status"], "refused");

    let bad = client
        .call("manage_jobs", json!({ "action": "explode" }))
        .await;
    assert_eq!(bad["status"], "refused");
}

#[tokio::test]
async fn cancelling_an_unknown_job_is_refused_not_silently_ignored() {
    let client = Client::start(EMPTY, "jobcancel").await;
    let out = client
        .call(
            "manage_jobs",
            json!({ "action": "cancel", "jobId": "ghost" }),
        )
        .await;
    assert_eq!(out["status"], "refused");
}

#[tokio::test]
async fn listing_keys_never_reveals_one() {
    let client = Client::start(EMPTY, "keys").await;
    let listed = client.call("manage_keys", json!({})).await;
    assert_eq!(listed["provider"], "stitch");
    assert!(listed["count"].is_number());
    // Whatever is configured, the response carries hints, never whole keys.
    for hint in listed["keys"].as_array().unwrap() {
        let hint = hint.as_str().unwrap();
        assert!(
            hint.contains('…') || hint.contains('•'),
            "a listed key looks unmasked: {hint}"
        );
    }
}

#[tokio::test]
async fn key_management_refuses_what_it_cannot_do() {
    let client = Client::start(EMPTY, "keysbad").await;
    for (arguments, why) in [
        (json!({ "provider": "openai" }), "unknown provider"),
        (json!({ "action": "set" }), "set without a key"),
        (json!({ "action": "forget" }), "forget without a slot"),
        (json!({ "action": "explode" }), "unknown action"),
    ] {
        let out = client.call("manage_keys", arguments.clone()).await;
        assert_eq!(out["status"], "refused", "{why}: {out}");
    }
}
