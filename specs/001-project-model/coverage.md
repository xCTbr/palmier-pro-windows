# Coverage: spec 001

**Generated**: 2026-08-28 · Tasks T055 and T056.

## SC-001 — every persisted type is modeled and tested

| Type | Module | Covered by |
|---|---|---|
| `ProjectFile` | `project.rs` | `roundtrip_and_extra::legacy_bare_timeline_is_wrapped`, `edge_cases::zero_timelines_is_rejected` |
| `Timeline` | `timeline.rs` | `decoding_contract::timeline_requires_fps`, `timeline_name_defaults_to_timeline_one` |
| `Track` | `timeline.rs` | `decoding_contract::track_sync_locked_defaults_to_true`, `invalid_track_name_becomes_none_without_failing`, `display_height_clamps_out_of_range` |
| `Clip` | `timeline.rs` | `decoding_contract::clip_speed_is_lenient_*`, `edge_rounding_coerces_out_of_range_to_zero`, `one_malformed_clip_empties_its_entire_track` |
| `TimelineViewState` | `timeline.rs` | `roundtrip_and_extra::view_states_decode_by_timeline_id` |
| `ClipType`, `BlendMode` | `timeline.rs` | `decoding_contract::clip_media_type_is_lenient_on_unknown_variant`, `roundtrip_and_extra::full_fixture_decodes_with_expected_values` |
| `Transform` | `transform.rs` | `decoding_contract::transform_migrates_legacy_x_y`, `strict_field_destroys_its_whole_object_not_just_itself`, `roundtrip::transform_encodes_modern_keys_and_never_legacy` |
| `Crop` | `transform.rs` | `roundtrip_and_extra::full_fixture_decodes_with_expected_values` |
| `Keyframe`, `KeyframeTrack`, `AnimPair`, `Interpolation` | `keyframe.rs` | `edge_cases::keyframe_tracks_tolerate_empty_unordered_and_duplicate_frames` |
| `Effect`, `EffectParam` | `effect.rs` | `roundtrip_and_extra::full_fixture_decodes_with_expected_values`, `materialization_fills_every_absent_id` |
| `CurvePoint`, `GradeCurve`, `HueCurves` | `effect.rs` | **structure only** — see gaps below |
| `TextStyle`, `Outline`, `Background`, `Shadow`, `Rgba` | `text.rs` | `roundtrip_and_extra::full_fixture_decodes_with_expected_values` |
| `TextAnimation`, `WordTiming`, `TextFillMode` | `text.rs` | `roundtrip_and_extra::full_fixture_decodes_with_expected_values` |
| `Alignment`, `FontCase` | `text.rs` | `roundtrip::every_fixture_round_trips` |
| `TimelineMarker`, `MarkerStatus` | `marker.rs` | `decoding_contract::marker_requires_comment`, `roundtrip::required_keys_are_always_emitted` |
| `SpeakerRegistryEntry` | `marker.rs` | `roundtrip::every_fixture_round_trips` (absence path) |
| `MediaManifest`, `MediaManifestEntry`, `MediaFolder`, `MediaSource` | `media.rs` | **structure only** — see gaps below |
| `MulticamSource` | `media.rs` | preserved opaquely; `roundtrip::every_fixture_round_trips` |
| `FrameRange`, timecode | `frames.rs` | `frames.rs` — 9 tests |

## SC-003 — each guard has a test that fails without it

Verified by removing the guard, running the test, and restoring the source.

| Guard | Removed behavior | Test that caught it |
|---|---|---|
| `coerce_unit_interval` | out-of-range no longer becomes 0 | `edge_rounding_coerces_out_of_range_to_zero` ✓ |
| `clamp_range` | `displayHeight` no longer clamped | `display_height_clamps_out_of_range` ✓ |
| `syncLocked` default | flipped to `false` | `track_sync_locked_defaults_to_true` ✓ |
| `normalize_track_name` | invalid names kept | `invalid_track_name_becomes_none_without_failing` ✓ |
| zero-timeline rejection | empty project accepted | `zero_timelines_is_rejected` ✓ |
| `float_roundtrip` feature | float precision drift | `every_fixture_round_trips`, `round_trip_is_idempotent_after_the_first_pass` ✓ |

6 of 6 mutations were caught.

## SC-005 — performance

`bench_large_project`, release: **10,000 clips in 57 ms** against a 500 ms budget.
The map-buffering kernel stays; T054's streaming rewrite is not needed.

## Known gaps

Honest accounting of what is modeled but thinly tested.

- **`GradeCurve` / `HueCurves` / `CurvePoint`** — modeled with the correct required
  fields per the audit, and covered by round-trip only through their absence. No
  fixture exercises a real colour grade. They gain real coverage when `apply_color`
  arrives.
- **`MediaManifest` and its entries** — the manifest lives in a separate file that this
  feature does not read, so only the type's decoding shape is tested, not its use.
- **`MulticamSource`** — deliberately opaque until layer 3. Preserved verbatim,
  interpreted nowhere.
- **`TextStyle.isBold` / `isItalic` inference** — approximated by a font-name
  heuristic. The original reads installed font metadata through Core Text, so the same
  file can decode differently on two Macs. Documented in research.md T005; the
  heuristic is covered by `full_fixture_decodes_with_expected_values`, which asserts
  `Helvetica-Bold` infers bold.

## What none of this proves

Every test here establishes **self-consistency**: what this crate writes, it reads
back. Nothing in this repository can execute the Swift decoder, so interoperability
with macOS is unverified.

**T060 is deferred, not pending.** As of 2026-08-30 the project targets Linux and
Windows only and has no macOS access, so interoperability cannot be verified at
runtime by anyone on this project. Format compatibility is upheld by the code audit in
`research.md` and by these self-consistency tests — that is the strongest claim
available, and it should not be stated as more.
