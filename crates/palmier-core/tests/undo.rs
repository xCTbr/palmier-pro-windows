//! Constitution principle II: undo is the command journal.

use palmier_core::ClipType;
use palmier_core::edit::{
    ClipMove, ClipProperties, EditCommand, EditSession, SplitPoint, TrackProperties, TrimEdge,
};
use proptest::prelude::*;

mod fixture;
use fixture::*;

fn scenario() -> EditSession {
    session(&[
        track("v1", "video", &[("a", 0, 30), ("b", 30, 30), ("c", 60, 30)]),
        track("v2", "video", &[("d", 100, 40)]),
        track("a1", "audio", &[("e", 0, 60)]),
    ])
}

#[test]
fn three_commands_undone_three_times_restores_the_original() {
    let mut s = scenario();
    let original = s.project.clone();

    s.apply(EditCommand::MoveClips {
        moves: vec![ClipMove {
            clip_id: "a".into(),
            to_track_id: "v2".into(),
            to_frame: 300,
        }],
    })
    .unwrap();
    s.apply(EditCommand::SplitClips {
        points: vec![SplitPoint {
            track_id: "v1".into(),
            at_frame: 45,
        }],
    })
    .unwrap();
    s.apply(EditCommand::RippleDeleteRanges {
        ranges: vec![(0, 20)],
    })
    .unwrap();
    assert_ne!(s.project, original);

    for _ in 0..3 {
        s.undo().unwrap();
    }
    assert_eq!(
        s.project, original,
        "undo did not restore the project exactly"
    );
    assert!(!s.journal.can_undo());
}

#[test]
fn a_refusal_leaves_nothing_to_step_past() {
    let mut s = scenario();
    s.apply(EditCommand::MoveClips {
        moves: vec![ClipMove {
            clip_id: "a".into(),
            to_track_id: "v1".into(),
            to_frame: 500,
        }],
    })
    .unwrap();
    let after_move = s.project.clone();

    assert!(
        s.apply(EditCommand::MoveClips {
            moves: vec![ClipMove {
                clip_id: "ghost".into(),
                to_track_id: "v1".into(),
                to_frame: 0
            }],
        })
        .is_err()
    );
    assert_eq!(s.journal.len(), 1, "the refusal added no entry");

    s.undo().unwrap();
    assert_ne!(
        s.project, after_move,
        "undo stepped past the refusal to the real change"
    );
}

#[test]
fn a_no_op_creates_no_entry() {
    let mut s = scenario();
    let receipt = s
        .apply(EditCommand::MoveClips {
            moves: vec![ClipMove {
                clip_id: "a".into(),
                to_track_id: "v1".into(),
                to_frame: 0,
            }],
        })
        .unwrap();
    assert!(receipt.is_no_op());
    assert_eq!(s.journal.len(), 0);
    assert!(!s.journal.can_undo());
}

#[test]
fn redo_reapplies_identically() {
    let mut s = scenario();
    s.apply(EditCommand::RippleDeleteRanges {
        ranges: vec![(30, 60)],
    })
    .unwrap();
    let after = s.project.clone();

    s.undo().unwrap();
    assert_ne!(s.project, after);

    s.redo().unwrap();
    assert_eq!(
        s.project, after,
        "redo did not reproduce the original effect"
    );
}

#[test]
fn a_new_command_discards_the_redo_branch() {
    let mut s = scenario();
    s.apply(EditCommand::RemoveClips {
        clip_ids: vec!["b".into()],
        ripple: false,
    })
    .unwrap();
    s.undo().unwrap();
    assert!(s.journal.can_redo());

    s.apply(EditCommand::SetClipProperties {
        clip_ids: vec!["a".into()],
        properties: ClipProperties {
            opacity: Some(0.25),
            ..Default::default()
        },
    })
    .unwrap();
    assert!(!s.journal.can_redo(), "the redo branch must be gone");
    assert_eq!(s.journal.len(), 1);
}

#[test]
fn undo_and_redo_on_an_empty_journal_do_nothing() {
    let mut s = scenario();
    let original = s.project.clone();
    assert!(s.undo().is_none());
    assert!(s.redo().is_none());
    assert_eq!(s.project, original);
}

#[test]
fn undoing_a_multi_track_ripple_restores_every_clip_and_marker() {
    let json = r#"{"timelines":[{"id":"tl","fps":30,"width":1920,"height":1080,
      "tracks":[
        {"id":"v1","type":"video","clips":[
          {"id":"a","mediaRef":"m","startFrame":0,"durationFrames":50},
          {"id":"b","mediaRef":"m","startFrame":50,"durationFrames":50}]},
        {"id":"v2","type":"video","clips":[
          {"id":"c","mediaRef":"m","startFrame":200,"durationFrames":50}]}],
      "markers":[
        {"id":"m1","name":"n","startFrame":150,"durationFrames":0,
         "color":{"r":1,"g":1,"b":1,"a":1},"comment":""},
        {"id":"m2","name":"n","startFrame":10,"durationFrames":100,
         "color":{"r":1,"g":1,"b":1,"a":1},"comment":""}]}],
      "activeTimelineId":"tl"}"#;
    let mut s = EditSession::new(palmier_core::ProjectFile::decode(json.as_bytes()).unwrap());
    let original = s.project.clone();

    s.apply(EditCommand::RippleDeleteRanges {
        ranges: vec![(20, 70)],
    })
    .unwrap();
    assert_ne!(s.project, original);

    s.undo().unwrap();
    assert_eq!(
        s.project, original,
        "clips and markers must both come back exactly"
    );
}

