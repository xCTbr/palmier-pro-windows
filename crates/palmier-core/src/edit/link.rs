//! Link groups: clips that move, trim, and delete as one unit.

use std::collections::{BTreeMap, BTreeSet};

use crate::timeline::Timeline;

/// Reverse index from link group id to its member clip ids, in track then clip order.
pub fn link_index(timeline: &Timeline) -> BTreeMap<String, Vec<String>> {
    let mut index: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for track in &timeline.tracks {
        for clip in &track.clips {
            if let (Some(group), Some(id)) = (&clip.link_group_id, &clip.id) {
                index.entry(group.clone()).or_default().push(id.clone());
            }
        }
    }
    index
}

/// Every clip sharing a link group with any of `ids`, including `ids` themselves.
///
/// Commands expand their targets through this before validating, so the
/// refuse-or-apply decision covers every clip the command will actually touch.
pub fn expand_to_link_groups(timeline: &Timeline, ids: &BTreeSet<String>) -> BTreeSet<String> {
    let index = link_index(timeline);
    let mut clip_to_group: BTreeMap<&str, &str> = BTreeMap::new();
    for (group, members) in &index {
        for member in members {
            clip_to_group.insert(member.as_str(), group.as_str());
        }
    }

    let groups: BTreeSet<&str> = ids
        .iter()
        .filter_map(|id| clip_to_group.get(id.as_str()).copied())
        .collect();
    if groups.is_empty() {
        return ids.clone();
    }

    let mut result = ids.clone();
    for group in groups {
        if let Some(members) = index.get(group) {
            result.extend(members.iter().cloned());
        }
    }
    result
}

/// Clips sharing `clip_id`'s group, excluding `clip_id`.
pub fn partners_of(timeline: &Timeline, clip_id: &str) -> Vec<String> {
    for members in link_index(timeline).values() {
        if members.iter().any(|m| m == clip_id) {
            return members.iter().filter(|m| *m != clip_id).cloned().collect();
        }
    }
    Vec::new()
}

/// Partners that should receive a timing change applied uniformly to `ids`, excluding
/// any clip already in `ids`.
pub fn timing_propagation_partners(
    timeline: &Timeline,
    ids: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for id in ids {
        for partner in partners_of(timeline, id) {
            if !ids.contains(&partner) {
                out.insert(partner);
            }
        }
    }
    out
}
