//! The two pure engines, ported from Swift. These are where the subtle rules live.

use palmier_core::edit::overwrite::{OverwriteAction, advance_trim, compute_overwrite};
use palmier_core::edit::ripple::{
    map_frame, merge_ranges, push, ripple_markers, shifts_for_ranges,
};
use palmier_core::frames::FrameRange;
use palmier_core::marker::{MarkerStatus, TimelineMarker};
use palmier_core::text::Rgba;
use palmier_core::timeline::Clip;

fn range(start: i64, end: i64) -> FrameRange {
    FrameRange::from_duration(start, end - start).unwrap()
}

fn clip(id: &str, start: i64, duration: i64) -> Clip {
    let json = format!(
        r#"{{"timelines":[{{"id":"t","fps":30,"width":1920,"height":1080,"tracks":[
          {{"type":"video","clips":[{{"id":"{id}","mediaRef":"m","startFrame":{start},
             "durationFrames":{duration}}}]}}]}}]}}"#
    );
    let p = palmier_core::ProjectFile::decode(json.as_bytes()).unwrap();
    p.timelines[0].tracks[0].clips[0].clone()
}

fn marker(id: &str, start: i64, duration: i64) -> TimelineMarker {
    TimelineMarker {
        id: id.into(),
        name: id.into(),
        start_frame: start,
        duration_frames: duration,
        color: Rgba::new(1.0, 1.0, 1.0, 1.0),
        comment: String::new(),
        status: MarkerStatus::Open,
        extra: Default::default(),
    }
}

// ---- merge_ranges ----

#[test]
fn adjacent_ranges_merge_not_only_overlapping() {
    // The original's condition is `start <= last.end`, so [0,10) and [10,20) become one.
    let merged = merge_ranges(&[range(0, 10), range(10, 20)]);
    assert_eq!(merged.len(), 1);
    assert_eq!((merged[0].start(), merged[0].end()), (0, 20));
}

#[test]
fn disjoint_ranges_stay_separate() {
    let merged = merge_ranges(&[range(0, 10), range(11, 20)]);
    assert_eq!(merged.len(), 2);
}

#[test]
fn unsorted_and_nested_ranges_merge_correctly() {
    let merged = merge_ranges(&[range(30, 40), range(0, 50), range(5, 10)]);
    assert_eq!(merged.len(), 1);
    assert_eq!((merged[0].start(), merged[0].end()), (0, 50));
}

// ---- shifts ----

#[test]
fn only_clips_entirely_after_a_range_shift() {
    let clips = vec![clip("overlapping", 5, 20), clip("after", 40, 10)];
    let shifts = shifts_for_ranges(&clips, &[range(10, 20)]);
    assert_eq!(
        shifts.len(),
        1,
        "the overlapping clip must not move: {shifts:?}"
    );
    assert_eq!(shifts[0].clip_id, "after");
    assert_eq!(shifts[0].new_start_frame, 30);
}

#[test]
fn shifts_accumulate_across_several_ranges() {
    let clips = vec![clip("c", 100, 10)];
    let shifts = shifts_for_ranges(&clips, &[range(0, 10), range(20, 40)]);
    assert_eq!(shifts[0].new_start_frame, 100 - 10 - 20);
}

#[test]
fn no_ranges_means_no_shifts() {
    assert!(shifts_for_ranges(&[clip("c", 0, 10)], &[]).is_empty());
}

#[test]
fn push_moves_only_clips_at_or_after_the_insert_frame() {
    let clips = vec![
        clip("before", 0, 10),
        clip("at", 30, 10),
        clip("after", 60, 10),
    ];
    let shifts = push(&clips, 30, 15, &[]);
    assert_eq!(shifts.len(), 2);
    assert_eq!(shifts[0].new_start_frame, 45);
    assert_eq!(shifts[1].new_start_frame, 75);
}

#[test]
fn push_honours_exclusions() {
    let clips = vec![clip("a", 30, 10), clip("b", 40, 10)];
    let shifts = push(&clips, 0, 5, &["a".to_string()]);
    assert_eq!(shifts.len(), 1);
    assert_eq!(shifts[0].clip_id, "b");
}

// ---- map_frame ----

#[test]
fn map_frame_subtracts_ranges_entirely_before_it() {
    assert_eq!(map_frame(100, &[range(0, 10), range(20, 30)]), 80);
}

#[test]
fn map_frame_collapses_a_straddling_range_onto_its_start() {
    // The range [10,50) contains frame 30, so 30 maps to 10.
    assert_eq!(map_frame(30, &[range(10, 50)]), 10);
}

