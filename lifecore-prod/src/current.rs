//! The current planning graph of one view (PLANNING §4): which revisions are
//! items, what each current stance and `Done` is about, and so each item's
//! state. Everything here depends on the view alone, not on which boundary
//! is judged; the boundary-dependent terms of §6 and §7 are in `situation`.

use crate::{
    payload::{self, PayloadError},
    vocabulary::{Label, PlanningType, StanceKind},
};
use lifecore::{Edge, EntryId, Node, View};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    Open,
    Done,
    Sufficient,
    Abandoned,
}

impl ItemState {
    pub fn is_terminal(self) -> bool {
        self != Self::Open
    }
    pub fn name(self) -> &'static str {
        match self {
        Self::Open => "open",
        Self::Done => "done",
        Self::Sufficient => "sufficient",
        Self::Abandoned => "abandoned",
        }
    }
}

// The fields of a stance that the rules read. An unknown `kind` reads as
// `None`: section 8.1 rejects it, and it decides and finishes nothing.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StanceFields {
    pub kind: Option<StanceKind>,
    pub cost: Option<i64>,
    pub horizon: Option<i64>,
}

impl StanceFields {
    pub fn read(node: &Node) -> Result<Self, PayloadError> {
        Ok(Self {
            kind: StanceKind::from_name(payload::text(node, "kind")?),
            cost: payload::optional_integer(node, "cost")?,
            horizon: payload::optional_integer(node, "horizon")?,
        })
    }
}

pub(crate) fn planning_type(view: View<'_>, id: EntryId) -> Option<PlanningType> {
    PlanningType::from_name(&view.node(id)?.ty_name)
}

pub(crate) struct Current<'a> {
    pub view: View<'a>,
    // Every current revision of an item type.
    items: BTreeMap<EntryId, PlanningType>,
    // Every current stance.
    stances: BTreeMap<EntryId, StanceFields>,
    // Each revision's current stance, the current stance whose current
    // `About` edges include it.
    stance_of: BTreeMap<EntryId, EntryId>,
    // The revisions a current `Done` currently targets.
    done: BTreeSet<EntryId>,
    states: BTreeMap<EntryId, ItemState>,
}

impl<'a> Current<'a> {
    pub fn new(view: View<'a>) -> Result<Self, PayloadError> {
        let mut items = BTreeMap::new();
        let mut stances = BTreeMap::new();
        for node in view.fresh_nodes() {
            match PlanningType::from_name(&node.ty_name) {
            Some(PlanningType::Stance) => {
                stances.insert(node.id, StanceFields::read(node)?);
            }
            Some(ty) if ty.is_item() => {
                items.insert(node.id, ty);
            }
            _ => {}
            }
        }
        let mut stance_of = BTreeMap::new();
        let mut done = BTreeSet::new();
        // Section 8.3 keeps a second current stance about a revision out of
        // committed history. A candidate that has one is rejected anyway, and
        // append order lets the newer stance win there.
        for edge in current_edges(view, Label::About) {
            match planning_type(view, edge.from) {
            Some(PlanningType::Stance) => {
                stance_of.insert(edge.to, edge.from);
            }
            Some(PlanningType::Done) => {
                done.insert(edge.to);
            }
            _ => {}
            }
        }
        let mut current = Self {
            view,
            items,
            stances,
            stance_of,
            done,
            states: BTreeMap::new(),
        };
        current.states = current
            .items
            .keys()
            .map(|&item| (item, current.state_of(item)))
            .collect();
        Ok(current)
    }

    // Abandonment outranks a `Done`, which outranks sufficiency (§4).
    fn state_of(&self, item: EntryId) -> ItemState {
        let kind = self
            .current_stance(item)
            .and_then(|stance| self.kind(stance));
        if kind == Some(StanceKind::Abandoned) {
            ItemState::Abandoned
        } else if self.done.contains(&item) {
            ItemState::Done
        } else if kind == Some(StanceKind::Sufficient) {
            ItemState::Sufficient
        } else {
            ItemState::Open
        }
    }

