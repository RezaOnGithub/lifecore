//! Planning's typed findings (PLANNING §8). Each variant names its exact
//! subject entries and the details that distinguish it, and never a
//! boundary; the certificate is the variant's JSON encoding.

use crate::{
    current::ItemState,
    vocabulary::{Label, PlanningType, StanceKind},
};
use lifecore::{
    EntryId,
    analysis::{InnerMessageType, MessageType},
};
use lifecore_dag_check::CyclicComponent;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum PlanningMessage {
    // Section 8.1, at a node's own prefix.
    UnknownKind {
        stance: EntryId,
        kind: String,
    },
    MissingCost {
        stance: EntryId,
    },
    MissingHorizon {
        stance: EntryId,
        kind: StanceKind,
    },
    ForbiddenField {
        stance: EntryId,
        kind: StanceKind,
        field: &'static str,
    },
    NegativeCost {
        stance: EntryId,
        cost: i64,
    },
    HorizonBelowOne {
        stance: EntryId,
        horizon: i64,
    },
    HorizonOverCap {
        stance: EntryId,
        horizon: i64,
        cap: u64,
    },
    NegativeBudget {
        review: EntryId,
        budget: i64,
    },
    NegativeQuota {
        item: EntryId,
        quota: i64,
    },
    // A `Review`, `Done`, or `Did` with a predecessor.
    RevisedBookkeeping {
        node: EntryId,
        ty: PlanningType,
    },
    // A transition between an item type and a bookkeeping type (decision 72).
    CrossesBookkeeping {
        node: EntryId,
        from: PlanningType,
        to: PlanningType,
    },
    SuccessorOfTerminal {
        node: EntryId,
        predecessor: EntryId,
        state: ItemState,
    },
    SuccessorOfFinalStance {
        node: EntryId,
        predecessor: EntryId,
        kind: StanceKind,
    },
    // Section 8.1, at the prefix of a declaration of a planning type.
    UnmetRequirement {
        declaration: EntryId,
        name: PlanningType,
        requirement: Requirement,
    },
    // Sections 8.2 and 8.3, at the edge's prefix.
    BadEndpoints {
        edge: EntryId,
        label: Label,
        from_type: String,
        to_type: String,
    },
    // Section 8.2.
    TerminalTarget {
        edge: EntryId,
        label: Label,
        state: ItemState,
    },
    Cycle {
        edge: EntryId,
        component: CyclicComponent,
    },
    // Section 8.3.
    KindDoesNotApply {
        edge: EntryId,
        kind: StanceKind,
        target_type: PlanningType,
    },
    UntilFromNonDeferral {
        edge: EntryId,
        kind: StanceKind,
    },
    SecondUntil {
        edge: EntryId,
        current: EntryId,
    },
    SecondDoneAbout {
        edge: EntryId,
        earlier: EntryId,
    },
    AboutTerminal {
        edge: EntryId,
        state: ItemState,
    },
    // A `Done`, `sufficient`, or `abandoned` whose target still has an
    // `Enables` source that it waits for; `sources` are those sources.
    PrematureCompletion {
        edge: EntryId,
        completion: Completion,
        sources: Vec<EntryId>,
    },
    UnrelatedRepoint {
        edge: EntryId,
        label: Label,
    },
    SecondCurrentStance {
        edge: EntryId,
        current: EntryId,
    },
    // Section 8.4, at a seal.
    Undecided {
        revision: EntryId,
        review: EntryId,
    },
    BudgetOverrun {
        review: EntryId,
        stances: Vec<EntryId>,
        cost: i128,
        budget: i64,
    },
    BrokenPlan {
        item: EntryId,
        review: EntryId,
    },
    BlockedByAbandoned {
        edge: EntryId,
        source: EntryId,
        target: EntryId,
        review: EntryId,
    },
    // Section 8.5, at a seal.
    Shortfall {
        item: EntryId,
        review: EntryId,
        dids: Vec<EntryId>,
        quota: i64,
    },
    GracePeriod {
        item: EntryId,
        review: EntryId,
    },
    DroppedConnection {
        edge: EntryId,
        label: Label,
        // Each endpoint superseded in the window, in order.
        superseded: Vec<Superseded>,
        review: EntryId,
    },
    // Section 8.6.
    Bootstrap {
        name: PlanningType,
    },
    // Section 10; `None` when the view holds no review.
    StaleReview {
        review: Option<EntryId>,
    },
}

// An endpoint of a dropped connection that was superseded in the window,
// with its current successors.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Superseded {
    pub revision: EntryId,
    pub successors: Vec<EntryId>,
}