#[test]
fn map_frame_floors_at_zero() {
    assert_eq!(map_frame(5, &[range(0, 100)]), 0);
}

// ---- markers ----

#[test]
fn point_marker_inside_a_removed_range_is_dropped() {
    let out = ripple_markers(&[marker("m", 15, 0)], &[vec![range(10, 20)]]);
    assert!(out.is_empty());
}

#[test]
fn point_marker_after_a_removed_range_shifts_back() {
    let out = ripple_markers(&[marker("m", 50, 0)], &[vec![range(10, 20)]]);
    assert_eq!(out[0].start_frame, 40);
}

#[test]
fn range_marker_that_would_collapse_is_dropped() {
    let out = ripple_markers(&[marker("m", 10, 10)], &[vec![range(0, 100)]]);
    assert!(out.is_empty());
}

#[test]
fn marker_survives_if_it_survives_on_any_track_and_takes_the_minimum() {
    // Track A removes the marker's span; track B does not. It survives, at the minimum.
    let out = ripple_markers(
        &[marker("m", 50, 0)],
        &[vec![range(0, 40)], vec![range(0, 10)]],
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].start_frame, 10, "minimum of 10 and 40");
}

#[test]
fn empty_track_ranges_leave_markers_untouched() {
    let markers = vec![marker("m", 50, 0)];
    assert_eq!(ripple_markers(&markers, &[]), markers);
    assert_eq!(ripple_markers(&markers, &[vec![]]), markers);
}

// ---- overwrite ----

#[test]
fn clip_entirely_inside_the_region_is_removed() {
    let actions = compute_overwrite(&[clip("c", 10, 10)], 0, 100);
    assert_eq!(
        actions,
        vec![OverwriteAction::Remove {
            clip_id: "c".into()
        }]
    );
}

#[test]
fn clip_overlapping_the_left_edge_is_trimmed_at_its_end() {
    let actions = compute_overwrite(&[clip("c", 0, 50)], 30, 100);
    assert_eq!(
        actions,
        vec![OverwriteAction::TrimEnd {
            clip_id: "c".into(),
            new_duration: 30
        }]
    );
}

#[test]
fn clip_overlapping_the_right_edge_is_trimmed_at_its_start() {
    let actions = compute_overwrite(&[clip("c", 20, 50)], 0, 30);
    match &actions[0] {
        OverwriteAction::TrimStart {
            new_start_frame,
            new_trim_start,
            new_duration,
            ..
        } => {
            assert_eq!(*new_start_frame, 30);
            assert_eq!(*new_duration, 40);
            assert_eq!(*new_trim_start, 10, "source advances by the trimmed span");
        }
        other => panic!("expected TrimStart, got {other:?}"),
    }
}

#[test]
fn region_strictly_inside_a_clip_splits_it() {
    let actions = compute_overwrite(&[clip("c", 0, 100)], 30, 60);
    match &actions[0] {
        OverwriteAction::Split {
            left_duration,
            right_start_frame,
            right_duration,
            right_trim_start,
            ..
        } => {
            assert_eq!(*left_duration, 30);
            assert_eq!(*right_start_frame, 60);
            assert_eq!(*right_duration, 40);
            assert_eq!(*right_trim_start, 60);
        }
        other => panic!("expected Split, got {other:?}"),
    }
}

#[test]
fn disjoint_clips_are_untouched_and_empty_regions_do_nothing() {
    assert!(compute_overwrite(&[clip("c", 0, 10)], 50, 60).is_empty());
    assert!(compute_overwrite(&[clip("c", 0, 10)], 5, 5).is_empty());
    assert!(compute_overwrite(&[clip("c", 0, 10)], 10, 5).is_empty());
}

/// Source continuity under non-unit speed: this is FR-011's whole content.
#[test]
fn trim_advances_source_scaled_by_speed() {
    assert_eq!(advance_trim(0, 30, 1.0), 30);
    assert_eq!(
        advance_trim(0, 30, 2.0),
        60,
        "double speed consumes twice the source"
    );
    assert_eq!(advance_trim(100, 10, 0.5), 105);
    assert_eq!(advance_trim(0, 3, 0.5), 2, "rounds, not truncates");
}

#[test]
fn advance_trim_survives_a_poisoned_speed() {
    assert_eq!(advance_trim(50, 10, f64::NAN), 50);
    assert_eq!(advance_trim(50, 10, f64::INFINITY), 50);
}
