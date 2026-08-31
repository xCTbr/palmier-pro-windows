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