// One requirement of section 3.4 on a declaration of a planning type.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum Requirement {
    // Every label the canonical declaration permits, in the same direction.
    PermitsLabel {
        label: Label,
        outgoing: bool,
    },
    // Bookkeeping types are never replaced by a node of another type.
    NoSuccessors,
    Field {
        field: &'static str,
        rule: FieldRule,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum FieldRule {
    MandatoryText,
    MandatoryInteger,
    MandatoryTimestamp,
    // Declared as integer under any policy but mandatory.
    OptionalInteger,
    // Declared as timestamp under any policy, or not declared at all.
    TimestampIfDeclared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum Completion {
    // A `Done` waits for every `Enables` source to be done.
    Done,
    // `sufficient` waits for every `Enables` source to be done.
    Sufficient,
    // `abandoned` waits for no `Enables` source to be open (decision 74).
    Abandoned,
}

impl InnerMessageType for PlanningMessage {
    fn ty(&self) -> MessageType {
        match self {
        Self::Bootstrap { .. } => MessageType::Suggestion,
        Self::Shortfall { .. }
        | Self::GracePeriod { .. }
        | Self::DroppedConnection { .. }
        | Self::StaleReview { .. } => MessageType::Warning,
        _ => MessageType::Inconsistency,
        }
    }
}

impl PlanningMessage {
    // The seal warnings judge a closed window once, at its seal, so they
    // must outlive that prefix in current reports (decision 18).
    pub fn sticky(&self) -> bool {
        matches!(
            self,
            Self::Shortfall { .. } | Self::GracePeriod { .. } | Self::DroppedConnection { .. }
        )
    }

    // Where a rule lists no subjects, they are the judged entry, and at a
    // seal the judged revision and the review (§8).
    pub fn subjects(&self) -> BTreeSet<EntryId> {
        use PlanningMessage::*;
        match self {
        UnknownKind { stance, .. }
        | MissingCost { stance }
        | MissingHorizon { stance, .. }
        | ForbiddenField { stance, .. }
        | NegativeCost { stance, .. }
        | HorizonBelowOne { stance, .. }
        | HorizonOverCap { stance, .. } => [*stance].into(),
        NegativeBudget { review, .. } => [*review].into(),
        NegativeQuota { item, .. } => [*item].into(),
        RevisedBookkeeping { node, .. }
        | CrossesBookkeeping { node, .. }
        | SuccessorOfTerminal { node, .. }
        | SuccessorOfFinalStance { node, .. } => [*node].into(),
        UnmetRequirement { declaration, .. } => [*declaration].into(),
        BadEndpoints { edge, .. }
        | TerminalTarget { edge, .. }
        | KindDoesNotApply { edge, .. }
        | UntilFromNonDeferral { edge, .. }
        | SecondUntil { edge, .. }
        | SecondDoneAbout { edge, .. }
        | AboutTerminal { edge, .. }
        | PrematureCompletion { edge, .. }
        | UnrelatedRepoint { edge, .. }
        | SecondCurrentStance { edge, .. } => [*edge].into(),
        Cycle { component, .. } => component
            .nodes
            .iter()
            .chain(&component.edges)
            .copied()
            .collect(),
        Undecided { revision, review } => [*revision, *review].into(),
        BudgetOverrun {
            review, stances, ..
        } => std::iter::once(review).chain(stances).copied().collect(),
        BrokenPlan { item, review } | GracePeriod { item, review } => [*item, *review].into(),
        BlockedByAbandoned {
            edge,
            source,
            target,
            review,
        } => [*edge, *source, *target, *review].into(),
        Shortfall {
            item, review, dids, ..
        } => [*item, *review]
            .into_iter()
            .chain(dids.iter().copied())
            .collect(),
        DroppedConnection {
            edge,
            superseded,
            review,
            ..
        } => [*edge, *review]
            .into_iter()
            .chain(superseded.iter().flat_map(|superseded| {
                std::iter::once(superseded.revision).chain(superseded.successors.iter().copied())
            }))
            .collect(),
        Bootstrap { .. } => BTreeSet::new(),
        StaleReview { review } => review.iter().copied().collect(),
        }
    }

    // The entry a reader should look at first.
    pub fn focus(&self) -> Option<EntryId> {
        use PlanningMessage::*;
        match self {
        Cycle { edge, .. } | DroppedConnection { edge, .. } => Some(*edge),
        Undecided { revision, .. } => Some(*revision),
        BrokenPlan { item, .. } | Shortfall { item, .. } | GracePeriod { item, .. } => Some(*item),
        BlockedByAbandoned { target, .. } => Some(*target),
        BudgetOverrun { review, .. } => Some(*review),
        StaleReview { review } => *review,
        Bootstrap { .. } => None,
        // Every other finding has one subject, the judged entry.
        _ => self.subjects().first().copied(),
        }
    }

    // Presentation only: the certificate identifies a finding, and the text
    // names no boundary so that re-emissions merge.
    pub fn description(&self) -> String {
        use PlanningMessage::*;
        match self {
        UnknownKind { kind, .. } => format!(
            "stance kind {kind:?} is not one of committed, deferred, waiting, resumed, sufficient, abandoned"
        ),
        MissingCost { .. } => "a committed stance needs a `cost`".into(),
        MissingHorizon { kind, .. } => format!("a {kind} stance needs a `horizon`"),
        ForbiddenField { kind, field, .. } => format!("a {kind} stance may not carry `{field}`"),
        NegativeCost { cost, .. } => format!("`cost` {cost} is negative"),
        HorizonBelowOne { horizon, .. } => format!("`horizon` {horizon} is below 1"),
        HorizonOverCap { horizon, cap, .. } => {
            format!("deferral `horizon` {horizon} exceeds `max_deferral_horizon` {cap}")
        }
        NegativeBudget { budget, .. } => format!("review `budget` {budget} is negative"),
        NegativeQuota { quota, .. } => format!("`quota` {quota} is negative"),
        RevisedBookkeeping { ty, .. } => format!("{ty} is never revised"),
        CrossesBookkeeping { from, to, .. } => format!(
            "{to} cannot succeed {from}: items and their stances, events, and reviews never replace one another"
        ),
        SuccessorOfTerminal { state, .. } => {
            format!("the predecessor is {} and so final; new work is a new item", state.name())
        }
        SuccessorOfFinalStance { kind, .. } => format!("the {kind} stance it supersedes is final"),
        UnmetRequirement {
            name, requirement, ..
        } => format!("the declaration of {name} must {}", requirement.text()),
        BadEndpoints {
            label,
            from_type,
            to_type,
            ..
        } => format!("planning does not permit {label} from {from_type:?} to {to_type:?}"),
        TerminalTarget { label, state, .. } => {
            format!("{label} edge into an item that is {}", state.name())
        }
        Cycle { component, .. } => format!(
            "Enables and Helps edges form a cycle through nodes [{}]",
            component
                .nodes
                .iter()
                .map(|node| node.position().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        KindDoesNotApply {
            kind, target_type, ..
        } => format!("a {kind} stance cannot be about a {target_type}"),
        UntilFromNonDeferral { kind, .. } => {
            format!("only a deferred stance has an Until edge, not a {kind} one")
        }
        SecondUntil { .. } => "the stance already has a current Until edge".into(),
        SecondDoneAbout { .. } => "a Done has at most one About edge".into(),
        AboutTerminal { state, .. } => format!("About edge to a revision that is {}", state.name()),
        PrematureCompletion { completion, .. } => match completion {
        Completion::Done => "Done before every Enables source of its target is done".into(),
        Completion::Sufficient => {
            "sufficient before every Enables source of the goal is done".into()
        }
        Completion::Abandoned => {
            "abandoned while an Enables source of the item is still open".into()
        }
        },
        UnrelatedRepoint { label, .. } => format!(
            "{label} edge re-points the stance or event to an item that descends from none of its earlier targets"
        ),
        SecondCurrentStance { .. } => "the revision already has a current stance".into(),
        Undecided { .. } => "undecided: this revision needs a decision at this seal".into(),
        BudgetOverrun { cost, budget, .. } => {
            format!("committed cost {cost} exceeds the review's budget {budget}")
        }
        BrokenPlan { .. } => "broken plan: nothing in its strict subgraph is ready or waited on, and no habit in play helps it".into(),
        BlockedByAbandoned { .. } => "an abandoned prerequisite blocks this revision: revise it without the edge, or abandon it".into(),
        Shortfall { dids, quota, .. } => format!("{} of {quota} this window", dids.len()),
        GracePeriod { .. } => {
            "new habit or concern: it is first judged at the next seal".into()
        }
        DroppedConnection { label, .. } => format!(
            "a {label} connection of a revision superseded in this window has no equivalent on a current successor"
        ),
        Bootstrap { name } => {
            format!("planning type {name} is not declared; add its canonical declaration")
        }
        StaleReview { review: None } => "no review has been recorded".into(),
        StaleReview { review: Some(_) } => "the latest review is older than the configured `stale_review_after_days`".into(),
        }
    }
}

impl Requirement {
    fn text(&self) -> String {
        match self {
        Self::PermitsLabel { label, outgoing } => format!(
            "permit {} {label}",
            if *outgoing { "outgoing" } else { "incoming" }
        ),
        Self::NoSuccessors => "permit no successor types".into(),
        Self::Field { field, rule } => match rule {
        FieldRule::MandatoryText => format!("declare `{field}` as mandatory text"),
        FieldRule::MandatoryInteger => format!("declare `{field}` as a mandatory integer"),
        FieldRule::MandatoryTimestamp => format!("declare `{field}` as a mandatory timestamp"),
        FieldRule::OptionalInteger => {
            format!("declare `{field}` as an integer that is not mandatory")
        }
        FieldRule::TimestampIfDeclared => {
            format!("declare `{field}` as a timestamp, or not at all")
        }
        },
        }
    }
}