    pub fn items(&self) -> impl Iterator<Item = (EntryId, PlanningType)> + '_ {
        self.items.iter().map(|(&id, &ty)| (id, ty))
    }
    pub fn item_type(&self, id: EntryId) -> Option<PlanningType> {
        self.items.get(&id).copied()
    }
    pub fn node(&self, id: EntryId) -> &'a Node {
        self.view.node(id).expect("a node of this view")
    }

    // A revision that is not a current item has no state of its own and
    // reads as open, as a superseded revision does at its successor's prefix.
    pub fn state(&self, id: EntryId) -> ItemState {
        self.states.get(&id).copied().unwrap_or(ItemState::Open)
    }
    pub fn is_open(&self, id: EntryId) -> bool {
        self.items.contains_key(&id) && self.state(id) == ItemState::Open
    }

    pub fn stance(&self, id: EntryId) -> Option<StanceFields> {
        self.stances.get(&id).copied()
    }
    pub fn kind(&self, stance: EntryId) -> Option<StanceKind> {
        self.stance(stance)?.kind
    }
    pub fn current_stance(&self, item: EntryId) -> Option<EntryId> {
        self.stance_of.get(&item).copied()
    }
    pub fn stances(&self) -> impl Iterator<Item = (EntryId, StanceFields)> + '_ {
        self.stances.iter().map(|(&id, &fields)| (id, fields))
    }

    // The sources of the current `Enables` edges into `item`.
    pub fn enables_sources(&self, item: EntryId) -> Vec<EntryId> {
        current_edges(self.view, Label::Enables)
            .filter(|edge| edge.to == item)
            .map(|edge| edge.from)
            .collect()
    }

    // What `item` serves (§7 rule 1): the targets of its current `Enables`
    // edges, and of its current `Helps` edges whose target is not terminal
    // (decision 55).
    pub fn serves(&self, item: EntryId) -> BTreeSet<EntryId> {
        let enables = current_edges(self.view, Label::Enables);
        let helps = current_edges(self.view, Label::Helps)
            .filter(|edge| !self.state(edge.to).is_terminal());
        enables
            .chain(helps)
            .filter(|edge| edge.from == item && self.items.contains_key(&edge.to))
            .map(|edge| edge.to)
            .collect()
    }

    // §7 rule 4. Readiness reads only `Enables` sources, so it is the same
    // whether the revision is in play or dormant.
    pub fn is_ready(&self, item: EntryId) -> bool {
        let sources = self.enables_sources(item);
        let all_done = sources
            .iter()
            .all(|&source| self.state(source) == ItemState::Done);
        self.is_open(item)
            && match self.item_type(item) {
            Some(PlanningType::Task | PlanningType::Project) => all_done,
            // A goal with no prerequisite is judged by Broken plan instead.
            Some(PlanningType::Goal) => !sources.is_empty() && all_done,
            _ => false,
            }
    }

    // §7 rule 5: the item and every open revision reached from it by walking
    // `Enables` backward through open revisions.
    pub fn strict_subgraph(&self, item: EntryId) -> BTreeSet<EntryId> {
        let mut members = BTreeSet::from([item]);
        let mut pending = vec![item];
        while let Some(target) = pending.pop() {
            for source in self.enables_sources(target) {
                if self.is_open(source) && members.insert(source) {
                    pending.push(source);
                }
            }
        }
        members
    }

    // The current targets of the current edges with `label` from `source`.
    pub fn targets(&self, source: EntryId, label: Label) -> Vec<EntryId> {
        current_edges(self.view, label)
            .filter(|edge| edge.from == source)
            .map(|edge| edge.to)
            .collect()
    }
}

pub(crate) fn current_edges(view: View<'_>, label: Label) -> impl Iterator<Item = &Edge> {
    view.fresh_edges()
        .filter(move |edge| edge.ty_name == label.name())
}

// The planning root of `id` (§4): walking back through its predecessors, the
// oldest revision of the youngest unbroken run of item types that ends at it.
pub(crate) fn planning_root(view: View<'_>, id: EntryId) -> EntryId {
    run_root(view, id, PlanningType::is_item)
}

// The oldest revision of the youngest unbroken run of `Habit` and `Concern`
// revisions that ends at `id`, which decides newness (§7 rule 6).
pub(crate) fn habit_root(view: View<'_>, id: EntryId) -> EntryId {
    run_root(view, id, |ty| {
        matches!(ty, PlanningType::Habit | PlanningType::Concern)
    })
}

fn run_root(view: View<'_>, id: EntryId, in_run: impl Fn(PlanningType) -> bool) -> EntryId {
    let mut root = id;
    for ancestor in view.ancestors(id) {
        if !PlanningType::from_name(&ancestor.ty_name).is_some_and(&in_run) {
            break;
        }
        root = ancestor.id;
    }
    root
}
