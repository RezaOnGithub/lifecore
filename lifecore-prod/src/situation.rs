//! The terms of PLANNING §6 and §7 at one boundary: which stances are
//! decisions, which open revisions are dormant or in play, which habits and
//! concerns are new, and the frontier and what decides it. Seals (§8.4 and
//! §8.5) and the valuation (§9) both read them.

use crate::{
    current::{Current, current_edges, habit_root},
    payload::PayloadError,
    vocabulary::{Label, PlanningType, StanceKind},
    windows::{Boundary, Reviews},
};
use lifecore::{EntryId, View};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Situation<'a> {
    pub current: Current<'a>,
    pub reviews: Reviews<'a>,
    pub boundary: Boundary,
    // The current stances that are decisions, fresh or standing, here.
    decisions: BTreeSet<EntryId>,
    // The open revisions that are dormant here.
    dormant: BTreeSet<EntryId>,
}

impl<'a> Situation<'a> {
    // The seal at a prefix whose newest entry is a review.
    pub fn at_seal(view: View<'a>) -> Result<Self, PayloadError> {
        let reviews = Reviews::new(view);
        let boundary = Boundary::Seal(reviews.count());
        Self::new(view, reviews, boundary)
    }

    // The open window after the view's latest review, as the valuation reads
    // any boundary.
    pub fn in_open_window(view: View<'a>) -> Result<Self, PayloadError> {
        let reviews = Reviews::new(view);
        let boundary = Boundary::Open(reviews.count());
        Self::new(view, reviews, boundary)
    }

    fn new(view: View<'a>, reviews: Reviews<'a>, boundary: Boundary) -> Result<Self, PayloadError> {
        let mut situation = Self {
            current: Current::new(view)?,
            reviews,
            boundary,
            decisions: BTreeSet::new(),
            dormant: BTreeSet::new(),
        };
        situation.decisions = situation
            .current
            .stances()
            .map(|(stance, _)| stance)
            .filter(|&stance| situation.lifetime_holds(stance))
            .collect();
        situation.dormant = situation.dormancy();
        Ok(situation)
    }

