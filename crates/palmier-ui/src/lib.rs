//! The desktop interface: a local web app served by the daemon itself.
//!
//! It edits through the same `EditSession` the agent does, so a change made here is a
//! change the agent sees, and the reverse — constitution principle I, which exists
//! exactly so this layer could be added without a second way to mutate a project.

use std::sync::Arc;

pub mod chat;

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use palmier_mcp::jobs::Jobs;
use palmier_mcp::session::Session;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Ui {
    pub session: Arc<Mutex<Session>>,
    pub jobs: Jobs,
    pub port: u16,
}

/// Static assets are compiled in, so the app is one file with nothing to install beside it.
const INDEX: &str = include_str!("../web/index.html");
const STYLE: &str = include_str!("../web/style.css");
const APP: &str = include_str!("../web/app.js");

fn asset(body: &'static str, mime: &'static str) -> Response {
    ([(header::CONTENT_TYPE, mime)], body).into_response()
}

pub fn router(state: Ui) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { asset(INDEX, "text/html; charset=utf-8") }),
        )
        .route(
            "/style.css",
            get(|| async { asset(STYLE, "text/css; charset=utf-8") }),
        )
        .route(
            "/app.js",
            get(|| async { asset(APP, "text/javascript; charset=utf-8") }),
        )
        .route("/api/status", get(status))
        .route("/api/keys", get(list_keys).post(set_key))
        .route("/api/keys/{slot}", axum::routing::delete(forget_key))
        .route("/api/project", get(project).post(open_or_create))
        .route("/api/project/save", post(save))
        .route("/api/frame/{frame}", get(frame))
        .route("/api/jobs", get(list_jobs))
        .route("/api/chat", post(chat::ask))
        .with_state(state)
}

async fn status(State(ui): State<Ui>) -> Json<Value> {
    let session = ui.session.lock().await;
    let missing: Vec<&str> = ["ffmpeg", "ffprobe"]
        .into_iter()
        .filter(|tool| palmier_media::require_tool(tool).is_err())
        .collect();
    let keys = palmier_gen::KeyRing::load("stitch", "STITCH_API_KEY");

    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "mcpUrl": format!("http://127.0.0.1:{}/mcp", ui.port),
        "connectCommand": format!(
            "claude mcp add --transport http palmier http://127.0.0.1:{}/mcp",
            ui.port
        ),
        "ffmpeg": { "ready": missing.is_empty(), "missing": missing },
        "keys": { "count": keys.len() },
        "project": {
            "open": session.is_open(),
            "path": session.path().map(|p| p.display().to_string()),
            "unsaved": session.is_dirty(),
        },
        "jobsRunning": ui.jobs.running_count().await,
        "chat": { "available": chat::cli_available() },
    }))
}

async fn list_keys(State(_ui): State<Ui>) -> Json<Value> {
    let ring = palmier_gen::KeyRing::load("stitch", "STITCH_API_KEY");
    Json(json!({ "provider": "stitch", "count": ring.len(), "keys": ring.hints() }))
}

#[derive(Deserialize)]
struct SetKey {
    key: String,
    #[serde(default)]
    slot: Option<usize>,
}

async fn set_key(State(_ui): State<Ui>, Json(body): Json<SetKey>) -> Response {
    let ring = palmier_gen::KeyRing::load("stitch", "STITCH_API_KEY");
    let slot = body.slot.unwrap_or(ring.len() + 1).max(1);
    match palmier_gen::KeyRing::store("stitch", slot, &body.key) {
        Ok(()) => Json(json!({ "status": "stored", "slot": slot })).into_response(),
        Err(error) => problem(error.to_string()),
    }
}

async fn forget_key(State(_ui): State<Ui>, Path(slot): Path<usize>) -> Response {
    match palmier_gen::KeyRing::forget("stitch", slot) {
        Ok(()) => Json(json!({ "status": "forgotten", "slot": slot })).into_response(),
        Err(error) => problem(error.to_string()),
    }
}

async fn project(State(ui): State<Ui>) -> Response {
    let session = ui.session.lock().await;
    let Ok(timeline) = session.active_timeline() else {
        return Json(json!({ "open": false })).into_response();
    };
    let media: Vec<Value> = session
        .manifest()
        .entries
        .iter()
        .map(|e| json!({ "mediaRef": e.id, "name": e.name, "durationSeconds": e.duration }))
        .collect();

    Json(json!({
        "open": true,
        "path": session.path().map(|p| p.display().to_string()),
        "unsaved": session.is_dirty(),
        "timeline": palmier_mcp::render::timeline(timeline, None),
        "media": media,
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenOrCreate {
    action: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    fps: Option<i64>,
    #[serde(default)]
    width: Option<i64>,
    #[serde(default)]
    height: Option<i64>,
}

async fn open_or_create(State(ui): State<Ui>, Json(body): Json<OpenOrCreate>) -> Response {
    let mut session = ui.session.lock().await;
    let result = match body.action.as_str() {
        "open" => match body.path {
            Some(path) => session.open(std::path::Path::new(&path)).map(|_| ()),
            None => return problem("open needs a path"),
        },
        "create" => session
            .create(
                body.fps.unwrap_or(30),
                body.width.unwrap_or(1920),
                body.height.unwrap_or(1080),
            )
            .and_then(|()| match &body.path {
                Some(path) => session.save(Some(std::path::Path::new(path))).map(|_| ()),
                None => Ok(()),
            }),
        "close" => {
            session.close();
            Ok(())
        }
        other => return problem(format!("unknown action `{other}`")),
    };
    match result {
        Ok(()) => Json(json!({ "status": "ok" })).into_response(),
        Err(error) => problem(error.to_string()),
    }
}

async fn save(State(ui): State<Ui>, Json(body): Json<serde_json::Value>) -> Response {
    let mut session = ui.session.lock().await;
    let path = body
        .get("path")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from);
    match session.save(path.as_deref()) {
        Ok(written) => Json(json!({ "status": "saved", "path": written.display().to_string() }))
            .into_response(),
        Err(error) => problem(error.to_string()),
    }
}

/// One composited frame as a PNG.
///
/// Rendering shells out to ffmpeg and takes a moment, so this is a click-to-seek
/// preview rather than a scrubbing one. Layer 1's compositor is what makes it live.
async fn frame(State(ui): State<Ui>, Path(frame): Path<i64>) -> Response {
    let session = ui.session.lock().await;
    let Ok(timeline) = session.active_timeline().cloned() else {
        return problem("no project is open");
    };
    if frame < 0 {
        return problem("frame is before the start of the timeline");
    }
    let resolve = session.resolver();
    // Held across the render, which is slow: the alternative is a torn frame from a
    // timeline someone edited halfway through drawing it.
    let options = palmier_media::FrameOptions {
        grid: false,
        max_width: 960,
    };
    match palmier_media::frame_png(&timeline, &resolve, frame, options) {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        Err(error) => problem(error.to_string()),
    }
}

async fn list_jobs(State(ui): State<Ui>) -> Json<Value> {
    let jobs = ui.jobs.list().await;
    Json(json!({
        "running": ui.jobs.running_count().await,
        "jobs": jobs.iter().map(|j| j.render()).collect::<Vec<_>>(),
    }))
}

/// One error shape for the whole API, so the UI has one thing to render.
pub(crate) fn problem(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": message.into() })),
    )
        .into_response()
}
