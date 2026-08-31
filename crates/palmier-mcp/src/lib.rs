//! The MCP surface over the edit layer.
//!
//! Tools own no editing logic, and structurally cannot: `EditSession::apply` is the only
//! public way to mutate a project. Each tool resolves arguments to stable ids, builds
//! exactly one `EditCommand`, applies it, and renders the `Receipt` that comes back
//! (constitution principles I and VI).

use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

pub mod jobs;
pub mod render;
pub mod session;

use palmier_core::edit::{
    ClipMove, ClipProperties, EditCommand, SplitPoint, TrackProperties, TrimEdge,
};
use palmier_core::timeline::{Clip, ClipType};
use session::Session;

pub const DEFAULT_PORT: u16 = 19789;

/// Said often enough to be worth saying once.
const NO_PROJECT: &str = "no project is open — call manage_project with action 'create' for a new edit, or 'open' for an existing .palmier";

#[derive(Clone)]
pub struct Palmier {
    session: Arc<Mutex<Session>>,
    jobs: jobs::Jobs,
    tool_router: ToolRouter<Self>,
}

impl Palmier {
    pub fn new() -> Self {
        Self::shared(Arc::new(Mutex::new(Session::default())), jobs::Jobs::new())
    }

    /// Build a server over state someone else owns.
    ///
    /// Every MCP client, and the desktop UI, must edit the *same* project. A factory
    /// that made fresh state per connection would give two agents two different films.
    pub fn shared(session: Arc<Mutex<Session>>, jobs: jobs::Jobs) -> Self {
        Self {
            session,
            jobs,
            tool_router: Self::tool_router(),
        }
    }

    pub fn session_handle(&self) -> Arc<Mutex<Session>> {
        self.session.clone()
    }

    pub fn jobs_handle(&self) -> jobs::Jobs {
        self.jobs.clone()
    }
}

impl Default for Palmier {
    fn default() -> Self {
        Self::new()
    }
}

fn ok(value: Value) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
    )]))
}

/// A malformed call: the request's *shape* is wrong. Protocol-level, like the ones
/// `rmcp` raises when a required field is missing.
/// Minimal base64 for image payloads. Pulling a crate in for forty lines of table
/// lookup is not worth the dependency.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn invalid(message: impl Into<String>) -> ErrorData {
    ErrorData::invalid_params(message.into(), None)
}

/// A well-formed call that cannot be honoured. Same shape as a command-layer refusal,
/// so an agent reads one vocabulary for "no" rather than two.
fn refused(reason: impl Into<String>) -> Result<CallToolResult, ErrorData> {
    ok(json!({ "status": "refused", "reason": reason.into() }))
}

