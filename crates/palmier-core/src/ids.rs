//! Identifier materialization, kept separate from decoding.
//!
//! The original generates a fresh UUID inside its decoder when an id is missing, which
//! makes decoding non-deterministic. Splitting it out reproduces the same observable
//! result — after a load every entity has an id — while leaving decode a pure function
//! so the round-trip comparison is exactly reproducible (plan.md Q4).

use crate::project::ProjectFile;

fn fill(slot: &mut Option<String>) {
    if slot.is_none() {
        *slot = Some(uuid::Uuid::new_v4().to_string());
    }
}

/// Fill every absent id in the project with a fresh UUID.
pub fn materialize_ids(project: &mut ProjectFile) {
    for timeline in &mut project.timelines {
        fill(&mut timeline.id);
        for track in &mut timeline.tracks {
            fill(&mut track.id);
            for clip in &mut track.clips {
                fill(&mut clip.id);
                if let Some(effects) = clip.effects.as_mut() {
                    for effect in effects {
                        fill(&mut effect.id);
                    }
                }
            }
        }
    }
}
