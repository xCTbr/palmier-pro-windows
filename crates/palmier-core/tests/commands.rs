//! Every command: success, refusal, and no-op (SC-001), plus the three divergences
//! from the reference implementation recorded in research.md.

use palmier_core::edit::{
    ClipMove, ClipProperties, EditCommand, EditSession, RefusalReason, SplitPoint, TrackProperties,
    TrimEdge,
};
use palmier_core::{ClipType, ProjectFile};

mod fixture;
use fixture::*;

// ------------------------------------------------------------------- add

#[test]
fn add_places_a_clip_and_journals_one_entry() {
    let mut s = session(&[track("v1", "video", &[])]);
    let receipt = s
        .apply(EditCommand::AddClips {
            track_id: "v1".into(),
            clips: vec![new_clip("c1", 100, 50)],
        })
        .unwrap();
    assert_eq!(receipt.created_clip_ids, vec!["c1"]);
    assert_eq!(s.journal.len(), 1);
    let c = clip_at(&s, "v1", 0);
    assert_eq!((c.start_frame, c.duration_frames), (100, 50));
}

#[test]
fn add_overwrites_what_it_lands_on() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 100)])]);
    s.apply(EditCommand::AddClips {
        track_id: "v1".into(),
        clips: vec![new_clip("b", 30, 30)],
    })
    .unwrap();
    // `a` is split around the new clip: [0,30) and [60,100).
    let clips = clips_of(&s, "v1");
    assert_eq!(clips.len(), 3, "{clips:#?}");
    assert_eq!((clips[0].start_frame, clips[0].duration_frames), (0, 30));
    assert_eq!((clips[1].start_frame, clips[1].duration_frames), (30, 30));
    assert_eq!((clips[2].start_frame, clips[2].duration_frames), (60, 40));
}

#[test]
fn add_refuses_an_unknown_track() {
    let mut s = session(&[track("v1", "video", &[])]);
    let err = s
        .apply(EditCommand::AddClips {
            track_id: "nope".into(),
            clips: vec![new_clip("c", 0, 10)],
        })
        .unwrap_err();
    assert_eq!(err, RefusalReason::UnknownTrack("nope".into()));
    assert_eq!(s.journal.len(), 0, "a refusal journals nothing");
}

#[test]
fn add_refuses_an_audio_clip_on_a_video_track() {
    let mut s = session(&[track("v1", "video", &[])]);
    let mut clip = new_clip("c", 0, 10);
    clip.media_type = ClipType::Audio;
    let err = s
        .apply(EditCommand::AddClips {
            track_id: "v1".into(),
            clips: vec![clip],
        })
        .unwrap_err();
    assert!(
        matches!(err, RefusalReason::IncompatibleTrackType { .. }),
        "{err}"
    );
}

#[test]
fn add_refuses_a_negative_frame() {
    let mut s = session(&[track("v1", "video", &[])]);
    let err = s
        .apply(EditCommand::AddClips {
            track_id: "v1".into(),
            clips: vec![new_clip("c", -1, 10)],
        })
        .unwrap_err();
    assert!(matches!(err, RefusalReason::NegativeFrame(_)), "{err}");
}

#[test]
fn add_with_no_clips_is_refused_as_empty() {
    let mut s = session(&[track("v1", "video", &[])]);
    assert_eq!(
        s.apply(EditCommand::AddClips {
            track_id: "v1".into(),
            clips: vec![]
        })
        .unwrap_err(),
        RefusalReason::EmptyTargets
    );
}

// ---------------------------------------------------------------- insert

#[test]
fn insert_opens_a_gap_and_pushes_what_follows() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30), ("b", 30, 30)])]);
    s.apply(EditCommand::InsertClips {
        track_id: "v1".into(),
        at_frame: 30,
        clips: vec![new_clip("new", 0, 20)],
    })
    .unwrap();
    let clips = clips_of(&s, "v1");
    assert_eq!(clips.len(), 3);
    assert_eq!(starts(&clips), vec![0, 30, 50], "b pushed from 30 to 50");
}

// ------------------------------------------------------------------ move

