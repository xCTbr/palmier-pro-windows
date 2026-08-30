//! Constitution principle IV: integer frames, half-open ranges, no panics.

use palmier_core::frames::{FrameError, FrameRange, timecode};

#[test]
fn duration_is_end_minus_start() {
    let r = FrameRange::from_duration(10, 30).unwrap();
    assert_eq!(r.start(), 10);
    assert_eq!(r.end(), 40);
    assert_eq!(r.duration(), 30);
}

#[test]
fn containment_is_half_open() {
    let r = FrameRange::from_duration(10, 30).unwrap();
    assert!(!r.contains(9));
    assert!(r.contains(10), "start is inside");
    assert!(r.contains(39));
    assert!(!r.contains(40), "end is outside");
}

#[test]
fn zero_duration_range_contains_nothing() {
    let r = FrameRange::from_duration(10, 0).unwrap();
    assert!(r.is_empty());
    assert!(!r.contains(10));
}

#[test]
fn negative_duration_is_rejected() {
    assert_eq!(
        FrameRange::from_duration(0, -1),
        Err(FrameError::NegativeDuration(-1))
    );
}

#[test]
fn overflow_is_an_error_not_a_panic() {
    assert_eq!(
        FrameRange::from_duration(i64::MAX, 1),
        Err(FrameError::Overflow {
            start: i64::MAX,
            duration: 1
        })
    );
}

#[test]
fn adjacent_ranges_do_not_overlap() {
    let a = FrameRange::from_duration(0, 30).unwrap();
    let b = FrameRange::from_duration(30, 30).unwrap();
    assert!(
        !a.overlaps(b),
        "[0,30) and [30,60) are adjacent, not overlapping"
    );
    let c = FrameRange::from_duration(29, 30).unwrap();
    assert!(a.overlaps(c));
}

#[test]
fn timecode_formats_at_rate() {
    assert_eq!(timecode(0, 30).unwrap(), "00:00:00:00");
    assert_eq!(timecode(29, 30).unwrap(), "00:00:00:29");
    assert_eq!(timecode(30, 30).unwrap(), "00:00:01:00");
    assert_eq!(
        timecode(3600 * 30 + 61 * 30 + 5, 30).unwrap(),
        "01:01:01:05"
    );
}

#[test]
fn timecode_rejects_non_positive_fps() {
    assert_eq!(timecode(0, 0), Err(FrameError::NonPositiveFps(0)));
}

#[test]
fn timecode_handles_negative_frames_without_panicking() {
    assert_eq!(timecode(-30, 30).unwrap(), "-00:00:01:00");
    assert!(
        timecode(i64::MIN, 30).is_ok(),
        "i64::MIN must not overflow on negation"
    );
}