// ---------------------------------------------------------------- SC-002

/// The guard that matters more than any hand-written undo test: if a command mutates
/// something its inverse patch does not record, this finds it.
fn arbitrary_command() -> impl Strategy<Value = EditCommand> {
    prop_oneof![
        (0i64..200, 1i64..80).prop_map(|(start, duration)| EditCommand::AddClips {
            track_id: "v1".into(),
            clips: vec![new_clip("gen", start, duration)],
        }),
        (
            prop::sample::select(vec!["a", "b", "c", "d", "e"]),
            0i64..300
        )
            .prop_map(|(id, frame)| {
                EditCommand::MoveClips {
                    moves: vec![ClipMove {
                        clip_id: id.into(),
                        to_track_id: if id == "e" { "a1".into() } else { "v1".into() },
                        to_frame: frame,
                    }],
                }
            }),
        (
            prop::sample::select(vec!["a", "b", "c", "d", "e"]),
            any::<bool>()
        )
            .prop_map(|(id, ripple)| EditCommand::RemoveClips {
                clip_ids: vec![id.into()],
                ripple,
            }),
        (0i64..150).prop_map(|frame| EditCommand::SplitClips {
            points: vec![SplitPoint {
                track_id: "v1".into(),
                at_frame: frame
            }],
        }),
        (0i64..100, 0i64..100).prop_map(|(a, b)| EditCommand::RippleDeleteRanges {
            ranges: vec![(a.min(b), a.max(b))],
        }),
        (
            prop::sample::select(vec!["a", "b", "c"]),
            -40i64..40,
            any::<bool>()
        )
            .prop_map(|(id, delta, left)| EditCommand::TrimClip {
                clip_id: id.into(),
                edge: if left {
                    TrimEdge::Left
                } else {
                    TrimEdge::Right
                },
                delta_frames: delta,
            }),
        (prop::sample::select(vec!["a", "b", "c"]), 0.0f64..1.0).prop_map(|(id, opacity)| {
            EditCommand::SetClipProperties {
                clip_ids: vec![id.into()],
                properties: ClipProperties {
                    opacity: Some(opacity),
                    ..Default::default()
                },
            }
        }),
        Just(EditCommand::AddTrack {
            track_type: ClipType::Video,
            at_index: None
        }),
        prop::sample::select(vec!["v1", "v2", "a1"]).prop_map(|id| {
            EditCommand::SetTrackProperties {
                track_id: id.into(),
                properties: TrackProperties {
                    sync_locked: Some(false),
                    ..Default::default()
                },
            }
        }),
        Just(EditCommand::LinkClips {
            clip_ids: vec!["a".into(), "e".into()]
        }),
        Just(EditCommand::UnlinkClips {
            clip_ids: vec!["a".into()]
        }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// SC-002: apply any sequence, undo it all, land exactly where you started.
    #[test]
    fn apply_then_undo_all_is_the_identity(
        commands in prop::collection::vec(arbitrary_command(), 1..12)
    ) {
        let mut s = scenario();
        let original = s.project.clone();

        let mut applied = 0usize;
        for command in commands {
            if let Ok(receipt) = s.apply(command) && !receipt.is_no_op() {
                applied += 1;
            }
        }
        prop_assert_eq!(s.journal.len(), applied);

        while s.journal.can_undo() {
            s.undo();
        }
        prop_assert_eq!(s.project, original);
    }

    /// SC-003 and SC-004 together: a refusal never writes, and nothing ever panics.
    #[test]
    fn a_refused_command_never_changes_the_project(command in arbitrary_command()) {
        let mut s = scenario();
        let before = s.project.clone();
        if s.apply(command).is_err() {
            prop_assert_eq!(s.project, before);
            prop_assert_eq!(s.journal.len(), 0);
        }
    }

    /// Hostile targets must refuse, never panic.
    #[test]
    fn hostile_ids_and_boundary_frames_never_panic(
        id in ".{0,40}",
        frame in prop_oneof![Just(i64::MIN), Just(i64::MAX), Just(0), -1000i64..1000],
    ) {
        let mut s = scenario();
        let _ = s.apply(EditCommand::MoveClips {
            moves: vec![ClipMove { clip_id: id.clone(), to_track_id: "v1".into(), to_frame: frame }],
        });
        let _ = s.apply(EditCommand::TrimClip {
            clip_id: id.clone(), edge: TrimEdge::Left, delta_frames: frame,
        });
        let _ = s.apply(EditCommand::RippleDeleteRanges { ranges: vec![(frame, frame)] });
        let _ = s.apply(EditCommand::RemoveClips { clip_ids: vec![id], ripple: true });
    }
}