#[test]
fn move_relocates_a_clip() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    s.apply(EditCommand::MoveClips {
        moves: vec![ClipMove {
            clip_id: "a".into(),
            to_track_id: "v1".into(),
            to_frame: 90,
        }],
    })
    .unwrap();
    assert_eq!(clip_at(&s, "v1", 0).start_frame, 90);
}

#[test]
fn move_to_the_same_place_is_a_no_op_and_journals_nothing() {
    let mut s = session(&[track("v1", "video", &[("a", 40, 30)])]);
    let receipt = s
        .apply(EditCommand::MoveClips {
            moves: vec![ClipMove {
                clip_id: "a".into(),
                to_track_id: "v1".into(),
                to_frame: 40,
            }],
        })
        .unwrap();
    assert!(receipt.is_no_op());
    assert_eq!(s.journal.len(), 0);
}

/// D1: the original silently skips an unknown target and applies the rest. We refuse.
#[test]
fn move_refuses_the_whole_command_when_one_target_is_unknown() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    let before = s.project.clone();
    let err = s
        .apply(EditCommand::MoveClips {
            moves: vec![
                ClipMove {
                    clip_id: "a".into(),
                    to_track_id: "v1".into(),
                    to_frame: 90,
                },
                ClipMove {
                    clip_id: "ghost".into(),
                    to_track_id: "v1".into(),
                    to_frame: 10,
                },
            ],
        })
        .unwrap_err();
    assert_eq!(err, RefusalReason::UnknownClip("ghost".into()));
    assert_eq!(
        s.project, before,
        "the valid move must not have been applied"
    );
}

/// D2: the original clamps a negative destination to 0. We refuse.
#[test]
fn move_refuses_a_negative_destination_instead_of_clamping() {
    let mut s = session(&[track("v1", "video", &[("a", 50, 30)])]);
    let before = s.project.clone();
    let err = s
        .apply(EditCommand::MoveClips {
            moves: vec![ClipMove {
                clip_id: "a".into(),
                to_track_id: "v1".into(),
                to_frame: -10,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, RefusalReason::NegativeFrame(_)), "{err}");
    assert_eq!(s.project, before);
}

// ---------------------------------------------------------------- linked

#[test]
fn moving_a_linked_clip_moves_its_partner_by_the_same_delta() {
    let mut s = linked_session();
    s.apply(EditCommand::MoveClips {
        moves: vec![ClipMove {
            clip_id: "v".into(),
            to_track_id: "v1".into(),
            to_frame: 40,
        }],
    })
    .unwrap();
    assert_eq!(clip_by_id(&s, "v").start_frame, 40);
    assert_eq!(clip_by_id(&s, "a").start_frame, 40, "partner followed");
}

#[test]
fn removing_a_linked_clip_removes_its_partner() {
    let mut s = linked_session();
    s.apply(EditCommand::RemoveClips {
        clip_ids: vec!["v".into()],
        ripple: false,
    })
    .unwrap();
    assert_eq!(clips_of(&s, "v1").len(), 0);
    assert_eq!(
        clips_of(&s, "a1").len(),
        0,
        "partner removed in the same entry"
    );
}

#[test]
fn unlinking_stops_the_partner_from_following() {
    let mut s = linked_session();
    s.apply(EditCommand::UnlinkClips {
        clip_ids: vec!["v".into()],
    })
    .unwrap();
    s.apply(EditCommand::MoveClips {
        moves: vec![ClipMove {
            clip_id: "v".into(),
            to_track_id: "v1".into(),
            to_frame: 40,
        }],
    })
    .unwrap();
    assert_eq!(clip_by_id(&s, "a").start_frame, 0, "no longer linked");
}

/// D2 again, and the reason it matters most: a clamped partner desyncs silently.
#[test]
fn move_refuses_when_a_linked_partner_would_go_negative() {
    let mut s = linked_session();
    // Put the partner earlier than the lead so the same delta drives it below zero.
    s.apply(EditCommand::MoveClips {
        moves: vec![ClipMove {
            clip_id: "v".into(),
            to_track_id: "v1".into(),
            to_frame: 100,
        }],
    })
    .unwrap();
    s.apply(EditCommand::UnlinkClips {
        clip_ids: vec!["v".into()],
    })
    .unwrap();
    s.apply(EditCommand::MoveClips {
        moves: vec![ClipMove {
            clip_id: "a".into(),
            to_track_id: "a1".into(),
            to_frame: 10,
        }],
    })
    .unwrap();
    s.apply(EditCommand::LinkClips {
        clip_ids: vec!["v".into(), "a".into()],
    })
    .unwrap();

    let before = s.project.clone();
    let err = s
        .apply(EditCommand::MoveClips {
            moves: vec![ClipMove {
                clip_id: "v".into(),
                to_track_id: "v1".into(),
                to_frame: 0,
            }],
        })
        .unwrap_err();
    assert!(matches!(err, RefusalReason::NegativeFrame(_)), "{err}");
    assert_eq!(
        s.project, before,
        "nothing moved, so the link stayed intact"
    );
}

#[test]
fn linking_needs_at_least_two_clips() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    assert!(matches!(
        s.apply(EditCommand::LinkClips {
            clip_ids: vec!["a".into()]
        })
        .unwrap_err(),
        RefusalReason::Invalid(_)
    ));
}

// ----------------------------------------------------------------- split

#[test]
fn split_divides_a_clip_preserving_source_continuity() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 100)])]);
    s.apply(EditCommand::SplitClips {
        points: vec![SplitPoint {
            track_id: "v1".into(),
            at_frame: 40,
        }],
    })
    .unwrap();
    let clips = clips_of(&s, "v1");
    assert_eq!(clips.len(), 2);
    assert_eq!((clips[0].start_frame, clips[0].duration_frames), (0, 40));
    assert_eq!((clips[1].start_frame, clips[1].duration_frames), (40, 60));
    assert_eq!(
        clips[1].trim_start_frame, 40,
        "right half starts 40 source frames in"
    );
}

