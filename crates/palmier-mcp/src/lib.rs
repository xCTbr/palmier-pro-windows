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

pub mod render;
pub mod session;

use palmier_core::edit::{
    ClipMove, ClipProperties, EditCommand, SplitPoint, TrackProperties, TrimEdge,
};
use palmier_core::timeline::{Clip, ClipType};
use session::Session;

pub const DEFAULT_PORT: u16 = 19789;

#[derive(Clone)]
pub struct Palmier {
    session: Arc<Mutex<Session>>,
    tool_router: ToolRouter<Self>,
}

impl Palmier {
    pub fn new() -> Self {
        Self {
            session: Arc::new(Mutex::new(Session::default())),
            tool_router: Self::tool_router(),
        }
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
    /// `open`, `save`, `close`, or `describe`.
    pub action: String,
    /// A `.palmier` folder or a `project.json`. Required for `open`; optional for `save`.
    #[serde(default)]
    pub path: Option<String>,
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
pub struct UndoArgs {
    /// When true, reapply the change that was last undone instead of undoing.
    #[serde(default)]
    pub redo: Option<bool>,
}

// -------------------------------------------------------------------- tools

#[tool_router]
impl Palmier {
    #[tool(
        description = "Open, save, close, or describe the project this session edits. \
Editing never writes to disk; the project is written only when you save it here."
    )]
    async fn manage_project(
        &self,
        Parameters(args): Parameters<ManageProjectArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut session = self.session.lock().await;
        match args.action.as_str() {
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
                "unknown action `{other}`; expected open, save, close, or describe"
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
        let Ok(project) = session.project() else {
            return refused("no project is open — call manage_project with action 'open' first");
        };
        let Some(timeline) = project.timelines.first() else {
            return refused("the project has no timelines");
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
            let timeline = edit
                .project
                .timelines
                .first()
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
            let timeline = edit
                .project
                .timelines
                .first()
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
            let timeline = edit
                .project
                .timelines
                .first()
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
            let timeline = edit
                .project
                .timelines
                .first()
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

/// Build the HTTP service that serves MCP. `rmcp` stays an implementation detail of
/// this crate; the binary only sees an `axum::Router`.
pub fn http_router() -> axum::Router {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
    };

    let service = StreamableHttpService::new(
        || Ok(Palmier::new()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    axum::Router::new().nest_service("/mcp", service)
}
