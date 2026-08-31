//! The spec's 13 edge cases. Each asserts the specified outcome, not merely survival.

use palmier_core::ClipType;
use palmier_core::edit::{ClipMove, EditCommand, RefusalReason, SplitPoint, TrimEdge};

mod fixture;
use fixture::*;

#[test]
fn unknown_ids_refuse_rather_than_skip() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    for command in [
        EditCommand::RemoveClips {
            clip_ids: vec!["ghost".into()],
            ripple: false,
        },
        EditCommand::TrimClip {
            clip_id: "ghost".into(),
            edge: TrimEdge::Left,
            delta_frames: 1,
        },
    ] {
        assert!(matches!(
            s.apply(command).unwrap_err(),
            RefusalReason::UnknownClip(_)
        ));
    }
    assert!(matches!(
        s.apply(EditCommand::RemoveTrack {
            track_id: "ghost".into()
        })
        .unwrap_err(),
        RefusalReason::UnknownTrack(_)
    ));
}

#[test]
fn an_empty_target_set_is_refused() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    for command in [
        EditCommand::RemoveClips {
            clip_ids: vec![],
            ripple: false,
        },
        EditCommand::MoveClips { moves: vec![] },
        EditCommand::SplitClips { points: vec![] },
        EditCommand::RippleDeleteRanges { ranges: vec![] },
    ] {
        assert_eq!(s.apply(command).unwrap_err(), RefusalReason::EmptyTargets);
    }
}

#[test]
fn placement_at_frame_zero_and_far_past_the_end() {
    let mut s = session(&[track("v1", "video", &[])]);
    s.apply(EditCommand::AddClips {
        track_id: "v1".into(),
        clips: vec![new_clip("z", 0, 1)],
    })
    .unwrap();
    s.apply(EditCommand::AddClips {
        track_id: "v1".into(),
        clips: vec![new_clip("far", 1_000_000, 30)],
    })
    .unwrap();
    assert_eq!(starts(&clips_of(&s, "v1")), vec![0, 1_000_000]);
}

#[test]
fn a_split_outside_any_clip_is_reported_as_skipped_not_refused() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    let receipt = s
        .apply(EditCommand::SplitClips {
            points: vec![SplitPoint {
                track_id: "v1".into(),
                at_frame: 500,
            }],
        })
        .unwrap();
    assert!(receipt.is_no_op());
    assert_eq!(
        receipt.skipped.len(),
        1,
        "the caller is told why nothing happened"
    );
    assert!(receipt.skipped[0].1.contains("500"));
}

#[test]
fn splitting_a_zero_duration_clip_does_nothing() {
    let mut s = session(&[track("v1", "video", &[("a", 10, 0)])]);
    let receipt = s
        .apply(EditCommand::SplitClips {
            points: vec![SplitPoint {
                track_id: "v1".into(),
                at_frame: 10,
            }],
        })
        .unwrap();
    assert!(receipt.is_no_op());
}

#[test]
fn removing_every_clip_leaves_an_empty_track_that_can_then_be_removed() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30), ("b", 30, 30)])]);
    s.apply(EditCommand::RemoveClips {
        clip_ids: vec!["a".into(), "b".into()],
        ripple: false,
    })
    .unwrap();
    assert!(clips_of(&s, "v1").is_empty());
    s.apply(EditCommand::RemoveTrack {
        track_id: "v1".into(),
    })
    .unwrap();
    assert!(s.project.timelines[0].tracks.is_empty());
}

#[test]
fn a_ripple_covering_the_whole_timeline_empties_it() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30), ("b", 30, 30)])]);
    s.apply(EditCommand::RippleDeleteRanges {
        ranges: vec![(0, 60)],
    })
    .unwrap();
    assert!(clips_of(&s, "v1").is_empty());
}

#[test]
fn overlapping_ranges_in_one_ripple_merge_before_anything_moves() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 20), ("keep", 100, 20)])]);
    // [0,30) and [20,50) merge into [0,50): `keep` moves back by 50, not by 60.
    s.apply(EditCommand::RippleDeleteRanges {
        ranges: vec![(0, 30), (20, 50)],
    })
    .unwrap();
    assert_eq!(clip_by_id(&s, "keep").start_frame, 50);
}

#[test]
fn a_link_partner_on_another_track_is_removed_with_its_lead() {
    let mut s = linked_session();
    s.apply(EditCommand::RemoveClips {
        clip_ids: vec!["a".into()],
        ripple: true,
    })
    .unwrap();
    assert!(clips_of(&s, "v1").is_empty(), "the video lead went too");
}

