//! Named blocks of mods inside one load order, and what locking one means.
//!
//! Load order is one integer per mod, and for a handful of mods that is enough
//! to hold in your head. For three hundred it is not, and the arrangement people
//! actually keep in their heads is not a sequence at all: it is "the frameworks,
//! then the big overhauls, then my patches on top". A group gives that structure
//! somewhere to live, and a lock makes it survive contact with everything that
//! rearranges lists: a drag aimed at something else, a bulk action, and later an
//! automatic sort.
//!
//! **A group is a label, not a second ordering.** `priority` remains the only
//! thing that decides which mod wins a file. A group names a contiguous run of
//! it. Nothing here reaches a `ModPlan`, and deployment is unchanged by any of
//! it: the same order deploys the same files whether it is grouped or not.
//!
//! **Membership is stored, never inferred from position.** The tempting model is
//! Mod Organizer's separators, where a group is a divider and a mod belongs to
//! whichever divider sits above it. That model cannot be locked. A lock is a
//! promise about position, and membership derived from position is defined by
//! the very thing the lock exists to freeze, so "did that drop join the group or
//! pass over it?" would have no answer except "whichever the drop did".
//!
//! **What a lock stops.** A locked group's members stay together, in the order
//! they are in, and nothing else can come between them. What it does not stop is
//! the block moving as a block: dragging the group itself is a deliberate
//! gesture aimed at the group, not a side effect of aiming at something else.
//! Nor does it stop renaming, recolouring, collapsing, or switching a member on
//! and off. A lock holds the order, not the label and not the switches.
//!
//! Pure, like the rest of this crate. Reading and writing rows is
//! `apoc-storage`'s job, and the sentence a person reads when a lock refuses
//! them belongs with the error type there.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which group each mod belongs to. Absent means ungrouped.
pub type Membership = BTreeMap<String, i64>;

/// A named block of mods within one profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModGroup {
    pub id: i64,
    pub name: String,
    /// A colour *token*, resolved against `theme.css` when it is painted, never
    /// a literal. Appearance retunes those variables at runtime, so a stored
    /// hex value would strand a group at a colour the current theme does not
    /// contain, which for some themes means invisible.
    pub color: String,
    pub locked: bool,
    pub collapsed: bool,
}

/// Where something being moved should land.
///
/// Anchored to a neighbouring mod rather than to an index, because an index is
/// only meaningful in the list that produced it. This list can be searched,
/// filtered and collapsed, so the row above a drop point is very often not the
/// entry above it in the true order, and a client that has not yet seen the last
/// change would name a position that has since moved. An id means the same thing
/// in every one of those cases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "at", content = "anchor")]
pub enum Placement {
    Start,
    End,
    Before(String),
    After(String),
}

/// What is being moved: one mod, or a whole group as a block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "id")]
pub enum MoveSubject {
    Mod(String),
    Group(i64),
}

/// What the move does to the moved mod's membership.
///
/// Carried explicitly rather than worked out from where the mod landed. "I
/// dragged this into Frameworks" and "I dragged this past Frameworks" put the
/// mod in the same place and mean different things, and only the gesture knows
/// which happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "groupId")]
pub enum Belonging {
    /// Leave membership exactly as it is. What a group move always means.
    Keep,
    Join(i64),
    Leave,
}

/// One drag, described so it can be replayed against the order as it now is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderMove {
    pub subject: MoveSubject,
    pub placement: Placement,
    pub belonging: Belonging,
}

/// Why a requested order was refused.
///
/// Data, not a sentence. Both ids are carried so the refusal can name the group
/// and the mod in the words the person chose, rather than making them match an
/// id against a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockBreach {
    /// A member of a locked group left it, or its members changed order among
    /// themselves.
    MemberMoved { group_id: i64, mod_id: String },
    /// Something that is not a member landed between two that are.
    Split { group_id: i64, mod_id: String },
}

impl LockBreach {
    pub fn group_id(&self) -> i64 {
        match self {
            LockBreach::MemberMoved { group_id, .. } | LockBreach::Split { group_id, .. } => {
                *group_id
            }
        }
    }

