//! The rules of PLANNING §8.1 judged at a node's own prefix: the kind
//! vocabulary, conditional fields, and signs of a planning node, and what may
//! succeed what.

use crate::{
    config::PlanningConfig,
    current::{Current, StanceFields},
    findings::PlanningMessage,
    payload::{self, PayloadError},
    vocabulary::{PlanningType, StanceKind},
};
use lifecore::{Node, View};

// Judges `node`, the newest entry of `view`. It is a node of a planning type,
// a node whose predecessor has one, or both.
pub(crate) fn judge(
    view: View<'_>,
    node: &Node,
    config: &PlanningConfig,
) -> Result<Vec<PlanningMessage>, PayloadError> {
    let mut findings = Vec::new();
    let ty = PlanningType::from_name(&node.ty_name);
    match ty {
    Some(PlanningType::Stance) => findings.extend(stance_fields(node, config)?),
    Some(PlanningType::Review) => {
        let budget = payload::integer(node, "budget")?;
        if budget < 0 {
            findings.push(PlanningMessage::NegativeBudget {
                review: node.id,
                budget,
            });
        }
    }
    Some(PlanningType::Habit | PlanningType::Concern) => {
        let quota = payload::integer(node, "quota")?;
        if quota < 0 {
            findings.push(PlanningMessage::NegativeQuota {
                item: node.id,
                quota,
            });
        }
    }
    _ => {}
    }
    if node.predecessor.is_some() {
        findings.extend(succession(view, node, ty)?);
    }
    Ok(findings)
}

// The kind vocabulary, the conditional fields of §3.3, their signs, and the
// deferral cap. An unknown kind has no conditional fields to judge.
fn stance_fields(
    node: &Node,
    config: &PlanningConfig,
) -> Result<Vec<PlanningMessage>, PayloadError> {
    use PlanningMessage::*;
    let stance = node.id;
    let fields = StanceFields::read(node)?;
    let mut findings = Vec::new();
    if fields.kind.is_none() {
        findings.push(UnknownKind {
            stance,
            kind: payload::text(node, "kind")?.into(),
        });
    }
    if let Some(cost) = fields.cost.filter(|cost| *cost < 0) {
        findings.push(NegativeCost { stance, cost });
    }
    if let Some(horizon) = fields.horizon.filter(|horizon| *horizon < 1) {
        findings.push(HorizonBelowOne { stance, horizon });
    }
    let Some(kind) = fields.kind else {
        return Ok(findings);
    };
    if kind.takes_cost() && fields.cost.is_none() {
        findings.push(MissingCost { stance });
    }
    if kind.takes_horizon() && fields.horizon.is_none() {
        findings.push(MissingHorizon { stance, kind });
    }
    if !kind.takes_cost() && fields.cost.is_some() {
        findings.push(ForbiddenField {
            stance,
            kind,
            field: "cost",
        });
    }
    if !kind.takes_horizon() && fields.horizon.is_some() {
        findings.push(ForbiddenField {
            stance,
            kind,
            field: "horizon",
        });
    }
    // Only a deferral is capped; a `waiting` external stays in play.
    if kind == StanceKind::Deferred
        && let (Some(horizon), Some(cap)) = (fields.horizon, config.max_deferral_horizon)
        && u64::try_from(horizon).is_ok_and(|horizon| horizon > cap.get())
    {
        findings.push(HorizonOverCap {
            stance,
            horizon,
            cap: cap.get(),
        });
    }
    Ok(findings)
}

// What may succeed what. Whether the predecessor is terminal is judged in
// the view immediately before the successor, as TypeCheck judges a type
// transition: at the successor's own prefix the predecessor is superseded,
// its `About` edges are historical, and its state reads open (§4).
fn succession(
    view: View<'_>,
    node: &Node,
    ty: Option<PlanningType>,
) -> Result<Vec<PlanningMessage>, PayloadError> {
    use PlanningMessage::*;
    let before = view
        .prefix(node.id.position())
        .expect("node is inside view");
    let predecessor = before
        .node(node.predecessor.expect("a successor"))
        .expect("validated predecessor");
    let predecessor_ty = PlanningType::from_name(&predecessor.ty_name);
    let mut findings = Vec::new();
    if let Some(ty @ (PlanningType::Review | PlanningType::Done | PlanningType::Did)) = ty {
        findings.push(RevisedBookkeeping { node: node.id, ty });
    }
    if let (Some(to), Some(from)) = (ty, predecessor_ty)
        && to.is_item() != from.is_item()
    {
        findings.push(CrossesBookkeeping {
            node: node.id,
            from,
            to,
        });
    }
    match predecessor_ty {
    Some(item) if item.is_item() => {
        let state = Current::new(before)?.state(predecessor.id);
        if state.is_terminal() {
            findings.push(SuccessorOfTerminal {
                node: node.id,
                predecessor: predecessor.id,
                state,
            });
        }
    }
    // Abandoning an item or calling a goal sufficient is as final as a
    // `Done`, whatever would replace the stance (decision 56).
    Some(PlanningType::Stance) => {
        if let Some(kind @ (StanceKind::Abandoned | StanceKind::Sufficient)) =
            StanceFields::read(predecessor)?.kind
        {
            findings.push(SuccessorOfFinalStance {
                node: node.id,
                predecessor: predecessor.id,
                kind,
            });
        }
    }
    _ => {}
    }
    Ok(findings)
}