// ---------------------------------------------------------------- arguments

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManageProjectArgs {
    /// `create`, `open`, `save`, `close`, or `describe`.
    pub action: String,
    /// A `.palmier` folder or a `project.json`. Required for `open`; optional for
    /// `save` and `create`.
    ///
    /// On Windows write the path with forward slashes (`C:/work/edit.palmier`) or with
    /// doubled backslashes — a single backslash is not a legal escape inside a JSON
    /// string, so the call is rejected before it reaches this tool.
    #[serde(default)]
    pub path: Option<String>,
    /// `create` only. Timeline frame rate. Defaults to 30.
    #[serde(default)]
    pub fps: Option<i64>,
    /// `create` only. Canvas width in pixels. Defaults to 1920.
    #[serde(default)]
    pub width: Option<i64>,
    /// `create` only. Canvas height in pixels. Defaults to 1080.
    #[serde(default)]
    pub height: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetectSilenceArgs {
    /// Media to analyse, from get_media or import_media.
    pub media_ref: String,
    /// Level below which audio counts as silence, in dBFS. Defaults to -30; go lower
    /// for a noisy room, higher to catch softer pauses.
    #[serde(default)]
    pub noise_db: Option<f64>,
    /// Ignore pauses shorter than this many seconds. Defaults to 0.5 — about the gap
    /// between sentences rather than between words.
    #[serde(default)]
    pub min_seconds: Option<f64>,
    /// Shrink each reported silence by this many seconds at both ends, so a cut does
    /// not clip the start or end of a word. Defaults to 0.15.
    #[serde(default)]
    pub padding_seconds: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetTimelineArgs {
    /// Optional. Window start, inclusive. Only clips intersecting `[startFrame, endFrame)`
    /// are returned. Omit both for the whole timeline; never pass a zero-width window.
    #[serde(default)]
    pub start_frame: Option<i64>,
    /// Optional. Window end, exclusive. Must be greater than `startFrame`.
    #[serde(default)]
    pub end_frame: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManageTracksArgs {
    /// `add`, `remove`, or `update`.
    pub action: String,
    /// Track to act on, for `remove` and `update`. Prefer this over `trackIndex`.
    #[serde(default)]
    pub track_id: Option<String>,
    /// Positional alias for `trackId`, accepted for compatibility and resolved to an id
    /// immediately. Indexes shift when tracks are removed; ids do not.
    #[serde(default)]
    pub track_index: Option<usize>,
    /// For `add`: `video`, `audio`, `image`, `text`, `lottie`, `sequence`, or `subtitle`.
    #[serde(default)]
    pub track_type: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub muted: Option<bool>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub sync_locked: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddClipEntry {
    /// Id of the media asset, or a timelineId to nest a sequence.
    pub media_ref: String,
    /// Destination track. Prefer this over `trackIndex`.
    #[serde(default)]
    pub track_id: Option<String>,
    #[serde(default)]
    pub track_index: Option<usize>,
    /// Timeline frame the clip starts on.
    pub start_frame: i64,
    /// Occupies `[startFrame, endFrame)`. End is exclusive.
    pub end_frame: i64,
    /// Optional. `video`, `audio`, `image`, or `text`. Defaults to `video`.
    #[serde(default)]
    pub media_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddClipsArgs {
    /// Clips to add. Every entry is validated up front; one bad entry rejects the whole
    /// call with no partial state.
    pub entries: Vec<AddClipEntry>,
    /// Optional. When set, existing clips are pushed later to open a gap instead of
    /// being overwritten.
    #[serde(default)]
    pub insert: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveEntry {
    pub clip_id: String,
    #[serde(default)]
    pub to_track_id: Option<String>,
    #[serde(default)]
    pub to_track_index: Option<usize>,
    pub to_frame: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveClipsArgs {
    /// Moves to apply as one undoable action. Linked partners follow automatically.
    pub moves: Vec<MoveEntry>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveClipsArgs {
    pub clip_ids: Vec<String>,
    /// When true, later clips shift back to close the gap on every sync-locked track.
    #[serde(default)]
    pub ripple: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SplitEntry {
    #[serde(default)]
    pub track_id: Option<String>,
    #[serde(default)]
    pub track_index: Option<usize>,
    /// Splitting at a clip's first or last frame does nothing.
    pub at_frame: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SplitClipsArgs {
    pub points: Vec<SplitEntry>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RangeEntry {
    pub start_frame: i64,
    /// Exclusive.
    pub end_frame: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RippleDeleteArgs {
    /// Spans to cut out. Overlapping and adjacent spans merge before anything moves.
    pub ranges: Vec<RangeEntry>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetClipPropertiesArgs {
    pub clip_ids: Vec<String>,
    #[serde(default)]
    pub opacity: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    /// Must be finite and positive. Nested sequence clips cannot be retimed.
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub fade_in_frames: Option<i64>,
    #[serde(default)]
    pub fade_out_frames: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrimClipArgs {
    pub clip_id: String,
    /// `left` or `right`.
    pub edge: String,
    /// Positive lengthens the right edge or shortens the left; negative does the reverse.
    pub delta_frames: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportMediaArgs {
    /// Absolute paths to media files. Each is probed; one unreadable file rejects the
    /// whole call with no partial state.
    pub paths: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetMediaArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectTimelineArgs {
    /// Project frame to render. Defaults to 0.
    #[serde(default)]
    pub start_frame: Option<i64>,
    /// Optional. Sample `maxFrames` evenly across `[startFrame, endFrame)` instead of
    /// rendering one frame.
    #[serde(default)]
    pub end_frame: Option<i64>,
    /// How many frames to sample across the range. Defaults to 1, capped at 6.
    #[serde(default)]
    pub max_frames: Option<usize>,
    /// Overlay a 0–1 coordinate grid over the canvas. Defaults to true.
    #[serde(default)]
    pub grid: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportProjectArgs {
    /// Where to write the file.
    pub output: String,
    /// Optional. `h264` (default) or `h265`.
    #[serde(default)]
    pub codec: Option<String>,
    /// Optional. Constant Rate Factor, 0–51. Lower is better quality, larger file.
    #[serde(default)]
    pub crf: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManageClipLinksArgs {
    /// `link` or `unlink`.
    pub action: String,
    /// Clips to act on. `link` needs at least two; `unlink` expands to whole groups.
    pub clip_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManageMarkersArgs {
    /// `add`, `update`, or `remove`.
    pub action: String,
    /// Marker to act on, for `update` and `remove`.
    #[serde(default)]
    pub marker_id: Option<String>,
    /// Several markers to remove at once.
    #[serde(default)]
    pub marker_ids: Option<Vec<String>>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    /// Where the marker sits. A point marker has no duration.
    #[serde(default)]
    pub start_frame: Option<i64>,
    /// Length in frames. 0, or absent, makes it a point marker.
    #[serde(default)]
    pub duration_frames: Option<i64>,
    /// Mark a review note as dealt with.
    #[serde(default)]
    pub resolved: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetProjectSettingsArgs {
    #[serde(default)]
    pub fps: Option<i64>,
    #[serde(default)]
    pub width: Option<i64>,
    #[serde(default)]
    pub height: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateTimelineArgs {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub fps: Option<i64>,
    #[serde(default)]
    pub width: Option<i64>,
    #[serde(default)]
    pub height: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveTimelineArgs {
    pub timeline_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SwapClipMediaArgs {
    pub clip_ids: Vec<String>,
    /// The asset the clips should point at instead, from get_media.
    pub media_ref: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CopyClipSettingsArgs {
    /// The clip whose look is copied.
    pub from_clip_id: String,
    /// Clips that take on that look. Timing and identity are never copied.
    pub to_clip_ids: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectMediaArgs {
    pub media_ref: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CaptureFrameArgs {
    /// Project frame to capture.
    pub frame: i64,
    /// Where to write the PNG.
    pub output: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateImageArgs {
    /// What the picture should be. Describe the subject, the light, and the framing.
    pub prompt: String,
    /// How many times to cycle through every key before giving up. Defaults to 2.
    #[serde(default)]
    pub rounds: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManageJobsArgs {
    /// `list`, `status`, or `cancel`. Defaults to `list`.
    #[serde(default)]
    pub action: Option<String>,
    /// The job to ask about or stop.
    #[serde(default)]
    pub job_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ManageKeysArgs {
    /// `list`, `set`, or `forget`. Defaults to `list`.
    #[serde(default)]
    pub action: Option<String>,
    /// Which provider. Only `stitch` today.
    #[serde(default)]
    pub provider: Option<String>,
    /// Which slot to write or clear, counting from 1.
    #[serde(default)]
    pub slot: Option<usize>,
    /// The key itself, for `set`. It is never echoed back.
    #[serde(default)]
    pub key: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UndoArgs {
    /// When true, reapply the change that was last undone instead of undoing.
    #[serde(default)]
    pub redo: Option<bool>,
}

// -------------------------------------------------------------------- tools

#[tool_router]
impl Palmier {
    #[tool(
        description = "Create, open, save, close, or describe the project this session \
edits.\n\n\
Start with `create` for a new edit: it makes an empty project in memory with one video \
track and one audio track, and writes nothing until you save. Pass a path to `create` to \
save it there at once, or to `save` later. Use `open` for a `.palmier` that already \
exists. On Windows, write paths with forward slashes. \
Editing never writes to disk; the project is written only when you save it here."
    )]
    async fn manage_project(
        &self,
        Parameters(args): Parameters<ManageProjectArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        match args.action.as_str() {
            "create" => {
                // Well-formed but impossible settings are a refusal, not a protocol
                // error — the agent should read the reason and pick better numbers.
                if let Err(error) = session.create(
                    args.fps.unwrap_or(30),
                    args.width.unwrap_or(1920),
                    args.height.unwrap_or(1080),
                ) {
                    return refused(error.to_string());
                }
                if let Some(path) = &args.path
                    && let Err(error) = session.save(Some(&PathBuf::from(path)))
                {
                    return refused(error.to_string());
                }
                let project = session.project().map_err(|e| invalid(e.to_string()))?;
                let timeline = &project.timelines[0];
                ok(json!({
                    "status": "created",
                    "path": session.path().map(|p| p.display().to_string()),
                    "timelineId": timeline.id.clone().unwrap_or_default(),
                    "fps": timeline.fps,
                    "width": timeline.width,
                    "height": timeline.height,
                    "tracks": timeline.tracks.iter().map(|t| json!({
                        "trackId": t.id.clone().unwrap_or_default(),
                        "type": render::clip_type(t.track_type),
                    })).collect::<Vec<_>>(),
                }))
            }
            "open" => {
                let path = args.path.ok_or_else(|| invalid("`open` needs a path"))?;
                session
                    .open(&PathBuf::from(&path))
                    .map_err(|e| invalid(e.to_string()))?;
                let project = session.project().map_err(|e| invalid(e.to_string()))?;
                ok(json!({
                    "status": "open",
                    "path": path,
                    "timelines": project.timelines.iter().map(|t| json!({
                        "timelineId": t.id.clone().unwrap_or_default(),
                        "name": t.name,
                        "fps": t.fps,
                        "totalFrames": t.total_frames().unwrap_or(0),
                    })).collect::<Vec<_>>(),
                }))
            }
            "save" => {
                let to = args.path.map(PathBuf::from);
                let written = session
                    .save(to.as_deref())
                    .map_err(|e| invalid(e.to_string()))?;
                ok(json!({ "status": "saved", "path": written.display().to_string() }))
            }
            "close" => {
                session.close();
                ok(json!({ "status": "closed" }))
            }
            "describe" => ok(json!({
                "open": session.is_open(),
                "path": session.path().map(|p| p.display().to_string()),
                "unsavedChanges": session.is_dirty(),
            })),
            other => refused(format!(
                "unknown action `{other}`; expected create, open, save, close, or describe"
            )),
        }
    }

    #[tool(
        description = "Read the timeline. Call this at the start of a session and after \
edits you want to verify. Every clip occupies frames [startFrame, endFrame) — end \
exclusive, duration = end − start. Tracks carry a stable trackId; prefer it over \
trackIndex, which shifts when tracks are removed. Fields equal to their defaults are \
omitted. `gaps` lists a track's empty spans; no `gaps` key means it is contiguous."
    )]
    async fn get_timeline(
        &self,
        Parameters(args): Parameters<GetTimelineArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self.session.lock().await;
        let Ok(timeline) = session.active_timeline() else {
            return refused(NO_PROJECT);
        };

        let window = match (args.start_frame, args.end_frame) {
            (None, None) => None,
            (Some(s), Some(e)) if e > s => Some((s, e)),
            (Some(s), Some(e)) => {
                return refused(format!(
                    "endFrame ({e}) must be greater than startFrame ({s})"
                ));
            }
            (Some(s), None) => Some((s, i64::MAX)),
            (None, Some(e)) => Some((0, e)),
        };
        ok(render::timeline(timeline, window))
    }

    #[tool(
        description = "Add, remove, or update a track. A track holding clips cannot be removed."
    )]
    async fn manage_tracks(
        &self,
        Parameters(args): Parameters<ManageTracksArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = {
            let edit = session.edit().map_err(|e| invalid(e.to_string()))?;
            let active = edit.project.active_timeline_id.clone();
            let timeline = edit
                .project
                .timelines
                .iter()
                .find(|t| active.is_none() || t.id == active)
                .or_else(|| edit.project.timelines.first())
                .ok_or_else(|| invalid("the project has no timelines"))?;
            match args.action.as_str() {
                "add" => {
                    let kind = args.track_type.as_deref().unwrap_or("video");
                    EditCommand::AddTrack {
                        track_type: parse_clip_type(kind)?,
                        at_index: args.track_index,
                    }
                }
                "remove" => EditCommand::RemoveTrack {
                    track_id: resolve_track(timeline, args.track_id.as_deref(), args.track_index)?,
                },
                "update" => EditCommand::SetTrackProperties {
                    track_id: resolve_track(timeline, args.track_id.as_deref(), args.track_index)?,
                    properties: TrackProperties {
                        name: args.name.map(Some),
                        muted: args.muted,
                        hidden: args.hidden,
                        sync_locked: args.sync_locked,
                    },
                },
                other => {
                    return Err(invalid(format!(
                        "unknown action `{other}`; expected add, remove, or update"
                    )));
                }
            }
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Place clips on the timeline as one undoable action. Every entry is \
validated up front; one bad entry rejects the whole call with no partial state. Clips on \
the same track are sequential: a new clip overwrites what it lands on, trimming, \
splitting, or removing to make room. Set `insert` to push later clips aside instead."
    )]
    async fn add_clips(
        &self,
        Parameters(args): Parameters<AddClipsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        if args.entries.is_empty() {
            return Err(invalid("`entries` is empty"));
        }
        let command = {
            let edit = session.edit().map_err(|e| invalid(e.to_string()))?;
            let active = edit.project.active_timeline_id.clone();
            let timeline = edit
                .project
                .timelines
                .iter()
                .find(|t| active.is_none() || t.id == active)
                .or_else(|| edit.project.timelines.first())
                .ok_or_else(|| invalid("the project has no timelines"))?;

            let first = &args.entries[0];
            let track_id = resolve_track(timeline, first.track_id.as_deref(), first.track_index)?;
            let mut clips = Vec::with_capacity(args.entries.len());
            for entry in &args.entries {
                let entry_track =
                    resolve_track(timeline, entry.track_id.as_deref(), entry.track_index)?;
                if entry_track != track_id {
                    return Err(invalid(
                        "every entry must target the same track; split into one call per track",
                    ));
                }
                if entry.end_frame <= entry.start_frame {
                    return Err(invalid(format!(
                        "`{}` has endFrame {} at or before startFrame {}",
                        entry.media_ref, entry.end_frame, entry.start_frame
                    )));
                }
                clips.push(build_clip(entry)?);
            }
            if args.insert.unwrap_or(false) {
                EditCommand::InsertClips {
                    track_id,
                    at_frame: args.entries[0].start_frame,
                    clips,
                }
            } else {
                EditCommand::AddClips { track_id, clips }
            }
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Move clips to another frame or track as one undoable action. A \
linked audio partner follows by the same delta. If any target is unknown or any \
destination is out of bounds, the whole call is refused and nothing moves."
    )]
    async fn move_clips(
        &self,
        Parameters(args): Parameters<MoveClipsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = {
            let edit = session.edit().map_err(|e| invalid(e.to_string()))?;
            let active = edit.project.active_timeline_id.clone();
            let timeline = edit
                .project
                .timelines
                .iter()
                .find(|t| active.is_none() || t.id == active)
                .or_else(|| edit.project.timelines.first())
                .ok_or_else(|| invalid("the project has no timelines"))?;
            let mut moves = Vec::with_capacity(args.moves.len());
            for entry in &args.moves {
                moves.push(ClipMove {
                    clip_id: entry.clip_id.clone(),
                    to_track_id: resolve_track(
                        timeline,
                        entry.to_track_id.as_deref(),
                        entry.to_track_index,
                    )?,
                    to_frame: entry.to_frame,
                });
            }
            EditCommand::MoveClips { moves }
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Remove clips. With `ripple` true, later clips shift back to close \
the gap on every sync-locked track and markers move with them. Linked partners are \
removed together."
    )]
    async fn remove_clips(
        &self,
        Parameters(args): Parameters<RemoveClipsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = EditCommand::RemoveClips {
            clip_ids: args.clip_ids,
            ripple: args.ripple.unwrap_or(false),
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Split clips at timeline frames. Splitting at a clip's first or last \
frame does nothing and is reported as a no-op. Linked partners split together, and the \
right-hand halves become their own link group."
    )]
    async fn split_clips(
        &self,
        Parameters(args): Parameters<SplitClipsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = {
            let edit = session.edit().map_err(|e| invalid(e.to_string()))?;
            let active = edit.project.active_timeline_id.clone();
            let timeline = edit
                .project
                .timelines
                .iter()
                .find(|t| active.is_none() || t.id == active)
                .or_else(|| edit.project.timelines.first())
                .ok_or_else(|| invalid("the project has no timelines"))?;
            let mut points = Vec::with_capacity(args.points.len());
            for entry in &args.points {
                points.push(SplitPoint {
                    track_id: resolve_track(
                        timeline,
                        entry.track_id.as_deref(),
                        entry.track_index,
                    )?,
                    at_frame: entry.at_frame,
                });
            }
            EditCommand::SplitClips { points }
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Cut spans out of the timeline and close the gaps. Every sync-locked \
track shifts by the same amount so the edit stays in sync; tracks with syncLocked false \
stay put. Markers move with the content they annotate. Overlapping and adjacent spans \
merge before anything moves."
    )]
    async fn ripple_delete_ranges(
        &self,
        Parameters(args): Parameters<RippleDeleteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let ranges = args
            .ranges
            .iter()
            .map(|r| (r.start_frame, r.end_frame))
            .collect();
        self.run(&mut session, EditCommand::RippleDeleteRanges { ranges })
            .await
    }

    #[tool(
        description = "Set opacity, volume, speed, or fades on clips. Speed must be finite and positive."
    )]
    async fn set_clip_properties(
        &self,
        Parameters(args): Parameters<SetClipPropertiesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = EditCommand::SetClipProperties {
            clip_ids: args.clip_ids,
            properties: ClipProperties {
                opacity: args.opacity,
                volume: args.volume,
                speed: args.speed,
                fade_in_frames: args.fade_in_frames,
                fade_out_frames: args.fade_out_frames,
            },
        };
        self.run(&mut session, command).await
    }

    #[tool(description = "Trim a clip's left or right edge, keeping its source media continuous.")]
    async fn trim_clip(
        &self,
        Parameters(args): Parameters<TrimClipArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let edge = match args.edge.as_str() {
            "left" => TrimEdge::Left,
            "right" => TrimEdge::Right,
            other => {
                return Err(invalid(format!(
                    "unknown edge `{other}`; expected left or right"
                )));
            }
        };
        let command = EditCommand::TrimClip {
            clip_id: args.clip_id,
            edge,
            delta_frames: args.delta_frames,
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "See the composited timeline — what a viewer would actually see at \
a given frame, with every visible clip stacked as the render will place them. Use this \
to check that an edit landed: a cut's timing, a clip's position, which layer is on top. \
Reading the timeline tells you the numbers; this tells you the picture.\n\n\
Frames are project frames from get_timeline. Pass startFrame alone for one frame, or \
add endFrame to sample maxFrames evenly across [startFrame, endFrame). Frames past the \
end of all content render black. Each image is preceded by a text block naming its frame \
and listing the clip ids visible in it, topmost first, so what you see maps straight \
back to the clips you can edit. A 0–1 coordinate grid is drawn over the canvas by \
default, origin top-left, one cell per 0.1."
    )]
    async fn inspect_timeline(
        &self,
        Parameters(args): Parameters<InspectTimelineArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self.session.lock().await;
        let Ok(timeline) = session.active_timeline().cloned() else {
            return refused(NO_PROJECT);
        };

        let start = args.start_frame.unwrap_or(0);
        if start < 0 {
            return refused(format!(
                "startFrame {start} is before the start of the timeline"
            ));
        }
        let count = args.max_frames.unwrap_or(1).clamp(1, 6);
        let frames: Vec<i64> = match args.end_frame {
            None => vec![start],
            Some(end) if end <= start => {
                return refused(format!(
                    "endFrame ({end}) must be greater than startFrame ({start})"
                ));
            }
            Some(end) => {
                if count == 1 {
                    vec![start]
                } else {
                    let span = end - start;
                    (0..count)
                        .map(|i| start + span * i as i64 / count as i64)
                        .collect()
                }
            }
        };

        let options = palmier_media::FrameOptions {
            grid: args.grid.unwrap_or(true),
            ..Default::default()
        };
        let resolve = session.resolver();

        let mut blocks = Vec::with_capacity(frames.len() * 2);
        for frame in frames {
            let visible: Vec<String> = timeline
                .visible_clips_at(frame)
                .iter()
                .map(|c| c.id.clone().unwrap_or_default())
                .collect();
            let seconds = frame as f64 / timeline.fps.max(1) as f64;
            let meta = json!({
                "frame": frame,
                "seconds": seconds,
                "visibleClipIds": visible,
                "canvas": { "width": timeline.width, "height": timeline.height },
            });
            blocks.push(ContentBlock::text(
                serde_json::to_string(&meta).unwrap_or_default(),
            ));

            match palmier_media::frame_png(&timeline, &resolve, frame, options) {
                Ok(png) => blocks.push(ContentBlock::image(
                    base64_encode(&png),
                    "image/png".to_string(),
                )),
                Err(error) => {
                    return refused(format!("cannot render frame {frame}: {error}"));
                }
            }
        }
        Ok(CallToolResult::success(blocks))
    }

    #[tool(
        description = "Import media files into the project so clips can reference them. \
Each file is probed for duration, resolution, frame rate, and whether it carries audio; \
the returned mediaRef is what add_clips takes. One unreadable file rejects the whole \
call with no partial state."
    )]
    async fn import_media(
        &self,
        Parameters(args): Parameters<ImportMediaArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        if !session.is_open() {
            return refused(NO_PROJECT);
        }
        if args.paths.is_empty() {
            return refused("`paths` is empty");
        }

        // Probe everything before recording anything, so a bad file leaves no trace.
        let package = session.package_dir();
        let mut probed = Vec::with_capacity(args.paths.len());
        for raw in &args.paths {
            let path = PathBuf::from(raw);
            match palmier_media::probe(&path) {
                Ok(info) => probed.push((path, info)),
                Err(error) => return refused(format!("{raw}: {error}")),
            }
        }

        let mut imported = Vec::with_capacity(probed.len());
        for (path, info) in probed {
            let id = uuid::Uuid::new_v4().to_string();
            let entry = session::entry_for(id.clone(), &path, &info, package.as_deref());
            imported.push(json!({
                "mediaRef": id,
                "name": entry.name,
                "durationSeconds": info.duration_seconds,
                "width": info.width,
                "height": info.height,
                "fps": info.fps,
                "hasAudio": info.has_audio,
            }));
            session.add_media(entry);
        }
        ok(json!({ "status": "imported", "media": imported }))
    }

    #[tool(
        description = "List the media this project knows about. A mediaRef here is what \
add_clips takes. `resolved` is false when the file is missing from disk, in which case \
clips using it render as black."
    )]
    async fn get_media(
        &self,
        Parameters(_args): Parameters<GetMediaArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self.session.lock().await;
        if !session.is_open() {
            return refused(NO_PROJECT);
        }
        let dir = session.package_dir();
        let media: Vec<Value> = session
            .manifest()
            .entries
            .iter()
            .map(|e| {
                let path = e.source.resolve(dir.as_deref());
                let mut row = json!({
                    "mediaRef": e.id,
                    "name": e.name,
                    "durationSeconds": e.duration,
                    "resolved": path.as_ref().is_some_and(|p| p.is_file()),
                });
                if let Some(width) = e.source_width {
                    row["width"] = json!(width);
                }
                if let Some(height) = e.source_height {
                    row["height"] = json!(height);
                }
                if let Some(has_audio) = e.has_audio {
                    row["hasAudio"] = json!(has_audio);
                }
                row
            })
            .collect();
        ok(json!({ "media": media }))
    }

    #[tool(
        description = "Find the silent stretches of a media file, reported in timeline \
frames ready to hand straight to ripple_delete_ranges.\n\n\
This is how you tighten talking-head footage: detect the pauses, then cut them. Each \
silence is shrunk by `paddingSeconds` at both ends so a cut does not clip the start or \
end of a word — the 0.15s default leaves a breath of room. `speech` lists the inverse, \
the spans that carry sound.\n\n\
Frames are computed at the open timeline's frame rate, so the ranges need no conversion \
before you cut with them."
    )]
    async fn detect_silence(
        &self,
        Parameters(args): Parameters<DetectSilenceArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self.session.lock().await;
        let Ok(timeline) = session.active_timeline() else {
            return refused(NO_PROJECT);
        };
        let fps = timeline.fps.max(1);

        let resolve = session.resolver();
        let Some(media) = resolve(&args.media_ref) else {
            return refused(format!(
                "`{}` is not in this project's media, or its file is missing from disk",
                args.media_ref
            ));
        };
        let info = match palmier_media::probe(&media.path) {
            Ok(info) => info,
            Err(error) => return refused(error.to_string()),
        };
        if !info.has_audio {
            return refused(format!("`{}` has no audio track", args.media_ref));
        }

        let padding = args.padding_seconds.unwrap_or(0.15).max(0.0);
        let silences = match palmier_media::detect_silence(
            &media.path,
            args.noise_db.unwrap_or(-30.0),
            args.min_seconds.unwrap_or(0.5),
            info.duration_seconds,
        ) {
            Ok(spans) => spans,
            Err(error) => return refused(error.to_string()),
        };
        let speech = palmier_media::speech_spans(&silences, info.duration_seconds);

        // Shrinking by the padding is what keeps a cut off the edge of a word.
        let to_range = |span: &palmier_media::SilentSpan, pad: f64| -> Option<Value> {
            let start = (span.start_seconds + pad).max(0.0);
            let end = (span.end_seconds - pad).min(info.duration_seconds);
            if end <= start {
                return None;
            }
            Some(json!({
                "startFrame": (start * fps as f64).round() as i64,
                "endFrame": (end * fps as f64).round() as i64,
                "startSeconds": start,
                "endSeconds": end,
                "durationSeconds": end - start,
            }))
        };

        ok(json!({
            "mediaRef": args.media_ref,
            "fps": fps,
            "durationSeconds": info.duration_seconds,
            "paddingSeconds": padding,
            "silences": silences.iter().filter_map(|s| to_range(s, padding)).collect::<Vec<_>>(),
            "speech": speech.iter().filter_map(|s| to_range(s, 0.0)).collect::<Vec<_>>(),
        }))
    }

    #[tool(
        description = "Render the timeline to a video file. Clips are composited bottom \
track up, gaps render black, and audio is mixed. Any clip whose media is missing from \
disk is reported in `missingMedia` rather than silently omitted."
    )]
    async fn export_project(
        &self,
        Parameters(args): Parameters<ExportProjectArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self.session.lock().await;
        let Ok(timeline) = session.active_timeline().cloned() else {
            return refused(NO_PROJECT);
        };

        let mut options = palmier_media::RenderOptions::new(&args.output);
        if let Some(codec) = args.codec {
            options.codec = codec;
        }
        if let Some(crf) = args.crf {
            if crf > 51 {
                return refused(format!("crf {crf} is out of range; use 0–51"));
            }
            options.crf = crf;
        }

        let resolve = session.resolver();
        match palmier_media::render(&timeline, &resolve, &options) {
            Ok(report) => {
                let mut out = json!({
                    "status": "exported",
                    "output": report.output.display().to_string(),
                    "durationSeconds": report.duration_seconds,
                    "width": report.width,
                    "height": report.height,
                    "fps": report.fps,
                });
                if !report.missing_media.is_empty() {
                    out["missingMedia"] = json!(report.missing_media);
                }
                ok(out)
            }
            Err(error) => refused(error.to_string()),
        }
    }

    #[tool(
        description = "Link or unlink clips so they behave as one. A linked video and \
audio pair move, trim, split, and delete together, keeping their relative offset — this \
is what stops dialogue drifting out of sync when you rearrange a cut. `unlink` expands \
to the whole group, so unlinking any member unlinks all of them."
    )]
    async fn manage_clip_links(
        &self,
        Parameters(args): Parameters<ManageClipLinksArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = match args.action.as_str() {
            "link" => EditCommand::LinkClips {
                clip_ids: args.clip_ids,
            },
            "unlink" => EditCommand::UnlinkClips {
                clip_ids: args.clip_ids,
            },
            other => return refused(format!("unknown action `{other}`; expected link or unlink")),
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Add, update, or remove a marker — a persistent note on the \
timeline. A point marker has durationFrames 0; a range marker spans \
[startFrame, startFrame + durationFrames). Markers move with the content they annotate \
when a ripple edit shifts it, so a note stays on the shot it was about."
    )]
    async fn manage_markers(
        &self,
        Parameters(args): Parameters<ManageMarkersArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let input = palmier_core::edit::MarkerInput {
            name: args.name,
            comment: args.comment,
            start_frame: args.start_frame,
            duration_frames: args.duration_frames,
            color: None,
            resolved: args.resolved,
        };
        let command = match args.action.as_str() {
            "add" => EditCommand::AddMarker {
                marker: Box::new(input),
            },
            "update" => match args.marker_id {
                Some(marker_id) => EditCommand::UpdateMarker {
                    marker_id,
                    marker: Box::new(input),
                },
                None => return refused("`update` needs a markerId"),
            },
            "remove" => {
                let ids = args
                    .marker_ids
                    .or_else(|| args.marker_id.map(|id| vec![id]))
                    .unwrap_or_default();
                if ids.is_empty() {
                    return refused("`remove` needs a markerId or markerIds");
                }
                EditCommand::RemoveMarkers { marker_ids: ids }
            }
            other => {
                return refused(format!(
                    "unknown action `{other}`; expected add, update, or remove"
                ));
            }
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Change the active timeline's frame rate, canvas size, or name. \
Frame positions are not rescaled: changing fps changes how long the existing frame \
counts last, it does not retime the cut."
    )]
    async fn set_project_settings(
        &self,
        Parameters(args): Parameters<SetProjectSettingsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = EditCommand::SetTimelineSettings {
            fps: args.fps,
            width: args.width,
            height: args.height,
            name: args.name,
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Add another timeline to the project. A project can hold several \
— a main cut and alternates, say. The new timeline starts empty and does not become \
active; use set_active_timeline for that."
    )]
    async fn create_timeline(
        &self,
        Parameters(args): Parameters<CreateTimelineArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = EditCommand::CreateTimeline {
            name: args.name,
            fps: args.fps.unwrap_or(30),
            width: args.width.unwrap_or(1920),
            height: args.height.unwrap_or(1080),
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Switch which timeline every other tool reads and edits. \
get_timeline reports the one that is active."
    )]
    async fn set_active_timeline(
        &self,
        Parameters(args): Parameters<SetActiveTimelineArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = EditCommand::SetActiveTimeline {
            timeline_id: args.timeline_id,
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Point clips at a different media asset, keeping their position, \
duration, trims, and look. Use it to swap a proxy for the real footage, or one take for \
another of the same length."
    )]
    async fn swap_clip_media(
        &self,
        Parameters(args): Parameters<SwapClipMediaArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = EditCommand::SwapClipMedia {
            clip_ids: args.clip_ids,
            media_ref: args.media_ref,
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Copy one clip's look onto others — opacity, volume, speed, fades, \
transform, crop, edges, blend mode, and effects. Position, duration, trims, and \
identity are never copied, so the targets keep their place in the cut."
    )]
    async fn copy_clip_settings(
        &self,
        Parameters(args): Parameters<CopyClipSettingsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let command = EditCommand::CopyClipSettings {
            from_clip_id: args.from_clip_id,
            to_clip_ids: args.to_clip_ids,
        };
        self.run(&mut session, command).await
    }

    #[tool(
        description = "Describe one media asset in detail: duration, resolution, frame \
rate, whether it carries audio, and where its file is. get_media lists everything; this \
answers questions about one of them."
    )]
    async fn inspect_media(
        &self,
        Parameters(args): Parameters<InspectMediaArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self.session.lock().await;
        if !session.is_open() {
            return refused(NO_PROJECT);
        }
        let dir = session.package_dir();
        let Some(entry) = session
            .manifest()
            .entries
            .iter()
            .find(|e| e.id == args.media_ref)
        else {
            return refused(format!(
                "`{}` is not in this project's media",
                args.media_ref
            ));
        };
        let path = entry.source.resolve(dir.as_deref());
        let resolved = path.as_ref().is_some_and(|p| p.is_file());

        let mut out = json!({
            "mediaRef": entry.id,
            "name": entry.name,
            "durationSeconds": entry.duration,
            "resolved": resolved,
            "path": path.as_ref().map(|p| p.display().to_string()),
        });
        // Probe live rather than trusting the manifest: the file may have changed.
        if let Some(path) = path.filter(|p| p.is_file())
            && let Ok(info) = palmier_media::probe(&path)
        {
            out["width"] = json!(info.width);
            out["height"] = json!(info.height);
            out["fps"] = json!(info.fps);
            out["hasAudio"] = json!(info.has_audio);
            out["hasVideo"] = json!(info.has_video);
            out["durationSeconds"] = json!(info.duration_seconds);
        }
        ok(out)
    }

    #[tool(
        description = "Write one composited frame of the timeline to a PNG file. Unlike \
inspect_timeline, which hands you the picture to look at, this saves it — for a \
thumbnail, a still, or a frame to bring back in as media."
    )]
    async fn capture_frame(
        &self,
        Parameters(args): Parameters<CaptureFrameArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let session = self.session.lock().await;
        let Ok(timeline) = session.active_timeline().cloned() else {
            return refused(NO_PROJECT);
        };
        if args.frame < 0 {
            return refused(format!(
                "frame {} is before the start of the timeline",
                args.frame
            ));
        }
        let output = PathBuf::from(&args.output);
        let resolve = session.resolver();
        // Full canvas and no grid: this is a still to keep, not a diagram to read.
        let options = palmier_media::FrameOptions {
            grid: false,
            max_width: 0,
        };
        match palmier_media::render_frame(&timeline, &resolve, args.frame, &output, options) {
            Ok(()) => ok(json!({
                "status": "captured",
                "output": output.display().to_string(),
                "frame": args.frame,
                "width": timeline.width,
                "height": timeline.height,
            })),
            Err(error) => refused(error.to_string()),
        }
    }

    #[tool(
        description = "Generate an image and add it to the project's media, ready for \
add_clips.\n\n\
Generation takes minutes, so this returns a jobId at once rather than making you wait. \
Poll it with manage_jobs; when the job is done its result carries the mediaRef.\n\n\
Keys are yours and are configured with manage_keys. When several are set, they are used \
one at a time and the next is tried only when the current one fails — whether from \
exhausted quota or a passing error."
    )]
    async fn generate_image(
        &self,
        Parameters(args): Parameters<GenerateImageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if args.prompt.trim().is_empty() {
            return refused("`prompt` is empty");
        }
        let (media_dir, package) = {
            let session = self.session.lock().await;
            if !session.is_open() {
                return refused(NO_PROJECT);
            }
            match session.package_dir() {
                Some(dir) => (dir.join("media"), dir),
                // A project held only in memory has nowhere to put the file.
                None => {
                    return refused(
                        "save the project first — a generated image needs somewhere to live",
                    );
                }
            }
        };

        let keys = palmier_gen::KeyRing::load("stitch", "STITCH_API_KEY");
        if keys.is_empty() {
            return refused(
                "no Stitch keys configured — add one with manage_keys before generating",
            );
        }
        let provider = match palmier_gen::Stitch::new(keys, "Palmier") {
            Ok(provider) => provider,
            Err(error) => return refused(error.to_string()),
        };

        let label = args.prompt.chars().take(60).collect::<String>();
        let (job_id, cancel) = self.jobs.submit("generate_image", label).await;

        let jobs = self.jobs.clone();
        let session = self.session.clone();
        let id = job_id.clone();
        let prompt = args.prompt.clone();
        let rounds = args.rounds.unwrap_or(2);

        tokio::spawn(async move {
            let generated = tokio::select! {
                result = provider.generate(&prompt, rounds) => result,
                _ = cancel.cancelled() => return,
            };
            let image = match generated {
                Ok(image) => image,
                Err(error) => return jobs.fail(&id, error.to_string()).await,
            };
            if cancel.is_cancelled() {
                return;
            }

            // Land it in the package so the manifest can reference it relatively.
            let name = format!("generated-{}.{}", &id[..8], image.extension);
            let path = media_dir.join(&name);
            if let Err(error) = std::fs::create_dir_all(&media_dir)
                .and_then(|()| std::fs::write(&path, &image.bytes))
            {
                return jobs
                    .fail(&id, format!("cannot write {}: {error}", path.display()))
                    .await;
            }

            let info = match palmier_media::probe(&path) {
                Ok(info) => info,
                Err(error) => return jobs.fail(&id, error.to_string()).await,
            };
            let media_ref = uuid::Uuid::new_v4().to_string();
            let entry = session::entry_for(media_ref.clone(), &path, &info, Some(&package));
            session.lock().await.add_media(entry);

            jobs.succeed(
                &id,
                json!({
                    "mediaRef": media_ref,
                    "name": name,
                    "path": path.display().to_string(),
                    "width": image.width.or(info.width),
                    "height": image.height.or(info.height),
                }),
            )
            .await;
        });

        ok(json!({
            "status": "queued",
            "jobId": job_id,
            "detail": "generation runs in the background; poll manage_jobs for the mediaRef",
        }))
    }

    #[tool(
        description = "List background jobs, check one, or cancel one. Generation runs \
here; a job that is `done` carries its result, and one that `failed` carries why."
    )]
    async fn manage_jobs(
        &self,
        Parameters(args): Parameters<ManageJobsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        match args.action.as_deref().unwrap_or("list") {
            "list" => {
                let jobs = self.jobs.list().await;
                ok(json!({
                    "running": self.jobs.running_count().await,
                    "jobs": jobs.iter().map(|j| j.render()).collect::<Vec<_>>(),
                }))
            }
            "status" => {
                let Some(job_id) = args.job_id else {
                    return refused("`status` needs a jobId");
                };
                match self.jobs.get(&job_id).await {
                    Some(job) => ok(job.render()),
                    None => refused(format!("unknown job `{job_id}`")),
                }
            }
            "cancel" => {
                let Some(job_id) = args.job_id else {
                    return refused("`cancel` needs a jobId");
                };
                if self.jobs.cancel(&job_id).await {
                    ok(json!({ "status": "cancelled", "jobId": job_id }))
                } else {
                    refused(format!("job `{job_id}` is unknown or already finished"))
                }
            }
            other => refused(format!(
                "unknown action `{other}`; expected list, status, or cancel"
            )),
        }
    }

    #[tool(
        description = "See which provider keys are configured, add one, or remove one. \
Keys are stored in the operating system's keychain and are never echoed back — listing \
shows only how many there are and a few characters of each, enough to tell them apart.\n\n\
Several keys for one provider are used one at a time, moving to the next only when the \
current one fails."
    )]
    async fn manage_keys(
        &self,
        Parameters(args): Parameters<ManageKeysArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let provider = args.provider.as_deref().unwrap_or("stitch");
        if provider != "stitch" {
            return refused(format!("unknown provider `{provider}`; only stitch today"));
        }
        let env_prefix = "STITCH_API_KEY";

        match args.action.as_deref().unwrap_or("list") {
            "list" => {
                let ring = palmier_gen::KeyRing::load(provider, env_prefix);
                ok(json!({
                    "provider": provider,
                    "count": ring.len(),
                    "keys": ring.hints(),
                }))
            }
            "set" => {
                let Some(key) = args.key else {
                    return refused("`set` needs a key");
                };
                let slot = args
                    .slot
                    .unwrap_or_else(|| palmier_gen::KeyRing::load(provider, env_prefix).len() + 1);
                match palmier_gen::KeyRing::store(provider, slot.max(1), &key) {
                    Ok(()) => ok(json!({ "status": "stored", "provider": provider, "slot": slot })),
                    Err(error) => refused(error.to_string()),
                }
            }
            "forget" => {
                let Some(slot) = args.slot else {
                    return refused("`forget` needs a slot");
                };
                match palmier_gen::KeyRing::forget(provider, slot) {
                    Ok(()) => {
                        ok(json!({ "status": "forgotten", "provider": provider, "slot": slot }))
                    }
                    Err(error) => refused(error.to_string()),
                }
            }
            other => refused(format!(
                "unknown action `{other}`; expected list, set, or forget"
            )),
        }
    }

    #[tool(
        description = "Step back one change, or set `redo` true to reapply the last one \
undone. Commands that were refused or changed nothing are not in the history."
    )]
    async fn undo(
        &self,
        Parameters(args): Parameters<UndoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        let edit = session.edit().map_err(|e| invalid(e.to_string()))?;
        let redo = args.redo.unwrap_or(false);
        let outcome = if redo { edit.redo() } else { edit.undo() };
        match outcome {
            Some(receipt) => {
                session.mark_dirty();
                ok(json!({
                    "status": if redo { "redone" } else { "undone" },
                    "change": render::receipt(&receipt),
                }))
            }
            None => ok(json!({
                "status": "no_op",
                "detail": if redo { "nothing to redo" } else { "nothing to undo" },
            })),
        }
    }
}

