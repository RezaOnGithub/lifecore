//! The rules of PLANNING §8.2 and §8.3, judged at the prefix of an edge with
//! one of planning's labels.

use crate::{
    current::{Current, ItemState, StanceFields, planning_type},
    findings::{Completion, PlanningMessage},
    payload::PayloadError,
    vocabulary::{Label, PlanningType, StanceKind},
};
use lifecore::{
    Edge, EntryId, View,
    inspection::{Currentness, Direction, EdgeSelection},
};
use lifecore_dag_check::{CycleDisposition, DagCheckConfig, SubgraphFilter, check_dag};

// Judges `edge`, the newest entry of `view`.
pub(crate) fn judge(
    view: View<'_>,
    edge: &Edge,
    label: Label,
) -> Result<Vec<PlanningMessage>, PayloadError> {
    let endpoints = (planning_type(view, edge.from), planning_type(view, edge.to));
    let permitted = matches!(endpoints, (Some(from), Some(to)) if label.permits(from, to));
    let mut findings = Vec::new();
    if !permitted {
        findings.push(PlanningMessage::BadEndpoints {
            edge: edge.id,
            label,
            from_type: view.node(edge.from).expect("validated").ty_name.clone(),
            to_type: view.node(edge.to).expect("validated").ty_name.clone(),
        });
    }
    match label {
    Label::Enables | Label::Helps => findings.extend(dependency(view, edge, label)?),
    // The rules of §8.3 read the source's kind and the target's type, so an
    // edge between other types has nothing more to judge.
    Label::About | Label::Until if permitted => {
        let (Some(from), Some(to)) = endpoints else {
            unreachable!("permitted endpoints are planning types")
        };
        findings.extend(bookkeeping(view, edge, label, from, to)?);
    }
    Label::About | Label::Until => {}
    }
    Ok(findings)
}

// Section 8.2: a terminal target, and the cycle the edge closes.
fn dependency(
    view: View<'_>,
    edge: &Edge,
    label: Label,
) -> Result<Vec<PlanningMessage>, PayloadError> {
    let mut findings = Vec::new();
    let state = Current::new(view)?.state(edge.to);
    if state.is_terminal() {
        findings.push(PlanningMessage::TerminalTarget {
            edge: edge.id,
            label,
            state,
        });
    }
    // Only a component that holds this edge is new here, so each cycle is
    // reported once, at the edge that closes it.
    let config = DagCheckConfig {
        filter: Some(SubgraphFilter {
            node_types: None,
            edge_types: Some(
                [Label::Enables, Label::Helps]
                    .map(|label| label.name().to_string())
                    .into(),
            ),
        }),
        on_cycle: CycleDisposition::Inconsistency,
        range: None,
    };
    findings.extend(
        check_dag(view, &config)
            .components
            .into_iter()
            .filter(|component| component.edges.contains(&edge.id))
            .map(|component| PlanningMessage::Cycle {
                edge: edge.id,
                component,
            }),
    );
    Ok(findings)
}

// Section 8.3, for an `About` or `Until` edge whose endpoint types §3.2
// permits. Everything is judged in the view immediately before the edge,
// since the `About` edge of a `Done`, or of an `abandoned` or `sufficient`
// stance, makes its own target terminal.
fn bookkeeping(
    view: View<'_>,
    edge: &Edge,
    label: Label,
    from: PlanningType,
    to: PlanningType,
) -> Result<Vec<PlanningMessage>, PayloadError> {
    use PlanningMessage::*;
    let before = view
        .prefix(edge.id.position())
        .expect("edge is inside view");
    let current = Current::new(before)?;
    let source = current.node(edge.from);
    let kind = match from {
    PlanningType::Stance => StanceFields::read(source)?.kind,
    _ => None,
    };
    let earlier = earlier_targets(before, edge.from, label);
    let mut findings = Vec::new();
    match label {
    Label::About => {
        if let Some(kind) = kind
            && !kind.applies_to(to)
        {
            findings.push(KindDoesNotApply {
                edge: edge.id,
                kind,
                target_type: to,
            });
        }
        // A `Done` is never re-pointed, since its target is final (decision
        // 59).
        if from == PlanningType::Done
            && let Some(&earlier) = earlier.first()
        {
            findings.push(SecondDoneAbout {
                edge: edge.id,
                earlier: earlier.0,
            });
        }
        let state = current.state(edge.to);
        if state.is_terminal() {
            findings.push(AboutTerminal {
                edge: edge.id,
                state,
            });
        }
        let completion = match (from, kind) {
        (PlanningType::Done, _) => Some(Completion::Done),
        (_, Some(StanceKind::Sufficient)) => Some(Completion::Sufficient),
        (_, Some(StanceKind::Abandoned)) => Some(Completion::Abandoned),
        _ => None,
        };
        if let Some(completion) = completion {
            findings.extend(premature(&current, edge, completion));
        }
        if from == PlanningType::Stance
            && let Some(other) = current.current_stance(edge.to)
            && other != edge.from
        {
            findings.push(SecondCurrentStance {
                edge: edge.id,
                current: other,
            });
        }
    }
    Label::Until => {
        if let Some(kind) = kind
            && kind != StanceKind::Deferred
        {
            findings.push(UntilFromNonDeferral {
                edge: edge.id,
                kind,
            });
        }
        if let Some(&(other, _)) = earlier
            .iter()
            .find(|(id, _)| before.is_stale(*id) == Some(false))
        {
            findings.push(SecondUntil {
                edge: edge.id,
                current: other,
            });
        }
    }
    Label::Enables | Label::Helps => unreachable!("dependency labels are judged by section 8.2"),
    }
    // A stance or `Did` keeps its position, and so its lifetime, when
    // re-pointed to a revision of its target (decisions 5, 57, and 73).
    if matches!(from, PlanningType::Stance | PlanningType::Did)
        && !earlier.is_empty()
        && !before
            .ancestors(edge.to)
            .any(|ancestor| earlier.iter().any(|(_, target)| *target == ancestor.id))
    {
        findings.push(UnrelatedRepoint {
            edge: edge.id,
            label,
        });
    }
    Ok(findings)
}

// The earlier edges with `label` from `source`, current or historical, each
// with its target.
fn earlier_targets(before: View<'_>, source: EntryId, label: Label) -> Vec<(EntryId, EntryId)> {
    let selection = EdgeSelection {
        ty_name: Some(label.name()),
        currentness: Currentness::All,
    };
    before
        .relationships(source, Direction::Outgoing, selection)
        .map(|edge| (edge.id, edge.to))
        .collect()
}

// A `Done` or `sufficient` waits for every `Enables` source of its target to
// be done, and `abandoned` for none of them to be open, so that each
// prerequisite is settled by a decision of its own (decisions 10, 28, 74).
fn premature(
    current: &Current<'_>,
    edge: &Edge,
    completion: Completion,
) -> Option<PlanningMessage> {
    let waits_for = |state: ItemState| match completion {
    Completion::Done | Completion::Sufficient => state != ItemState::Done,
    Completion::Abandoned => state == ItemState::Open,
    };
    let mut sources = current
        .enables_sources(edge.to)
        .into_iter()
        .filter(|&source| waits_for(current.state(source)))
        .collect::<Vec<_>>();
    sources.sort();
    (!sources.is_empty()).then_some(PlanningMessage::PrematureCompletion {
        edge: edge.id,
        completion,
        sources,
    })
}