    pub fn view(&self) -> View<'a> {
        self.current.view
    }

    // Appended in the window being sealed, or in the open window (§6).
    pub fn is_fresh(&self, id: EntryId) -> bool {
        self.reviews.window_of(id.position()) == self.boundary.window()
    }

    // Whether the current stance `stance` is fresh or standing here (§6).
    fn lifetime_holds(&self, stance: EntryId) -> bool {
        let fields = self.current.stance(stance).expect("a current stance");
        let sealed_by = self.reviews.sealing_review(stance.position());
        match fields.kind {
        Some(StanceKind::Committed) => match (self.boundary, sealed_by) {
        // Appended in the open window: the plan for the window the next
        // seal opens.
        (_, None) => true,
        // The plan for the window its sealing review opens: fresh at that
        // seal, standing through that window, and lapsed at the next seal.
        (Boundary::Seal(seal), Some(review)) => review == seal,
        (Boundary::Open(window), Some(review)) => review == window,
        },
        Some(StanceKind::Deferred | StanceKind::Waiting) => {
            if self.until_target_terminal(stance) {
                return false;
            }
            let Some(review) = sealed_by else {
                return true;
            };
            let last = self.last_seal(stance, review, fields.horizon);
            match self.boundary {
            // It stands through its last seal...
            Boundary::Seal(seal) => seal <= last,
            // ...and lapses after it, so it decides nothing in the window
            // that seal opens.
            Boundary::Open(window) => window < last,
            }
        }
        // `resumed` is never a decision, and `sufficient` and `abandoned`
        // make the item terminal instead.
        _ => false,
        }
    }

    // A deferral lapses early at any boundary where its `Until` target is
    // terminal.
    fn until_target_terminal(&self, stance: EntryId) -> bool {
        self.current
            .targets(stance, Label::Until)
            .iter()
            .any(|&target| self.current.state(target).is_terminal())
    }

    // The last seal a `deferred` or `waiting` stance sealed by Review `review`
    // decides: the `horizon`-th seal after that review, or an earlier seal
    // that finds the stance with `Until` edges but no current one, since its
    // target was revised and the edge not re-pointed (decision 73). Only a
    // seal can end a deferral that way, so re-pointing after the target's
    // revision keeps the deferral only when it happens before the next seal.
    // Section 8.1 rejects a missing or nonpositive horizon; it reads as 0.
    fn last_seal(&self, stance: EntryId, review: usize, horizon: Option<i64>) -> usize {
        let horizon = horizon
            .and_then(|horizon| usize::try_from(horizon).ok())
            .unwrap_or(0);
        let by_horizon = review.saturating_add(horizon);
        if !has_until(self.view(), stance) {
            return by_horizon;
        }
        (review..=by_horizon.min(self.reviews.count()))
            .find(|&seal| lost_until(self.view_at_seal(seal), stance))
            .unwrap_or(by_horizon)
    }

    // The seals, counting the next one, at which the standing `deferred` or
    // `waiting` stance still decides, as the graph stands (§9). A deferral
    // whose `Until` edges are all historical here stands through the next
    // seal and no further, unless one is re-pointed before it (decision 82).
    pub fn seals_left(&self, stance: EntryId) -> usize {
        let fields = self.current.stance(stance).expect("a current stance");
        let review = self
            .reviews
            .sealing_review(stance.position())
            .expect("a standing stance is sealed");
        let mut last = self.last_seal(stance, review, fields.horizon);
        if lost_until(self.view(), stance) {
            last = last.min(self.reviews.count() + 1);
        }
        last - self.reviews.count()
    }

    // The prefix whose newest entry is Review `seal`.
    fn view_at_seal(&self, seal: usize) -> View<'a> {
        let position = self.reviews.review(seal).id.position();
        self.view()
            .prefix(position + 1)
            .expect("a review of this view")
    }

    // §7 rule 2, computed as the least fixed point: dormancy spreads from
    // every open revision whose own current stance is a `deferred` decision,
    // backward to every open revision that serves at least one revision and
    // only dormant ones. A `committed` or `waiting` decision keeps its
    // revision in play whatever it serves. In the acyclic graph section 3.2
    // requires, the fixed point is the rule's only solution; a cycle, which
    // section 8.2 rejects, leaves its undeferred members in play.
    fn dormancy(&self) -> BTreeSet<EntryId> {
        let open = self
            .current
            .items()
            .map(|(item, _)| item)
            .filter(|&item| self.current.is_open(item))
            .map(|item| (item, self.current.serves(item)))
            .collect::<BTreeMap<_, _>>();
        let mut dormant = BTreeSet::new();
        loop {
            let mut changed = false;
            for (&item, serves) in &open {
                let decision = self.decision(item);
                if dormant.contains(&item)
                    || matches!(decision, Some(StanceKind::Committed | StanceKind::Waiting))
                {
                    continue;
                }
                let deferred = decision == Some(StanceKind::Deferred);
                let serves_only_dormant =
                    !serves.is_empty() && serves.iter().all(|target| dormant.contains(target));
                if deferred || serves_only_dormant {
                    dormant.insert(item);
                    changed = true;
                }
            }
            if !changed {
                return dormant;
            }
        }
    }

    // The kind of `item`'s current stance, if that stance is a decision.
    pub fn decision(&self, item: EntryId) -> Option<StanceKind> {
        let stance = self.current.current_stance(item)?;
        if self.decisions.contains(&stance) {
            self.current.kind(stance)
        } else {
            None
        }
    }

    // Whether the stance is a decision appended before this window.
    pub fn is_standing(&self, stance: EntryId) -> bool {
        self.decisions.contains(&stance) && !self.is_fresh(stance)
    }

    pub fn is_dormant(&self, item: EntryId) -> bool {
        self.dormant.contains(&item)
    }

    pub fn in_play(&self, item: EntryId) -> bool {
        self.current.is_open(item) && !self.is_dormant(item)
    }

    // §7 rule 6: newness counts from when the item became a habit or
    // concern, so revising it does not make it new again.
    pub fn is_new(&self, item: EntryId) -> bool {
        matches!(
            self.current.item_type(item),
            Some(PlanningType::Habit | PlanningType::Concern)
        ) && self.is_fresh(habit_root(self.view(), item))
    }

    // The `Did` events of this window that currently target `item`.
    pub fn window_dids(&self, item: EntryId) -> Vec<EntryId> {
        current_edges(self.view(), Label::About)
            .filter(|edge| {
                edge.to == item
                    && self.current.node(edge.from).ty_name == PlanningType::Did.name()
                    && self.is_fresh(edge.from)
            })
            .map(|edge| edge.from)
            .collect()
    }

    // §7 rule 7: the revisions that need a decision.
    pub fn frontier(&self) -> Vec<EntryId> {
        use PlanningType::*;
        self.current
            .items()
            .filter(|&(item, ty)| {
                self.in_play(item)
                    && match ty {
                    Task | Project | Goal => self.current.is_ready(item),
                    External => true,
                    Habit | Concern => !self.is_new(item),
                    Stance | Done | Did | Review => false,
                    }
            })
            .map(|(item, _)| item)
            .collect()
    }

    // §7 rule 8. A ready goal or project is decided only by leaving the
    // frontier, so it is never decided while in it.
    pub fn is_decided(&self, item: EntryId) -> bool {
        match self.current.item_type(item) {
        Some(PlanningType::Task) => self.decision(item) == Some(StanceKind::Committed),
        Some(PlanningType::External) => self.decision(item) == Some(StanceKind::Waiting),
        Some(PlanningType::Habit | PlanningType::Concern) => !self.window_dids(item).is_empty(),
        _ => false,
        }
    }

    pub fn undecided(&self) -> Vec<EntryId> {
        self.frontier()
            .into_iter()
            .filter(|&item| !self.is_decided(item))
            .collect()
    }

    // The goals and projects in play whose plan is broken (§8.4): their
    // strict subgraph holds no ready item and no open external with a
    // `waiting` decision, and no habit in play helps into it. No other
    // helping work rescues a plan (decision 49).
    pub fn broken_plans(&self) -> Vec<EntryId> {
        self.current
            .items()
            .filter(|&(item, ty)| {
                matches!(ty, PlanningType::Goal | PlanningType::Project)
                    && self.in_play(item)
                    && !self.plan_can_advance(item)
            })
            .map(|(item, _)| item)
            .collect()
    }

    fn plan_can_advance(&self, item: EntryId) -> bool {
        let plan = self.current.strict_subgraph(item);
        let ready = plan.iter().any(|&member| self.current.is_ready(member));
        let waited_on = plan.iter().any(|&member| {
            self.current.item_type(member) == Some(PlanningType::External)
                && self.decision(member) == Some(StanceKind::Waiting)
        });
        let helped = current_edges(self.view(), Label::Helps).any(|edge| {
            plan.contains(&edge.to)
                && self.current.item_type(edge.from) == Some(PlanningType::Habit)
                && self.in_play(edge.from)
        });
        ready || waited_on || helped
    }
}

fn untils<'v>(view: View<'v>, stance: EntryId) -> impl Iterator<Item = &'v lifecore::Edge> {
    view.edges()
        .filter(move |edge| edge.from == stance && edge.ty_name == Label::Until.name())
}

fn has_until(view: View<'_>, stance: EntryId) -> bool {
    untils(view, stance).next().is_some()
}

// Whether `stance` has `Until` edges in `view` and none of them is current.
fn lost_until(view: View<'_>, stance: EntryId) -> bool {
    has_until(view, stance) && untils(view, stance).all(|edge| view.is_stale(edge.id) == Some(true))
}
