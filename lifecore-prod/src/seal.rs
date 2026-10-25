//! The protocol rules judged at a seal (PLANNING §8.4 and §8.5), at the
//! prefix whose newest entry is a review.

use crate::{
    config::PlanningConfig,
    current::{ItemState, current_edges},
    findings::{PlanningMessage, Superseded},
    payload::{self, PayloadError},
    situation::Situation,
    vocabulary::{Label, PlanningType, StanceKind},
};
use lifecore::{Edge, EntryId, Node, View};
use std::collections::BTreeSet;

// Judges the seal at `review`, the newest entry of `view`.
pub(crate) fn judge(
    view: View<'_>,
    review: &Node,
    config: &PlanningConfig,
) -> Result<Vec<PlanningMessage>, PayloadError> {
    // Adopting the analyzer on a database that already holds reviews must
    // not lock it behind a historical seal (decision 25).
    if config
        .first_sealed_review
        .is_some_and(|first| review.id.position() < first)
    {
        return Ok(Vec::new());
    }
    let situation = Situation::at_seal(view)?;
    let review = review.id;
    let mut findings = Vec::new();
    findings.extend(undecided(&situation, review));
    findings.extend(budget_overrun(&situation, review)?);
    findings.extend(broken_plans(&situation, review));
    findings.extend(blocked_by_abandoned(&situation, review));
    findings.extend(shortfalls(&situation, review)?);
    findings.extend(grace_periods(&situation, review));
    findings.extend(dropped_connections(&situation, review));
    Ok(findings)
}

fn undecided(situation: &Situation<'_>, review: EntryId) -> Vec<PlanningMessage> {
    situation
        .undecided()
        .into_iter()
        .map(|revision| PlanningMessage::Undecided { revision, review })
        .collect()
}

// Each fresh `committed` stance that is the current stance of at least one
// open task counts once, however many tasks it is about (decision 60). A
// `committed` decision keeps its task in play, so no commitment escapes the
// budget (decision 48).
fn budget_overrun(
    situation: &Situation<'_>,
    review: EntryId,
) -> Result<Option<PlanningMessage>, PayloadError> {
    let current = &situation.current;
    let counted = current
        .items()
        .filter(|&(item, ty)| ty == PlanningType::Task && current.is_open(item))
        .filter_map(|(item, _)| current.current_stance(item))
        .filter(|&stance| {
            current.kind(stance) == Some(StanceKind::Committed) && situation.is_fresh(stance)
        })
        .collect::<BTreeSet<_>>();
    // Section 8.1 rejects a commitment without a cost. Costs are summed wide
    // enough that no number of them can overflow.
    let cost = counted
        .iter()
        .map(|&stance| i128::from(current.stance(stance).and_then(|s| s.cost).unwrap_or(0)))
        .sum::<i128>();
    let budget = payload::integer(current.node(review), "budget")?;
    Ok(
        (cost > i128::from(budget)).then(|| PlanningMessage::BudgetOverrun {
            review,
            stances: counted.into_iter().collect(),
            cost,
            budget,
        }),
    )
}

fn broken_plans(situation: &Situation<'_>, review: EntryId) -> Vec<PlanningMessage> {
    situation
        .broken_plans()
        .into_iter()
        .map(|item| PlanningMessage::BrokenPlan { item, review })
        .collect()
}

// An abandoned prerequisite can never be done, so its target in play can
// never become ready; a dormant target is judged once it returns to play
// (decision 74).
fn blocked_by_abandoned(situation: &Situation<'_>, review: EntryId) -> Vec<PlanningMessage> {
    current_edges(situation.view(), Label::Enables)
        .filter(|edge| {
            situation.current.state(edge.from) == ItemState::Abandoned && situation.in_play(edge.to)
        })
        .map(|edge| PlanningMessage::BlockedByAbandoned {
            edge: edge.id,
            source: edge.from,
            target: edge.to,
            review,
        })
        .collect()
}