    pub fn mod_id(&self) -> &str {
        match self {
            LockBreach::MemberMoved { mod_id, .. } | LockBreach::Split { mod_id, .. } => mod_id,
        }
    }
}

/// An order, and the membership changes that go with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrangement {
    /// Every mod in the profile, in load order.
    pub order: Vec<String>,
    /// Mods whose group changed, and what it changed to. `None` is ungrouped.
    pub regrouped: Vec<(String, Option<i64>)>,
}

/// The members of one group, in the order they appear.
fn members_of(order: &[String], membership: &Membership, group_id: i64) -> Vec<String> {
    order
        .iter()
        .filter(|id| membership.get(*id) == Some(&group_id))
        .cloned()
        .collect()
}

/// Whether a requested order is legal, given what is locked.
///
/// Consulted by every writer rather than by the screen. The screen's job is to
/// make a refusal rare; it is not the reason one does not happen, because the
/// command line reaches the same rows and a future automatic sort will too.
pub fn check_order(
    current: &[String],
    requested: &[String],
    groups: &[ModGroup],
    membership: &Membership,
) -> Result<(), LockBreach> {
    for group in groups.iter().filter(|g| g.locked) {
        let was = members_of(current, membership, group.id);
        if was.len() < 2 {
            // One mod cannot be split from itself, and cannot change its order
            // relative to nothing.
            continue;
        }
        let now = members_of(requested, membership, group.id);

        // Same members, same sequence. Anything else means one of them was
        // aimed at individually, which is what the lock is for.
        if now != was {
            let strayed = now
                .iter()
                .zip(was.iter())
                .find(|(a, b)| a != b)
                .map(|(a, _)| a.clone())
                .unwrap_or_else(|| was[0].clone());
            return Err(LockBreach::MemberMoved {
                group_id: group.id,
                mod_id: strayed,
            });
        }

        // Contiguous: nothing that is not a member sits between the first and
        // the last of them.
        let first = requested.iter().position(|id| id == &now[0]);
        let last = requested.iter().rposition(|id| id == &now[now.len() - 1]);
        if let (Some(first), Some(last)) = (first, last) {
            if let Some(intruder) = requested[first..=last]
                .iter()
                .find(|id| membership.get(*id) != Some(&group.id))
            {
                return Err(LockBreach::Split {
                    group_id: group.id,
                    mod_id: intruder.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Pull every group's members back into one run.
///
/// A run is where a group's earliest member already sits, so gathering moves the
/// stragglers to the block rather than moving the block to the stragglers: the
/// mods somebody deliberately placed stay put, and the one that wandered comes
/// back to them.
pub fn gather(order: &[String], membership: &Membership) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(order.len());
    let mut placed: Vec<i64> = Vec::new();

    for id in order {
        match membership.get(id) {
            Some(group_id) => {
                if placed.contains(group_id) {
                    // Already emitted with the rest of its group.
                    continue;
                }
                placed.push(*group_id);
                out.extend(members_of(order, membership, *group_id));
            }
            None => out.push(id.clone()),
        }
    }
    out
}

/// Replay one drag against the order as it currently stands.
///
/// Contiguity is repaired here rather than refused, because a drop is an intent
/// somebody expressed with a gesture and the honest answer is the arrangement
/// they were drawing. A *locked* group is not repaired, because there is no
/// arrangement they could have been drawing that the lock permits.
pub fn apply_move(
    current: &[String],
    groups: &[ModGroup],
    membership: &Membership,
    mv: &OrderMove,
) -> Result<Arrangement, LockBreach> {
    let moving: Vec<String> = match &mv.subject {
        MoveSubject::Mod(id) => vec![id.clone()],
        MoveSubject::Group(group_id) => members_of(current, membership, *group_id),
    };
    if moving.is_empty() {
        return Ok(Arrangement {
            order: current.to_vec(),
            regrouped: Vec::new(),
        });
    }

    let mut after = membership.clone();
    let mut regrouped = Vec::new();
    if let MoveSubject::Mod(id) = &mv.subject {
        match mv.belonging {
            Belonging::Keep => {}
            Belonging::Join(group_id) => {
                if after.insert(id.clone(), group_id) != Some(group_id) {
                    regrouped.push((id.clone(), Some(group_id)));
                }
            }
            Belonging::Leave => {
                if after.remove(id).is_some() {
                    regrouped.push((id.clone(), None));
                }
            }
        }
    }

    let rest: Vec<String> = current
        .iter()
        .filter(|id| !moving.contains(id))
        .cloned()
        .collect();

    // An anchor that is itself being moved cannot be landed against, and a
    // client naming one is describing a drop onto the thing it is dragging.
    let at = match &mv.placement {
        Placement::Start => 0,
        Placement::End => rest.len(),
        Placement::Before(anchor) => rest
            .iter()
            .position(|id| id == anchor)
            .unwrap_or(rest.len()),
        Placement::After(anchor) => rest
            .iter()
            .position(|id| id == anchor)
            .map(|i| i + 1)
            .unwrap_or(rest.len()),
    };

    let mut order = rest;
    for (offset, id) in moving.into_iter().enumerate() {
        order.insert(at + offset, id);
    }
    let order = gather(&order, &after);

    check_order(current, &order, groups, &after)?;
    // Membership changed, so the "same members" test above cannot see a member
    // that left a locked group. Catch that separately.
    for (id, to) in &regrouped {
        if let Some(from) = membership.get(id) {
            if groups.iter().any(|g| g.id == *from && g.locked) && Some(*from) != *to {
                return Err(LockBreach::MemberMoved {
                    group_id: *from,
                    mod_id: id.clone(),
                });
            }
        }
        if let Some(to) = to {
            if groups.iter().any(|g| g.id == *to && g.locked) {
                return Err(LockBreach::Split {
                    group_id: *to,
                    mod_id: id.clone(),
                });
            }
        }
    }

    Ok(Arrangement { order, regrouped })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: i64, locked: bool) -> ModGroup {
        ModGroup {
            id,
            name: format!("Group {id}"),
            color: "default".into(),
            locked,
            collapsed: false,
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// `a b c` in group 1, `x y` loose.
    fn world() -> (Vec<String>, Membership) {
        let order = ids(&["a", "b", "c", "x", "y"]);
        let mut membership = Membership::new();
        for id in ["a", "b", "c"] {
            membership.insert(id.to_string(), 1);
        }
        (order, membership)
    }

    #[test]
    fn a_reorder_that_lifts_one_mod_out_of_a_locked_group_is_refused() {
        let (order, membership) = world();
        let requested = ids(&["a", "c", "x", "b", "y"]);
        let err = check_order(&order, &requested, &[group(1, true)], &membership).unwrap_err();
        assert_eq!(err.group_id(), 1);
    }

    #[test]
    fn a_reorder_that_drops_a_stranger_between_two_members_of_a_locked_group_is_refused() {
        let (order, membership) = world();
        let requested = ids(&["a", "b", "x", "c", "y"]);
        assert_eq!(
            check_order(&order, &requested, &[group(1, true)], &membership),
            Err(LockBreach::Split {
                group_id: 1,
                mod_id: "x".into()
            })
        );
    }

    #[test]
    fn a_reorder_that_shuffles_a_locked_groups_members_among_themselves_is_refused() {
        let (order, membership) = world();
        let requested = ids(&["b", "a", "c", "x", "y"]);
        assert!(check_order(&order, &requested, &[group(1, true)], &membership).is_err());
    }

    #[test]
    fn a_locked_block_can_still_be_carried_whole_because_that_gesture_aims_at_the_group() {
        let (order, membership) = world();
        let requested = ids(&["x", "y", "a", "b", "c"]);
        assert_eq!(
            check_order(&order, &requested, &[group(1, true)], &membership),
            Ok(())
        );
    }

    #[test]
    fn an_unlocked_group_refuses_nothing_at_all() {
        let (order, membership) = world();
        let requested = ids(&["a", "x", "b", "y", "c"]);
        assert_eq!(
            check_order(&order, &requested, &[group(1, false)], &membership),
            Ok(())
        );
    }

    #[test]
    fn a_profile_with_no_groups_is_the_same_unconstrained_order_it_always_was() {
        let order = ids(&["a", "b", "c"]);
        let requested = ids(&["c", "b", "a"]);
        assert_eq!(
            check_order(&order, &requested, &[], &Membership::new()),
            Ok(())
        );
    }

    #[test]
    fn dropping_a_loose_mod_below_a_named_row_lands_it_immediately_after_that_row() {
        let (order, membership) = world();
        let mv = OrderMove {
            subject: MoveSubject::Mod("y".into()),
            placement: Placement::After("x".into()),
            belonging: Belonging::Keep,
        };
        let out = apply_move(&order, &[group(1, false)], &membership, &mv).unwrap();
        assert_eq!(out.order, ids(&["a", "b", "c", "x", "y"]));
    }

    #[test]
    fn a_drop_that_would_land_inside_an_unlocked_group_without_joining_it_gathers_the_group_instead(
    ) {
        let (order, membership) = world();
        let mv = OrderMove {
            subject: MoveSubject::Mod("x".into()),
            placement: Placement::After("a".into()),
            belonging: Belonging::Leave,
        };
        let out = apply_move(&order, &[group(1, false)], &membership, &mv).unwrap();
        // The group closes back up rather than being quietly split by a mod
        // that did not join it.
        assert_eq!(out.order, ids(&["a", "b", "c", "x", "y"]));
    }

    #[test]
    fn joining_a_group_puts_the_mod_inside_the_run_and_says_so() {
        let (order, membership) = world();
        let mv = OrderMove {
            subject: MoveSubject::Mod("x".into()),
            placement: Placement::After("a".into()),
            belonging: Belonging::Join(1),
        };
        let out = apply_move(&order, &[group(1, false)], &membership, &mv).unwrap();
        assert_eq!(out.order, ids(&["a", "x", "b", "c", "y"]));
        assert_eq!(out.regrouped, vec![("x".to_string(), Some(1))]);
    }

    #[test]
    fn joining_a_locked_group_is_refused_because_the_lock_is_about_what_sits_between_its_mods() {
        let (order, membership) = world();
        let mv = OrderMove {
            subject: MoveSubject::Mod("x".into()),
            placement: Placement::After("a".into()),
            belonging: Belonging::Join(1),
        };
        assert!(apply_move(&order, &[group(1, true)], &membership, &mv).is_err());
    }

    #[test]
    fn leaving_a_locked_group_is_refused_even_though_the_survivors_stay_in_order() {
        let (order, membership) = world();
        let mv = OrderMove {
            subject: MoveSubject::Mod("c".into()),
            placement: Placement::End,
            belonging: Belonging::Leave,
        };
        assert!(apply_move(&order, &[group(1, true)], &membership, &mv).is_err());
    }

    #[test]
    fn dragging_a_locked_group_moves_every_member_and_keeps_their_sequence() {
        let (order, membership) = world();
        let mv = OrderMove {
            subject: MoveSubject::Group(1),
            placement: Placement::End,
            belonging: Belonging::Keep,
        };
        let out = apply_move(&order, &[group(1, true)], &membership, &mv).unwrap();
        assert_eq!(out.order, ids(&["x", "y", "a", "b", "c"]));
        assert!(out.regrouped.is_empty());
    }

    #[test]
    fn a_group_of_one_is_not_something_a_lock_can_be_broken_against() {
        let order = ids(&["a", "x"]);
        let mut membership = Membership::new();
        membership.insert("a".into(), 1);
        let requested = ids(&["x", "a"]);
        assert_eq!(
            check_order(&order, &requested, &[group(1, true)], &membership),
            Ok(())
        );
    }

    #[test]
    fn an_anchor_that_is_itself_being_dragged_lands_the_move_at_the_end_rather_than_nowhere() {
        let (order, membership) = world();
        let mv = OrderMove {
            subject: MoveSubject::Group(1),
            placement: Placement::After("b".into()),
            belonging: Belonging::Keep,
        };
        let out = apply_move(&order, &[group(1, false)], &membership, &mv).unwrap();
        assert_eq!(out.order, ids(&["x", "y", "a", "b", "c"]));
    }
}
