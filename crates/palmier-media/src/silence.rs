//! Finding the quiet parts, via `ffmpeg`'s `silencedetect` filter.
//!
//! Cutting dead air out of talking-head footage is the most common edit of its kind,
//! and until this existed an agent had to shell out to ffmpeg itself and convert the
//! seconds it got back into frames by hand.

use std::path::Path;
use std::process::Command;

use crate::{MediaError, require_tool};

/// A quiet span of the source, in source seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilentSpan {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

impl SilentSpan {
    pub fn duration_seconds(&self) -> f64 {
        self.end_seconds - self.start_seconds
    }
}

/// Parse `silencedetect`'s stderr chatter.
///
/// It reports `silence_start:` and `silence_end:` on separate lines, and a silence that
/// runs to the end of the file has a start with no matching end.
fn parse(log: &str, total_seconds: f64) -> Vec<SilentSpan> {
    let mut spans = Vec::new();
    let mut open: Option<f64> = None;

    for line in log.lines() {
        if let Some(rest) = line.split("silence_start:").nth(1) {
            open = rest.split_whitespace().next().and_then(|v| v.parse().ok());
        } else if let Some(rest) = line.split("silence_end:").nth(1)
            && let Some(end) = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
            && let Some(start) = open.take()
            && end > start
        {
            spans.push(SilentSpan {
                start_seconds: start,
                end_seconds: end,
            });
        }
    }

    // A trailing silence never gets its `silence_end`.
    if let Some(start) = open
        && total_seconds > start
    {
        spans.push(SilentSpan {
            start_seconds: start,
            end_seconds: total_seconds,
        });
    }
    spans
}

/// Find silences in `path`.
///
/// `noise_db` is the threshold below which audio counts as silence — around `-30` for
/// a normal room, lower for a noisy one. `min_seconds` ignores pauses shorter than a
/// natural gap between words.
pub fn detect(
    path: &Path,
    noise_db: f64,
    min_seconds: f64,
    total_seconds: f64,
) -> Result<Vec<SilentSpan>, MediaError> {
    require_tool("ffmpeg")?;
    if !min_seconds.is_finite() || min_seconds <= 0.0 {
        return Err(MediaError::Invalid(format!(
            "minimum silence must be a positive number of seconds, got {min_seconds}"
        )));
    }
    if !noise_db.is_finite() {
        return Err(MediaError::Invalid(
            "the noise threshold must be a finite number".into(),
        ));
    }

    let output = Command::new("ffmpeg")
        .args(["-v", "info", "-nostdin"])
        .arg("-i")
        .arg(path)
        .arg("-af")
        .arg(format!("silencedetect=noise={noise_db}dB:d={min_seconds}"))
        .args(["-f", "null", "-"])
        .output()
        .map_err(|_| MediaError::ToolMissing { tool: "ffmpeg" })?;

    if !output.status.success() {
        return Err(MediaError::ToolFailed {
            tool: "ffmpeg",
            message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    // silencedetect reports on stderr even on success.
    Ok(parse(
        &String::from_utf8_lossy(&output.stderr),
        total_seconds,
    ))
}

/// Invert a list of silences into the spans that carry sound.
pub fn speech_spans(silences: &[SilentSpan], total_seconds: f64) -> Vec<SilentSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0.0;
    for silence in silences {
        if silence.start_seconds > cursor {
            spans.push(SilentSpan {
                start_seconds: cursor,
                end_seconds: silence.start_seconds,
            });
        }
        cursor = cursor.max(silence.end_seconds);
    }
    if total_seconds > cursor {
        spans.push(SilentSpan {
            start_seconds: cursor,
            end_seconds: total_seconds,
        });
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
[silencedetect @ 0x1] silence_start: 1.5
[silencedetect @ 0x1] silence_end: 3.25 | silence_duration: 1.75
[silencedetect @ 0x1] silence_start: 8.0
[silencedetect @ 0x1] silence_end: 9.5 | silence_duration: 1.5
";

    #[test]
    fn parses_paired_spans() {
        let spans = parse(LOG, 20.0);
        assert_eq!(spans.len(), 2);
        assert_eq!(
            spans[0],
            SilentSpan {
                start_seconds: 1.5,
                end_seconds: 3.25
            }
        );
        assert_eq!(spans[1].end_seconds, 9.5);
    }

    #[test]
    fn a_silence_running_to_the_end_has_no_end_line() {
        let spans = parse("[silencedetect] silence_start: 12.0\n", 15.0);
        assert_eq!(
            spans,
            vec![SilentSpan {
                start_seconds: 12.0,
                end_seconds: 15.0
            }]
        );
    }

    #[test]
    fn an_unterminated_silence_past_the_end_is_dropped() {
        assert!(parse("[silencedetect] silence_start: 99.0\n", 15.0).is_empty());
    }

    #[test]
    fn noise_without_any_silence_yields_nothing() {
        assert!(parse("frame= 100 fps=0.0 q=-1.0 size=N/A\n", 10.0).is_empty());
    }

    #[test]
    fn speech_is_the_inverse_of_silence() {
        let silences = parse(LOG, 20.0);
        let speech = speech_spans(&silences, 20.0);
        assert_eq!(speech.len(), 3, "{speech:?}");
        assert_eq!(
            speech[0],
            SilentSpan {
                start_seconds: 0.0,
                end_seconds: 1.5
            }
        );
        assert_eq!(
            speech[2],
            SilentSpan {
                start_seconds: 9.5,
                end_seconds: 20.0
            }
        );
    }

    #[test]
    fn speech_of_a_wholly_silent_file_is_empty() {
        let silences = vec![SilentSpan {
            start_seconds: 0.0,
            end_seconds: 10.0,
        }];
        assert!(speech_spans(&silences, 10.0).is_empty());
    }
}