// A habit or concern in play that is not new, with fewer of this window's
// `Did` events than its quota. A deferred one is dormant, so never short.
fn shortfalls(
    situation: &Situation<'_>,
    review: EntryId,
) -> Result<Vec<PlanningMessage>, PayloadError> {
    let mut findings = Vec::new();
    for (item, ty) in situation.current.items() {
        if !matches!(ty, PlanningType::Habit | PlanningType::Concern)
            || !situation.in_play(item)
            || situation.is_new(item)
        {
            continue;
        }
        let quota = payload::integer(situation.current.node(item), "quota")?;
        let dids = situation.window_dids(item);
        if i64::try_from(dids.len()).is_ok_and(|have| have < quota) {
            findings.push(PlanningMessage::Shortfall {
                item,
                review,
                dids,
                quota,
            });
        }
    }
    Ok(findings)
}

// A new habit or concern in play is spared Undecided item and Shortfall at
// this seal; the warning says why (decisions 46 and 47).
fn grace_periods(situation: &Situation<'_>, review: EntryId) -> Vec<PlanningMessage> {
    situation
        .current
        .items()
        .filter(|&(item, _)| situation.in_play(item) && situation.is_new(item))
        .map(|(item, _)| PlanningMessage::GracePeriod { item, review })
        .collect()
}

// One warning per connection with a planning label that became historical in
// the window being sealed, because an endpoint was superseded there, and has
// no equivalent current connection: a current edge with the same label and
// direction whose endpoints are each the original endpoint, if still
// current, or one of its current descendants. A connection becomes
// historical once, so it is judged at one seal and never again (decision
// 75). The match is a heuristic and may misfire (decision 17).
fn dropped_connections(situation: &Situation<'_>, review: EntryId) -> Vec<PlanningMessage> {
    let view = situation.view();
    let window = situation.boundary.window();
    // The positions of the window being sealed: after the previous review, if
    // any, and before this one.
    let start = match window {
    0 => 0,
    _ => situation.reviews.review(window).id.position() + 1,
    };
    let in_window = |position: usize| (start..review.position()).contains(&position);
    // The position of the first successor of a superseded node.
    let superseded_at = |node: EntryId| view.children(node).map(|child| child.id.position()).min();
    // The node itself while current, else its current descendants.
    let current_ends = |node: EntryId| {
        view.fresh_successors(node)
            .map(|n| n.id)
            .collect::<Vec<_>>()
    };
    let mut findings = Vec::new();
    for edge in view.edges() {
        let Some(label) = Label::from_name(&edge.ty_name) else {
            continue;
        };
        let became_historical = [edge.from, edge.to]
            .into_iter()
            .filter_map(superseded_at)
            .min();
        if !became_historical.is_some_and(in_window) || !needs_carrying(situation, edge, label) {
            continue;
        }
        let (froms, tos) = (current_ends(edge.from), current_ends(edge.to));
        let equivalent = view.fresh_edges().any(|other| {
            other.ty_name == edge.ty_name && froms.contains(&other.from) && tos.contains(&other.to)
        });
        if equivalent {
            continue;
        }
        let mut superseded = [edge.from, edge.to]
            .into_iter()
            .filter(|&node| superseded_at(node).is_some_and(in_window))
            .map(|revision| Superseded {
                revision,
                successors: current_ends(revision),
            })
            .collect::<Vec<_>>();
        superseded.sort_by_key(|superseded| superseded.revision);
        findings.push(PlanningMessage::DroppedConnection {
            edge: edge.id,
            label,
            superseded,
            review,
        });
    }
    findings
}

// Two connections nothing can or need carry forward (decision 83): the
// `Until` edge of a deferral that a successor of another kind ended, since
// only a deferral may have one, and the `About` edge of a `Did` from an
// earlier window, which decides and counts nothing any more.
fn needs_carrying(situation: &Situation<'_>, edge: &Edge, label: Label) -> bool {
    let view = situation.view();
    match label {
    Label::Until if view.is_stale(edge.from) == Some(true) => view
        .fresh_successors(edge.from)
        .any(|successor| situation.current.kind(successor.id) == Some(StanceKind::Deferred)),
    Label::About
        if view.node(edge.from).expect("validated").ty_name == PlanningType::Did.name() =>
    {
        situation.is_fresh(edge.from)
    }
    _ => true,
    }
}
