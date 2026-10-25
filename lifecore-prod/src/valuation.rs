//! The valuation (PLANNING §9 and §10): a deterministic ranking of what
//! presses at one boundary. It has no identity, no dismissal, and no
//! persistence, and never affects acceptance; it is a pure function over a
//! bounded view, computed only at that view's boundary (decisions 12, 13,
//! and 63).

use crate::{
    current::{ItemState, current_edges, planning_root},
    payload::{self, PayloadError},
    situation::Situation,
    vocabulary::{Label, PlanningType, StanceKind},
    windows::Reviews,
};
use lifecore::{EntryId, Timestamp, View, ViewId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

// Not analyzer configuration: it affects no finding, so it is no part of
// any finding's identity (decision 62).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValuationOptions {
    // The deadline estimate's sample: the last this many pairs of
    // consecutive reviews whose `at` strictly increases.
    pub deadline_sample_windows: usize,
}

impl Default for ValuationOptions {
    fn default() -> Self {
        Self {
            deadline_sample_windows: 6,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Valuation {
    pub boundary: ViewId,
    pub ranked: Vec<Ranked>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ranked {
    pub revision: EntryId,
    pub tier: u8,
    pub reasons: Vec<RenderedReason>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderedReason {
    pub reason: Reason,
    pub text: String,
}

// Tests assert the structured reason; clients show the text. Counts of
// seals and events, bounded by the history's length, travel as JSON
// numbers. What a payload integer bounds, `quota` and `seals_left`, which
// grows with a stance's `horizon`, and the deadline estimate travel as
// decimal strings, so JavaScript clients never round them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reason {
    Undecided,
    BrokenPlan,
    OpenFor {
        seals: usize,
    },
    Standing {
        kind: StanceKind,
        #[serde(with = "lifecore::decimal")]
        seals_left: usize,
    },
    LapsesAtNextSeal,
    DeadlineIn {
        #[serde(with = "lifecore::decimal")]
        reviews: u64,
    },
    DeadlinePassed,
    Starved {
        seals: usize,
    },
    Shortfall {
        have: usize,
        #[serde(with = "lifecore::decimal")]
        quota: i64,
    },
    New,
}

impl Reason {
    pub fn text(self) -> String {
        let seals = |n: usize| {
            if n == 1 {
                "1 seal".into()
            } else {
                format!("{n} seals")
            }
        };
        match self {
        Self::Undecided => "undecided: the next seal needs a decision".into(),
        Self::BrokenPlan => "broken plan: nothing in its strict subgraph can advance it".into(),
        Self::OpenFor { seals: n } => format!("open for {}", seals(n)),
        Self::Standing { kind, seals_left } => {
            format!("{kind}, deciding the next {}", seals(seals_left))
        }
        Self::LapsesAtNextSeal => "lapses at the next seal".into(),
        Self::DeadlineIn { reviews: 1 } => "deadline in 1 review".into(),
        Self::DeadlineIn { reviews } => format!("deadline in {reviews} reviews"),
        Self::DeadlinePassed => "deadline passed".into(),
        Self::Starved { seals: n } => format!("no commitment toward it sealed for {}", seals(n)),
        Self::Shortfall { have, quota } => format!("{have} of {quota} so far this window"),
        Self::New => "new: the next seal gives it a grace period".into(),
        }
    }
}

// Ranks the revisions in play at the boundary of `view`, and in tier 3 the
// open revisions whose own current stance is a standing deferral, with the
// terms of §7 judged in the open window. A payload that does not fit the
// canonical declarations is an error, as it is for the analyzer.
pub fn valuation(view: View<'_>, options: &ValuationOptions) -> Result<Valuation, PayloadError> {
    let situation = Situation::in_open_window(view)?;
    let ranking = Ranking {
        estimate: DeadlineEstimate::new(&situation.reviews, options.deadline_sample_windows)?,
        undecided: situation.undecided().into_iter().collect(),
        broken: situation.broken_plans().into_iter().collect(),
        situation: &situation,
    };
    let mut rows = Vec::new();
    for (item, ty) in situation.current.items() {
        if let Some(row) = ranking.rank(item, ty)? {
            rows.push(row);
        }
    }
    rows.sort_by_key(|row| row.order);
    Ok(Valuation {
        boundary: view.id(),
        ranked: rows.into_iter().map(|row| row.ranked).collect(),
    })
}

struct Ranking<'s, 'a> {
    situation: &'s Situation<'a>,
    estimate: Option<DeadlineEstimate>,
    undecided: BTreeSet<EntryId>,
    broken: BTreeSet<EntryId>,
}

struct Row {
    // Tier, the tier's two keys with descending ones negated, planning-root
    // position, then the revision's own position, which orders split
    // siblings that share a root (decision 70).
    order: (u8, [i64; 2], usize, usize),
    ranked: Ranked,
}

// Counts used as descending keys.
fn descending(count: usize) -> i64 {
    -i64::try_from(count).unwrap_or(i64::MAX)
}

impl Ranking<'_, '_> {
    // The first tier whose members include `item`, if any.
    fn rank(&self, item: EntryId, ty: PlanningType) -> Result<Option<Row>, PayloadError> {
        use PlanningType::*;
        let s = self.situation;
        let current = &s.current;
        let root = planning_root(s.view(), item);
        let open_for = s.reviews.count() - s.reviews.window_of(root.position());
        let standing = current
            .current_stance(item)
            .filter(|&stance| s.is_standing(stance));
        let standing_kind = standing.and_then(|stance| current.kind(stance));
        let in_play = s.in_play(item);
        let row = |tier: u8, keys: [i64; 2], reasons: Vec<Reason>| Row {
            order: (tier, keys, root.position(), item.position()),
            ranked: Ranked {
                revision: item,
                tier,
                reasons: reasons
                    .into_iter()
                    .map(|reason| RenderedReason {
                        reason,
                        text: reason.text(),
                    })
                    .collect(),
            },
        };

        // Tier 0: what the next seal would reject as it stands, goals and
        // concerns first, then by fan-out.
        let sinks_first = i64::from(!matches!(ty, Goal | Concern));
        if in_play && self.undecided.contains(&item) {
            let keys = [sinks_first, descending(self.fan_out(item))];
            return Ok(Some(row(0, keys, vec![Reason::Undecided])));
        }
        if in_play && self.broken.contains(&item) {
            let keys = [sinks_first, descending(self.fan_out(item))];
            return Ok(Some(row(0, keys, vec![Reason::BrokenPlan])));
        }

        // Tier 1: standing commitments, oldest items first, then nearest
        // deadline. A commitment always lapses at the next seal, so it
        // carries no `Standing` reason.
        if in_play && ty == Task && standing_kind == Some(StanceKind::Committed) {
            let mut reasons = vec![Reason::OpenFor { seals: open_for }];
            let deadline = self.deadline(item)?;
            reasons.extend(deadline);
            let deadline_key = match deadline {
            Some(Reason::DeadlinePassed) => i64::MIN,
            Some(Reason::DeadlineIn { reviews }) => i64::try_from(reviews).unwrap_or(i64::MAX),
            _ => i64::MAX,
            };
            return Ok(Some(row(1, [descending(open_for), deadline_key], reasons)));
        }

        // Tier 2: habits and concerns below quota so far in this window. One
        // with no `Did` yet is undecided and ranked in tier 0.
        if in_play && matches!(ty, Habit | Concern) && !s.is_new(item) {
            let have = s.window_dids(item).len();
            let quota = payload::integer(current.node(item), "quota")?;
            let short = quota.saturating_sub(i64::try_from(have).unwrap_or(i64::MAX));
            if short > 0 {
                return Ok(Some(row(
                    2,
                    [-short, 0],
                    vec![Reason::Shortfall { have, quota }],
                )));
            }
        }

        // Tier 3: standing deferrals and waits, those the next seal is the
        // last to decide first, then oldest items first. This is the only
        // tier that ranks a dormant revision: one dormant by its own
        // deferral, not the revisions dormant only through it (decision 51).
        if current.state(item) == ItemState::Open
            && let Some(kind @ (StanceKind::Deferred | StanceKind::Waiting)) = standing_kind
        {
            let seals_left = s.seals_left(standing.expect("a standing stance"));
            let mut reasons = vec![Reason::Standing { kind, seals_left }];
            if seals_left == 1 {
                reasons.push(Reason::LapsesAtNextSeal);
            }
            reasons.push(Reason::OpenFor { seals: open_for });
            let keys = [i64::from(seals_left != 1), descending(open_for)];
            return Ok(Some(row(3, keys, reasons)));
        }

        if !in_play {
            return Ok(None);
        }

        // Tier 4: goals and projects in play, with a sealed planning root,
        // with no standing commitment in their strict subgraph, most starved
        // first.
        let root_sealed = s.reviews.sealing_review(root.position()).is_some();
        if matches!(ty, Goal | Project) && root_sealed && !self.has_standing_commitment(item) {
            let seals = self.starved(item, root)?;
            return Ok(Some(row(
                4,
                [descending(seals), 0],
                vec![Reason::Starved { seals }],
            )));
        }

        // Tier 5: the rest in play, in planning-root order.
        let reasons = if s.is_new(item) {
            vec![Reason::New]
        } else {
            vec![]
        };
        Ok(Some(row(5, [0, 0], reasons)))
    }

    // The distinct goals in play reachable forward over current `Enables`
    // and `Helps` edges.
    fn fan_out(&self, item: EntryId) -> usize {
        let s = self.situation;
        reachable(s.view(), item, &[Label::Enables, Label::Helps])
            .into_iter()
            .filter(|&reached| {
                s.current.item_type(reached) == Some(PlanningType::Goal) && s.in_play(reached)
            })
            .count()
    }

    fn has_standing_commitment(&self, item: EntryId) -> bool {
        let s = self.situation;
        s.current.strict_subgraph(item).into_iter().any(|member| {
            s.current.item_type(member) == Some(PlanningType::Task)
                && s.current.current_stance(member).is_some_and(|stance| {
                    s.is_standing(stance) && s.current.kind(stance) == Some(StanceKind::Committed)
                })
        })
    }

    // Seals since the latest review that sealed a `committed` stance, current
    // or superseded, with a current or historical `About` edge to a task in
    // the item's strict subgraph or to a done task its walk reaches; with
    // none, since the sealing review of the item's planning root (decisions
    // 52, 67, and 71).
    fn starved(&self, item: EntryId, root: EntryId) -> Result<usize, PayloadError> {
        let s = self.situation;
        let current = &s.current;
        let is_task = |id: EntryId| current.item_type(id) == Some(PlanningType::Task);
        let plan = current.strict_subgraph(item);
        let mut tasks = plan
            .iter()
            .copied()
            .filter(|&member| is_task(member))
            .collect::<BTreeSet<_>>();
        for &member in &plan {
            tasks.extend(
                current
                    .enables_sources(member)
                    .into_iter()
                    .filter(|&source| is_task(source) && current.state(source) == ItemState::Done),
            );
        }
        let mut latest = None;
        for edge in s.view().edges() {
            let stance = current.node(edge.from);
            if edge.ty_name != Label::About.name()
                || !tasks.contains(&edge.to)
                || stance.ty_name != PlanningType::Stance.name()
                || payload::text(stance, "kind")? != StanceKind::Committed.name()
            {
                continue;
            }
            latest = latest.max(s.reviews.sealing_review(stance.id.position()));
        }
        let from = latest
            .or_else(|| s.reviews.sealing_review(root.position()))
            .expect("tier 4 ranks only items with a sealed planning root");
        Ok(s.reviews.count() - from)
    }

    // A task's deadline is the nearest `deadline` over the goals and
    // projects in play reachable forward over `Enables`; a dormant or
    // terminal one sets none, since a deferral means "not now" (decision
    // 68).
    fn deadline(&self, task: EntryId) -> Result<Option<Reason>, PayloadError> {
        let s = self.situation;
        let Some(estimate) = &self.estimate else {
            return Ok(None);
        };
        let mut nearest = None::<Timestamp>;
        for reached in reachable(s.view(), task, &[Label::Enables]) {
            if !matches!(
                s.current.item_type(reached),
                Some(PlanningType::Goal | PlanningType::Project)
            ) || !s.in_play(reached)
            {
                continue;
            }
            if let Some(deadline) =
                payload::optional_timestamp(s.current.node(reached), "deadline")?
            {
                nearest = Some(nearest.map_or(deadline, |nearest| nearest.min(deadline)));
            }
        }
        Ok(nearest.map(|deadline| estimate.reason(deadline)))
    }
}

// The nodes reachable from `start`, excluding it, over current edges with
// one of `labels`.
fn reachable(view: View<'_>, start: EntryId, labels: &[Label]) -> BTreeSet<EntryId> {
    let mut reached = BTreeSet::new();
    let mut pending = vec![start];
    while let Some(node) = pending.pop() {
        for &label in labels {
            for edge in current_edges(view, label).filter(|edge| edge.from == node) {
                if edge.to != start && reached.insert(edge.to) {
                    pending.push(edge.to);
                }
            }
        }
    }
    reached
}

// `DeadlineIn` is the time from the latest review's `at` to the deadline,
// divided by the median window length of a sample, rounded down (§10).
struct DeadlineEstimate {
    latest: Timestamp,
    median_nanos: i128,
}

impl DeadlineEstimate {
    // The sample is the last `sample_windows` pairs of consecutive reviews
    // whose `at` strictly increases. With fewer than two reviews or no such
    // pair there is no estimate, and no deadline reason.
    fn new(reviews: &Reviews<'_>, sample_windows: usize) -> Result<Option<Self>, PayloadError> {
        let ats = reviews
            .all()
            .map(|review| payload::timestamp(review, "at"))
            .collect::<Result<Vec<_>, _>>()?;
        let lengths = ats
            .windows(2)
            .filter(|pair| pair[1] > pair[0])
            .map(|pair| pair[1].duration_since(pair[0]).as_nanos())
            .collect::<Vec<_>>();
        let mut sample = lengths[lengths.len().saturating_sub(sample_windows)..].to_vec();
        let (Some(&latest), false) = (ats.last(), sample.is_empty()) else {
            return Ok(None);
        };
        sample.sort_unstable();
        let middle = sample.len() / 2;
        let median_nanos = if sample.len() % 2 == 1 {
            sample[middle]
        } else {
            (sample[middle - 1] + sample[middle]) / 2
        };
        Ok(Some(Self {
            latest,
            median_nanos,
        }))
    }

    fn reason(&self, deadline: Timestamp) -> Reason {
        let remaining = deadline.duration_since(self.latest).as_nanos();
        if remaining < 0 {
            Reason::DeadlinePassed
        } else {
            Reason::DeadlineIn {
                reviews: u64::try_from(remaining / self.median_nanos).unwrap_or(u64::MAX),
            }
        }
    }
}