impl Palmier {
    /// The one place a tool reaches the mutation path.
    async fn run(
        &self,
        session: &mut Session,
        command: EditCommand,
    ) -> Result<CallToolResult, ErrorData> {
        let edit = session.edit().map_err(|e| invalid(e.to_string()))?;
        match edit.apply(command) {
            Ok(receipt) => {
                if !receipt.is_no_op() {
                    session.mark_dirty();
                }
                ok(render::receipt(&receipt))
            }
            // A refusal is a legitimate outcome, not a protocol error: the agent needs to
            // read the reason and try something else.
            Err(reason) => ok(render::refusal(&reason)),
        }
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "palmier",
    instructions = "Edit a video project by talking. Call manage_project with action \
'open' first, then get_timeline. Frames are integers and clip ranges are half-open: \
[startFrame, endFrame), so duration = end − start. Prefer trackId and clipId over \
positional indexes. A tool that reports 'refused' changed nothing; one that reports \
'no_op' was applied and changed nothing."
)]
impl ServerHandler for Palmier {}

// ------------------------------------------------------------------ helpers

fn parse_clip_type(value: &str) -> Result<ClipType, ErrorData> {
    Ok(match value {
        "video" => ClipType::Video,
        "audio" => ClipType::Audio,
        "image" => ClipType::Image,
        "text" => ClipType::Text,
        "lottie" => ClipType::Lottie,
        "sequence" => ClipType::Sequence,
        "subtitle" => ClipType::Subtitle,
        other => return Err(invalid(format!("unknown track type `{other}`"))),
    })
}

