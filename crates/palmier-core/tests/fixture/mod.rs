//! Shared builders for edit-layer tests.
#![allow(dead_code)]

use palmier_core::ProjectFile;
use palmier_core::edit::EditSession;
use palmier_core::timeline::Clip;

pub struct TrackSpec {
    pub id: String,
    pub kind: String,
    pub clips: Vec<(String, i64, i64)>,
}

pub fn track(id: &str, kind: &str, clips: &[(&str, i64, i64)]) -> TrackSpec {
    TrackSpec {
        id: id.into(),
        kind: kind.into(),
        clips: clips
            .iter()
            .map(|(i, s, d)| ((*i).to_string(), *s, *d))
            .collect(),
    }
}

pub fn session(tracks: &[TrackSpec]) -> EditSession {
    let rendered: Vec<String> = tracks
        .iter()
        .map(|t| {
            let clips: Vec<String> = t
                .clips
                .iter()
                .map(|(id, start, duration)| {
                    let media = if t.kind == "audio" { "audio" } else { "video" };
                    format!(
                        r#"{{"id":"{id}","mediaRef":"m-{id}","mediaType":"{media}",
                            "startFrame":{start},"durationFrames":{duration}}}"#
                    )
                })
                .collect();
            format!(
                r#"{{"id":"{}","type":"{}","clips":[{}]}}"#,
                t.id,
                t.kind,
                clips.join(",")
            )
        })
        .collect();
    let json = format!(
        r#"{{"timelines":[{{"id":"tl","fps":30,"width":1920,"height":1080,"tracks":[{}]}}],
            "activeTimelineId":"tl"}}"#,
        rendered.join(",")
    );
    EditSession::new(ProjectFile::decode(json.as_bytes()).expect("fixture must parse"))
}

/// A video clip on `v1` linked to an audio clip on `a1`, both `[0, 100)`.
pub fn linked_session() -> EditSession {
    let json = r#"{"timelines":[{"id":"tl","fps":30,"width":1920,"height":1080,"tracks":[
        {"id":"v1","type":"video","clips":[
          {"id":"v","mediaRef":"m","mediaType":"video","startFrame":0,"durationFrames":100,
           "linkGroupId":"g1"}]},
        {"id":"a1","type":"audio","clips":[
          {"id":"a","mediaRef":"m","mediaType":"audio","startFrame":0,"durationFrames":100,
           "linkGroupId":"g1"}]}
      ]}],"activeTimelineId":"tl"}"#;
    EditSession::new(ProjectFile::decode(json.as_bytes()).unwrap())
}

pub fn new_clip(id: &str, start: i64, duration: i64) -> Clip {
    let json = format!(
        r#"{{"timelines":[{{"id":"t","fps":30,"width":1920,"height":1080,"tracks":[
          {{"type":"video","clips":[{{"id":"{id}","mediaRef":"m-{id}","startFrame":{start},
             "durationFrames":{duration}}}]}}]}}]}}"#
    );
    ProjectFile::decode(json.as_bytes()).unwrap().timelines[0].tracks[0].clips[0].clone()
}

pub fn clips_of(session: &EditSession, track_id: &str) -> Vec<Clip> {
    session.project.timelines[0]
        .tracks
        .iter()
        .find(|t| t.id.as_deref() == Some(track_id))
        .map(|t| t.clips.clone())
        .unwrap_or_default()
}

pub fn clip_at(session: &EditSession, track_id: &str, index: usize) -> Clip {
    clips_of(session, track_id)[index].clone()
}

pub fn clip_by_id(session: &EditSession, clip_id: &str) -> Clip {
    session.project.timelines[0]
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .find(|c| c.id.as_deref() == Some(clip_id))
        .unwrap_or_else(|| panic!("no clip `{clip_id}`"))
        .clone()
}

pub fn starts(clips: &[Clip]) -> Vec<i64> {
    clips.iter().map(|c| c.start_frame).collect()
}