#[test]
fn split_at_a_boundary_is_a_no_op() {
    for frame in [0, 100] {
        let mut s = session(&[track("v1", "video", &[("a", 0, 100)])]);
        let receipt = s
            .apply(EditCommand::SplitClips {
                points: vec![SplitPoint {
                    track_id: "v1".into(),
                    at_frame: frame,
                }],
            })
            .unwrap();
        assert!(receipt.is_no_op(), "split at {frame} must do nothing");
        assert_eq!(s.journal.len(), 0);
        assert_eq!(clips_of(&s, "v1").len(), 1);
    }
}

#[test]
fn split_propagates_to_linked_partners_and_regroups_the_right_halves() {
    let mut s = linked_session();
    s.apply(EditCommand::SplitClips {
        points: vec![SplitPoint {
            track_id: "v1".into(),
            at_frame: 50,
        }],
    })
    .unwrap();
    assert_eq!(clips_of(&s, "v1").len(), 2);
    assert_eq!(clips_of(&s, "a1").len(), 2, "the audio partner split too");

    let right_video = &clips_of(&s, "v1")[1];
    let right_audio = &clips_of(&s, "a1")[1];
    assert!(right_video.link_group_id.is_some());
    assert_eq!(
        right_video.link_group_id, right_audio.link_group_id,
        "right halves share a new group"
    );
    assert_ne!(
        right_video.link_group_id,
        clips_of(&s, "v1")[0].link_group_id
    );
}

// ---------------------------------------------------------- ripple delete

#[test]
fn ripple_delete_closes_the_gap() {
    let mut s = session(&[track(
        "v1",
        "video",
        &[("a", 0, 30), ("b", 30, 30), ("c", 60, 30)],
    )]);
    s.apply(EditCommand::RippleDeleteRanges {
        ranges: vec![(30, 60)],
    })
    .unwrap();
    let clips = clips_of(&s, "v1");
    assert_eq!(clips.len(), 2);
    assert_eq!(starts(&clips), vec![0, 30], "c pulled back into b's place");
}

#[test]
fn ripple_delete_shifts_a_sync_locked_track_with_nothing_in_the_range() {
    let mut s = session(&[
        track("v1", "video", &[("a", 0, 100)]),
        track("v2", "video", &[("b", 200, 30)]),
    ]);
    s.apply(EditCommand::RippleDeleteRanges {
        ranges: vec![(0, 50)],
    })
    .unwrap();
    assert_eq!(
        clip_by_id(&s, "b").start_frame,
        150,
        "sync-locked track kept alignment"
    );
}