/// Resolve a track by id, or by index for compatibility, into a stable id (FR-008).
fn resolve_track(
    timeline: &palmier_core::Timeline,
    track_id: Option<&str>,
    track_index: Option<usize>,
) -> Result<String, ErrorData> {
    if let Some(id) = track_id {
        return if timeline.tracks.iter().any(|t| t.id.as_deref() == Some(id)) {
            Ok(id.to_string())
        } else {
            Err(invalid(format!("unknown trackId `{id}`")))
        };
    }
    if let Some(index) = track_index {
        return timeline
            .tracks
            .get(index)
            .and_then(|t| t.id.clone())
            .ok_or_else(|| invalid(format!("no track at index {index}")));
    }
    Err(invalid("give a trackId (preferred) or a trackIndex"))
}

fn build_clip(entry: &AddClipEntry) -> Result<Clip, ErrorData> {
    let media_type = match entry.media_type.as_deref() {
        None => ClipType::Video,
        Some(kind) => parse_clip_type(kind)?,
    };
    let json = json!({
        "timelines": [{
            "id": "t", "fps": 30, "width": 1920, "height": 1080,
            "tracks": [{ "type": "video", "clips": [{
                "mediaRef": entry.media_ref,
                "mediaType": render::clip_type(media_type),
                "startFrame": entry.start_frame,
                "durationFrames": entry.end_frame - entry.start_frame,
            }]}]
        }]
    });
    let bytes = serde_json::to_vec(&json).map_err(|e| invalid(e.to_string()))?;
    let project = palmier_core::ProjectFile::decode(&bytes).map_err(|e| invalid(e.to_string()))?;
    Ok(project.timelines[0].tracks[0].clips[0].clone())
}

/// Build the MCP service over shared state.
///
/// The session is created once by the caller and closed over here, so every connecting
/// client edits the same project rather than one of its own. `rmcp` stays an
/// implementation detail of this crate; callers only see an `axum::Router`.
pub fn mcp_router(session: Arc<Mutex<Session>>, jobs: jobs::Jobs) -> axum::Router {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
    };

    let service = StreamableHttpService::new(
        move || Ok(Palmier::shared(session.clone(), jobs.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    axum::Router::new().nest_service("/mcp", service)
}

/// The MCP surface alone, over fresh state. Convenient for tests.
pub fn http_router() -> axum::Router {
    mcp_router(Arc::new(Mutex::new(Session::default())), jobs::Jobs::new())
}

/// Serve MCP over stdin/stdout.
///
/// This is how a desktop client runs a local server: it spawns the process and speaks
/// over the pipes. Claude Desktop's custom connectors require HTTPS, which a loopback
/// server cannot offer and should not need, so stdio — not HTTP — is the supported
/// transport there.
///
/// Nothing may be written to stdout except protocol traffic; a stray `println!` corrupts
/// the stream.
pub async fn serve_stdio() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use rmcp::ServiceExt;
    let service = Palmier::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
