//! Building an `ffmpeg -filter_complex` graph from a timeline.
//!
//! The graph is a string, so it is asserted in tests without decoding a frame. The
//! rules below were established by measuring, not by reading documentation:
//!
//! **A clip is positioned with `setpts`, never with `overlay=enable=`.** `enable` only
//! gates visibility; the clip's frames still start at PTS 0, so a clip placed later on
//! the timeline shows its last frame frozen for its whole span. Offsetting the
//! presentation timestamp is what actually moves it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use palmier_core::timeline::{ClipType, Timeline};

/// What a `mediaRef` resolves to. The graph must know whether a file actually carries
/// audio: emitting `[N:a]` for a file without an audio stream makes ffmpeg reject the
/// whole filtergraph, not just that chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMedia {
    pub path: PathBuf,
    pub has_audio: bool,
    pub has_video: bool,
}

impl ResolvedMedia {
    pub fn new(path: impl Into<PathBuf>, has_audio: bool, has_video: bool) -> Self {
        Self {
            path: path.into(),
            has_audio,
            has_video,
        }
    }
}

/// One `-i` on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub index: usize,
    pub path: PathBuf,
}

/// A complete render plan: what to feed ffmpeg and what to ask it for.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterGraph {
    pub inputs: Vec<Input>,
    pub filter_complex: String,
    pub video_label: String,
    pub audio_label: Option<String>,
    pub duration_seconds: f64,
    pub width: i64,
    pub height: i64,
    pub fps: i64,
}

/// Seconds, formatted so ffmpeg never sees exponential notation.
fn seconds(frames: i64, fps: i64) -> String {
    if fps <= 0 {
        return "0.000000".into();
    }
    format!("{:.6}", frames as f64 / fps as f64)
}

/// Build the graph for `timeline`, resolving each clip's `mediaRef` through `resolve`.
///
/// A clip whose media does not resolve is skipped and named in `missing`, so a render
/// reports what it could not include instead of silently producing a shorter film.
pub fn build(
    timeline: &Timeline,
    resolve: &dyn Fn(&str) -> Option<ResolvedMedia>,
) -> (FilterGraph, Vec<String>) {
    let fps = timeline.fps.max(1);
    let total_frames = timeline.total_frames().unwrap_or(0);
    let duration = seconds(total_frames, fps);
    let (width, height) = (timeline.width.max(2), timeline.height.max(2));

    let mut inputs: Vec<Input> = Vec::new();
    let mut input_index: BTreeMap<PathBuf, usize> = BTreeMap::new();
    let mut missing: Vec<String> = Vec::new();
    let mut chains: Vec<String> = Vec::new();
    let mut video_labels: Vec<String> = Vec::new();
    let mut audio_labels: Vec<String> = Vec::new();

    // Video tracks composite bottom-up, so the last track in the project is on top.
    for track in &timeline.tracks {
        if track.hidden {
            continue;
        }
        for clip in &track.clips {
            let Some(media) = resolve(&clip.media_ref) else {
                missing.push(clip.media_ref.clone());
                continue;
            };
            let index = *input_index.entry(media.path.clone()).or_insert_with(|| {
                let index = inputs.len();
                inputs.push(Input {
                    index,
                    path: media.path.clone(),
                });
                index
            });

            let visual = media.has_video
                && matches!(
                    clip.media_type,
                    ClipType::Video | ClipType::Image | ClipType::Sequence | ClipType::Lottie
                );
            // A video file without an audio stream contributes no audio chain.
            let audible =
                media.has_audio && matches!(clip.media_type, ClipType::Video | ClipType::Audio);

            // Source span consumed, scaled by speed; timeline span it occupies.
            let source_len = (clip.duration_frames as f64 * clip.speed).round() as i64;
            let in_start = seconds(clip.trim_start_frame, fps);
            let in_end = seconds(clip.trim_start_frame + source_len.max(0), fps);
            let at = seconds(clip.start_frame, fps);

            if visual && !track.muted {
                let label = format!("v{}", video_labels.len());
                let mut chain = format!(
                    "[{index}:v]trim=start={in_start}:end={in_end},setpts=PTS-STARTPTS+{at}/TB"
                );
                if clip.speed != 1.0 && clip.speed > 0.0 {
                    chain.push_str(&format!(",setpts={:.6}*PTS", 1.0 / clip.speed));
                }
                chain.push_str(&format!(
                    ",scale={width}:{height}:force_original_aspect_ratio=decrease,\
                     pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1"
                ));
                if clip.opacity < 1.0 {
                    chain.push_str(&format!(
                        ",format=yuva420p,colorchannelmixer=aa={:.4}",
                        clip.opacity
                    ));
                }
                chain.push_str(&format!("[{label}]"));
                chains.push(chain);
                video_labels.push(label);
            }

            if audible && !track.muted {
                let label = format!("a{}", audio_labels.len());
                let mut chain =
                    format!("[{index}:a]atrim=start={in_start}:end={in_end},asetpts=PTS-STARTPTS");
                let delay_ms = (clip.start_frame as f64 / fps as f64 * 1000.0).round() as i64;
                if delay_ms > 0 {
                    chain.push_str(&format!(",adelay={delay_ms}|{delay_ms}"));
                }
                if clip.volume != 1.0 && clip.volume.is_finite() {
                    chain.push_str(&format!(",volume={:.4}", clip.volume.max(0.0)));
                }
                chain.push_str(&format!("[{label}]"));
                chains.push(chain);
                audio_labels.push(label);
            }
        }
    }

    // A black base of the full duration guarantees the render is timeline-length even
    // when the timeline opens or ends with a gap.
    let mut graph = vec![format!(
        "color=c=black:s={width}x{height}:r={fps}:d={duration}[base]"
    )];
    graph.extend(chains);

    let mut current = "base".to_string();
    for (n, label) in video_labels.iter().enumerate() {
        let next = format!("ov{n}");
        // `eof_action=pass` keeps the base flowing once a clip ends, instead of
        // truncating the render at the first clip's end.
        graph.push(format!(
            "[{current}][{label}]overlay=eof_action=pass:shortest=0[{next}]"
        ));
        current = next;
    }
    let video_label = current;

    let audio_label = match audio_labels.len() {
        0 => None,
        1 => {
            let only = &audio_labels[0];
            graph.push(format!("[{only}]apad=whole_dur={duration}[aout]"));
            Some("aout".to_string())
        }
        n => {
            let joined: String = audio_labels.iter().map(|l| format!("[{l}]")).collect();
            graph.push(format!(
                "{joined}amix=inputs={n}:dropout_transition=0:normalize=0[amixed]"
            ));
            graph.push(format!("[amixed]apad=whole_dur={duration}[aout]"));
            Some("aout".to_string())
        }
    };

    (
        FilterGraph {
            inputs,
            filter_complex: graph.join(";"),
            video_label,
            audio_label,
            duration_seconds: total_frames as f64 / fps as f64,
            width,
            height,
            fps,
        },
        missing,
    )
}