#[test]
fn ripple_delete_leaves_a_non_sync_locked_track_alone() {
    let mut s = session(&[
        track("v1", "video", &[("a", 0, 100)]),
        track("v2", "video", &[("b", 200, 30)]),
    ]);
    s.apply(EditCommand::SetTrackProperties {
        track_id: "v2".into(),
        properties: TrackProperties {
            sync_locked: Some(false),
            ..Default::default()
        },
    })
    .unwrap();
    s.apply(EditCommand::RippleDeleteRanges {
        ranges: vec![(0, 50)],
    })
    .unwrap();
    assert_eq!(
        clip_by_id(&s, "b").start_frame,
        200,
        "unlocked track did not shift"
    );
}

#[test]
fn ripple_delete_of_an_empty_range_is_a_no_op() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    let receipt = s
        .apply(EditCommand::RippleDeleteRanges {
            ranges: vec![(10, 10)],
        })
        .unwrap();
    assert!(receipt.is_no_op());
    assert_eq!(s.journal.len(), 0);
}

#[test]
fn ripple_delete_refuses_an_inverted_range() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    assert!(matches!(
        s.apply(EditCommand::RippleDeleteRanges {
            ranges: vec![(50, 10)]
        })
        .unwrap_err(),
        RefusalReason::Invalid(_)
    ));
}

// ------------------------------------------------------------------ trim

#[test]
fn trim_left_advances_the_source_and_shortens_the_clip() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 100)])]);
    s.apply(EditCommand::TrimClip {
        clip_id: "a".into(),
        edge: TrimEdge::Left,
        delta_frames: 20,
    })
    .unwrap();
    let c = clip_by_id(&s, "a");
    assert_eq!(
        (c.start_frame, c.duration_frames, c.trim_start_frame),
        (20, 80, 20)
    );
}

#[test]
fn trim_right_changes_only_the_duration() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 100)])]);
    s.apply(EditCommand::TrimClip {
        clip_id: "a".into(),
        edge: TrimEdge::Right,
        delta_frames: -30,
    })
    .unwrap();
    let c = clip_by_id(&s, "a");
    assert_eq!(
        (c.start_frame, c.duration_frames, c.trim_start_frame),
        (0, 70, 0)
    );
}

#[test]
fn trim_to_nothing_is_refused() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 100)])]);
    assert!(matches!(
        s.apply(EditCommand::TrimClip {
            clip_id: "a".into(),
            edge: TrimEdge::Right,
            delta_frames: -100
        })
        .unwrap_err(),
        RefusalReason::Invalid(_)
    ));
}

#[test]
fn trim_by_zero_is_a_no_op() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 100)])]);
    let receipt = s
        .apply(EditCommand::TrimClip {
            clip_id: "a".into(),
            edge: TrimEdge::Left,
            delta_frames: 0,
        })
        .unwrap();
    assert!(receipt.is_no_op());
}

// ------------------------------------------------------------ properties

#[test]
fn set_clip_properties_changes_only_what_is_named() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    s.apply(EditCommand::SetClipProperties {
        clip_ids: vec!["a".into()],
        properties: ClipProperties {
            opacity: Some(0.5),
            ..Default::default()
        },
    })
    .unwrap();
    let c = clip_by_id(&s, "a");
    assert_eq!(c.opacity, 0.5);
    assert_eq!(c.volume, 1.0, "untouched");
}

#[test]
fn setting_a_property_to_its_current_value_is_a_no_op() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    let receipt = s
        .apply(EditCommand::SetClipProperties {
            clip_ids: vec!["a".into()],
            properties: ClipProperties {
                opacity: Some(1.0),
                ..Default::default()
            },
        })
        .unwrap();
    assert!(receipt.is_no_op());
    assert_eq!(s.journal.len(), 0);
}

#[test]
fn a_non_finite_or_non_positive_speed_is_refused() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        assert!(
            s.apply(EditCommand::SetClipProperties {
                clip_ids: vec!["a".into()],
                properties: ClipProperties {
                    speed: Some(bad),
                    ..Default::default()
                },
            })
            .is_err(),
            "speed {bad} must be refused"
        );
    }
}