#[test]
fn a_hidden_or_muted_track_still_participates_in_link_groups() {
    let mut s = linked_session();
    s.apply(EditCommand::SetTrackProperties {
        track_id: "a1".into(),
        properties: palmier_core::edit::TrackProperties {
            muted: Some(true),
            hidden: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    s.apply(EditCommand::MoveClips {
        moves: vec![ClipMove {
            clip_id: "v".into(),
            to_track_id: "v1".into(),
            to_frame: 25,
        }],
    })
    .unwrap();
    assert_eq!(
        clip_by_id(&s, "a").start_frame,
        25,
        "muting is not unlinking"
    );
}

#[test]
fn frame_overflow_is_refused_not_wrapped() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    let err = s
        .apply(EditCommand::MoveClips {
            moves: vec![ClipMove {
                clip_id: "a".into(),
                to_track_id: "v1".into(),
                to_frame: i64::MAX,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, RefusalReason::FrameOverflow(_)), "{err}");
}

#[test]
fn a_clip_can_move_between_tracks_of_a_compatible_type() {
    let mut s = session(&[
        track("v1", "video", &[("a", 0, 30)]),
        track("v2", "video", &[]),
    ]);
    s.apply(EditCommand::MoveClips {
        moves: vec![ClipMove {
            clip_id: "a".into(),
            to_track_id: "v2".into(),
            to_frame: 0,
        }],
    })
    .unwrap();
    assert!(clips_of(&s, "v1").is_empty());
    assert_eq!(clips_of(&s, "v2").len(), 1);
}

#[test]
fn adding_a_track_at_an_index_beyond_the_end_appends() {
    let mut s = session(&[track("v1", "video", &[])]);
    s.apply(EditCommand::AddTrack {
        track_type: ClipType::Audio,
        at_index: Some(99),
    })
    .unwrap();
    assert_eq!(s.project.timelines[0].tracks.len(), 2);
    assert_eq!(s.project.timelines[0].tracks[1].track_type, ClipType::Audio);
}

/// Extreme frame operands on every command that takes one.
///
/// This was a real defect: `trim_clip` added `delta_frames` to `start_frame` unchecked,
/// so `i64::MIN` panicked in a debug build. The property test found it, but only on the
/// platform whose random seed happened to try that value — Linux passed and Windows did
/// not. Boundaries this important are asserted explicitly, not left to chance.
#[test]
fn extreme_frame_operands_refuse_instead_of_panicking() {
    let extremes = [i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX - 1, i64::MAX];

    for value in extremes {
        let mut s = session(&[
            track("v1", "video", &[("a", 10, 30)]),
            track("v2", "video", &[]),
        ]);

        // Every command that accepts a frame, at every extreme.
        let commands = vec![
            EditCommand::TrimClip {
                clip_id: "a".into(),
                edge: TrimEdge::Left,
                delta_frames: value,
            },
            EditCommand::TrimClip {
                clip_id: "a".into(),
                edge: TrimEdge::Right,
                delta_frames: value,
            },
            EditCommand::MoveClips {
                moves: vec![ClipMove {
                    clip_id: "a".into(),
                    to_track_id: "v2".into(),
                    to_frame: value,
                }],
            },
            EditCommand::SplitClips {
                points: vec![SplitPoint {
                    track_id: "v1".into(),
                    at_frame: value,
                }],
            },
            EditCommand::RippleDeleteRanges {
                ranges: vec![(value, value)],
            },
            EditCommand::RippleDeleteRanges {
                ranges: vec![(0, value)],
            },
            EditCommand::AddClips {
                track_id: "v1".into(),
                clips: vec![new_clip("x", value, 30)],
            },
            EditCommand::InsertClips {
                track_id: "v1".into(),
                at_frame: value,
                clips: vec![new_clip("y", 0, 30)],
            },
        ];

        for command in commands {
            // Snapshot per command: earlier ones in this list legitimately apply.
            let before = s.project.clone();
            // The contract is only that it must not panic; refusing is a fine outcome.
            match s.apply(command.clone()) {
                Err(_) => assert_eq!(
                    s.project, before,
                    "{command:?} at {value} refused but mutated the project"
                ),
                Ok(receipt) if receipt.is_no_op() => {}
                Ok(_) => {
                    // It applied, so the timeline must still be arithmetically sound.
                    for t in &s.project.timelines {
                        let _ = t.total_frames();
                    }
                }
            }
        }
    }
}

/// A linked partner at an extreme offset must not overflow either — the partner move is
/// computed from a delta, which is its own subtraction.
#[test]
fn a_linked_partner_at_an_extreme_offset_does_not_panic() {
    let mut s = linked_session();
    for frame in [i64::MIN, i64::MAX, i64::MAX - 100] {
        let before = s.project.clone();
        let result = s.apply(EditCommand::MoveClips {
            moves: vec![ClipMove {
                clip_id: "v".into(),
                to_track_id: "v1".into(),
                to_frame: frame,
            }],
        });
        if result.is_err() {
            assert_eq!(s.project, before);
        }
    }
}