#[test]
fn a_nested_sequence_clip_cannot_be_retimed() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    {
        let clip = s.project.timelines[0].tracks[0].clips[0].clone();
        s.project.timelines[0].tracks[0].clips[0].source_clip_type = ClipType::Sequence;
        let _ = clip;
    }
    assert!(matches!(
        s.apply(EditCommand::SetClipProperties {
            clip_ids: vec!["a".into()],
            properties: ClipProperties {
                speed: Some(2.0),
                ..Default::default()
            },
        })
        .unwrap_err(),
        RefusalReason::SequenceNotRetimable(_)
    ));
}

// ---------------------------------------------------------------- tracks

#[test]
fn add_and_remove_a_track() {
    let mut s = session(&[track("v1", "video", &[])]);
    let receipt = s
        .apply(EditCommand::AddTrack {
            track_type: ClipType::Audio,
            at_index: None,
        })
        .unwrap();
    let new_id = receipt.created_track_ids[0].clone();
    assert_eq!(s.project.timelines[0].tracks.len(), 2);

    s.apply(EditCommand::RemoveTrack { track_id: new_id })
        .unwrap();
    assert_eq!(s.project.timelines[0].tracks.len(), 1);
}

#[test]
fn a_track_holding_clips_cannot_be_removed() {
    let mut s = session(&[track("v1", "video", &[("a", 0, 30)])]);
    assert_eq!(
        s.apply(EditCommand::RemoveTrack {
            track_id: "v1".into()
        })
        .unwrap_err(),
        RefusalReason::TrackNotEmpty("v1".into())
    );
}

#[test]
fn set_track_properties_no_ops_when_nothing_differs() {
    let mut s = session(&[track("v1", "video", &[])]);
    let receipt = s
        .apply(EditCommand::SetTrackProperties {
            track_id: "v1".into(),
            properties: TrackProperties {
                muted: Some(false),
                ..Default::default()
            },
        })
        .unwrap();
    assert!(receipt.is_no_op());
}

#[test]
fn commands_never_mutate_the_project_on_refusal() {
    // SC-003 in one place, across every refusing command.
    let build = || linked_session();
    let refusals: Vec<EditCommand> = vec![
        EditCommand::AddClips {
            track_id: "ghost".into(),
            clips: vec![new_clip("x", 0, 10)],
        },
        EditCommand::MoveClips {
            moves: vec![ClipMove {
                clip_id: "ghost".into(),
                to_track_id: "v1".into(),
                to_frame: 0,
            }],
        },
        EditCommand::RemoveClips {
            clip_ids: vec!["ghost".into()],
            ripple: true,
        },
        EditCommand::SplitClips {
            points: vec![SplitPoint {
                track_id: "ghost".into(),
                at_frame: 5,
            }],
        },
        EditCommand::RippleDeleteRanges {
            ranges: vec![(50, 10)],
        },
        EditCommand::TrimClip {
            clip_id: "ghost".into(),
            edge: TrimEdge::Left,
            delta_frames: 5,
        },
        EditCommand::RemoveTrack {
            track_id: "v1".into(),
        },
        EditCommand::LinkClips {
            clip_ids: vec!["v".into()],
        },
    ];
    for command in refusals {
        let mut s = build();
        let before = s.project.clone();
        let result = s.apply(command.clone());
        assert!(result.is_err(), "{command:?} should have been refused");
        assert_eq!(
            s.project, before,
            "{command:?} mutated the project despite refusing"
        );
        assert_eq!(s.journal.len(), 0);
    }
}

#[test]
fn a_project_with_no_timelines_refuses_everything() {
    let mut s = EditSession::new(ProjectFile {
        timelines: vec![],
        active_timeline_id: None,
        open_timeline_ids: None,
        view_states: None,
        speakers: None,
        multicam_groups: None,
        extra: Default::default(),
    });
    assert_eq!(
        s.apply(EditCommand::AddTrack {
            track_type: ClipType::Video,
            at_index: None
        })
        .unwrap_err(),
        RefusalReason::NoActiveTimeline
    );
}
