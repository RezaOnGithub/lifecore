//! Deterministic planning-shaped histories, for the checking benchmark in
//! `lifecore-app` (PERFORMANCE §1.1) and for tests that run analyzers over the
//! same histories.
//!
//! The shape is the one PERFORMANCE §1.2 measured by hand at `59c702c`: the ten
//! planning declarations of PLANNING §11 at that commit, written here as plain
//! `Type` nodes with today's value kinds, then windows that each end in a
//! `Review`. Nothing here implements planning; the vocabulary only gives the
//! history its size and shape.

use lifecore::{
    Body, Edge, Entry, EntryId, LifeCoreList, Node, Payload, Timestamp,
    analysis::{Config, SuiteConfig},
    typing::{FieldDecl, FieldStatus, Type, ValueType},
};
use serde_json::json;
use std::collections::HashSet;
use uuid::Uuid;

/// The analyzer names of the default suite, TypeCheck then Reconnect, each
/// with configuration `{}`.
const DEFAULT_ANALYZERS: [&str; 2] = ["Type_Check", "Reconnect"];

/// One review per window of seven entries, the density PERFORMANCE §1.2
/// measured: every task is reviewed while it is committed, and its `Done`
/// opens the next window.
pub const REVIEW_EVERY_TASK: usize = 7;
/// One review per 1,000 entries: a weekly review of a database growing by
/// 1,000 entries a week (PERFORMANCE §1.1).
pub const REVIEW_WEEKLY: usize = 1_000;
/// The declarations that open every history.
pub const DECLARATIONS: usize = 10;
/// The shortest window: its review then follows every two task entries.
pub const MIN_REVIEW_EVERY: usize = 3;

// Fixed identities, so equal shapes give equal histories.
const DATABASE: Uuid = Uuid::from_u128(0x6c69_6665_636f_7265_0000_0000_0000_0001);
const ORIGINS: u128 = 0x6c69_6665_636f_7265_0001_0000_0000_0000;
// Monday 2026-01-05T00:00:00Z; every entry is ten minutes after the one
// before it, about 1,000 entries a week.
const START_SECOND: i64 = 1_767_571_200;
const ENTRY_SECONDS: i64 = 600;

/// The size and seal density of a generated history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shape {
    /// Total entries, the declarations included.
    pub entries: usize,
    /// Entries per window, its closing review included. The first window is
    /// two entries shorter, lacking the `Done` of a task before it; the last
    /// is cut at `entries` and may lack its review.
    pub review_every: usize,
}

impl Shape {
    /// The number of `Review` nodes in the history.
    pub fn reviews(self) -> usize {
        // Reviews sit at offsets `review_every - 3` modulo `review_every`
        // after the declarations (see `history`).
        (self.entries.saturating_sub(DECLARATIONS) + 2) / self.review_every
    }
}

/// The suite `lifecore init --default-analyzers` stores, which the benchmark
/// checks under. This crate depends on no analyzer, so the names are spelled
/// here; a `lifecore-cli` test compares this suite with the one `init`
/// stores, and this crate's tests run it with the registered factories.
pub fn default_suite() -> SuiteConfig {
    SuiteConfig(
        DEFAULT_ANALYZERS
            .map(|name| Config {
                name: name.into(),
                configuration: "{}".into(),
            })
            .into(),
    )
}

/// The ten planning declarations of PLANNING §11 at `59c702c`, in the order
/// every history declares them.
pub fn declarations() -> [Type; DECLARATIONS] {
    use lifecore::typing::{
        FieldStatus::{Mandatory, Optional},
        ValueType::{Integer, Text, Timestamp},
    };
    let title = ("title", (Text, Mandatory));
    let item = ["Enables", "Helps"];
    [
        Type::new(
            "Goal",
            &[
                title.clone(),
                ("criterion", (Text, Optional)),
                ("deadline", (Timestamp, Optional)),
            ],
            &[],
            &["Enables", "Helps", "About"],
            &[],
        ),
        Type::new(
            "Concern",
            &[title.clone(), ("quota", (Integer, Mandatory))],
            &[],
            &["Helps", "About"],
            &[],
        ),
        Type::new(
            "Project",
            &[
                title.clone(),
                ("deliverable", (Text, Optional)),
                ("deadline", (Timestamp, Optional)),
            ],
            &item,
            &["Enables", "Helps", "About", "Until"],
            &[],
        ),
        Type::new(
            "Task",
            &[title.clone(), ("note", (Text, Optional))],
            &item,
            &["Enables", "Helps", "About", "Until"],
            &[],
        ),
        Type::new(
            "Habit",
            &[title.clone(), ("quota", (Integer, Mandatory))],
            &["Helps"],
            &["About"],
            &[],
        ),
        Type::new("External", &[title], &item, &["About", "Until"], &[]),
        Type::new(
            "Stance",
            &[
                ("kind", (Text, Mandatory)),
                ("at", (Timestamp, Mandatory)),
                ("cost", (Integer, Optional)),
                ("horizon", (Integer, Optional)),
                ("reason", (Text, Optional)),
            ],
            &["About", "Until"],
            &[],
            &[],
        ),
        Type::new(
            "Done",
            &[("at", (Timestamp, Mandatory)), ("note", (Text, Optional))],
            &["About"],
            &[],
            &[],
        ),
        Type::new(
            "Did",
            &[("at", (Timestamp, Mandatory)), ("note", (Text, Optional))],
            &["About"],
            &[],
            &[],
        ),
        Type::new(
            "Review",
            &[
                ("budget", (Integer, Mandatory)),
                ("at", (Timestamp, Mandatory)),
                ("note", (Text, Optional)),
            ],
            &[],
            &[],
            &[],
        ),
    ]
}

/// A planning-shaped history of exactly `shape.entries` entries under
/// `suite`, which is stored but not run. It passes structural validation by
/// construction and raises no finding under TypeCheck or Reconnect.
///
/// After the declarations comes a stream of tasks, six entries each: the
/// task, its revision, a `committed` stance with its `About` edge to the
/// revision, and a `Done` with its `About` edge to the revision. A review
/// closes each window, so at `REVIEW_EVERY_TASK` every window but the first
/// is a `Done` and its edge, the next task's first four entries, and the
/// review. Each window has its own origin, as if applied as one proposal.
///
/// Every entry depends only on its position and `review_every`, so a shorter
/// history of the same density is a prefix of a longer one.
///
/// # Panics
///
/// If `shape.entries < DECLARATIONS` or `shape.review_every <
/// MIN_REVIEW_EVERY`.
pub fn history(shape: Shape, suite: SuiteConfig) -> LifeCoreList {
    generate(shape, None, suite)
}

/// The history of `shape` with variation an index could get wrong, for the
/// tests that compare indexed and unindexed runs (SPEC §9, "Indexes"):
/// revised and duplicated `Type` declarations, some malformed; revision
/// chains and splits; `Enables`, `Helps`, `Until`, and `About` edges between
/// items, left behind with stale endpoints when an item is revised, and
/// sometimes carried over; superseded roots appended again with the same
/// content; and nodes and edges that raise TypeCheck, Reconnect, and DAG
/// Check findings.
///
/// The task stream of [`history`] runs underneath, and after each of its
/// entries a variation event of a few entries follows with probability one
/// half, chosen by `seed`. Equal seeds and shapes give equal histories, and a
/// shorter history of the same seed and density is a prefix of a longer
/// one. Reviews close windows by their position in the task stream, so
/// [`Shape::reviews`] does not count them here. The history passes
/// structural validation, which is all its findings may not break.
///
/// # Panics
///
/// As [`history`].
pub fn varied_history(shape: Shape, seed: u64, suite: SuiteConfig) -> LifeCoreList {
    generate(shape, Some(seed), suite)
}

fn generate(shape: Shape, seed: Option<u64>, suite: SuiteConfig) -> LifeCoreList {
    assert!(
        shape.entries >= DECLARATIONS,
        "a history holds at least the {DECLARATIONS} declarations"
    );
    assert!(
        shape.review_every >= MIN_REVIEW_EVERY,
        "a window holds at least {MIN_REVIEW_EVERY} entries"
    );
    let mut builder = Builder {
        entries: Vec::with_capacity(shape.entries),
        window: 0,
        edges: HashSet::new(),
    };
    let declared = declarations()
        .map(|declaration| builder.node(Type::TYPE_NAME, None, declaration.to_body()));
    builder.window = 1;
    let mut tasks = Tasks::default();
    let mut variation = seed.map(|seed| Variation::new(seed, declared));
    let mut offset = 0;
    while builder.entries.len() < shape.entries {
        // A window closes at every offset `review_every - 3` modulo
        // `review_every`, so the first holds two entries fewer than the rest:
        // at `REVIEW_EVERY_TASK` each review then falls between a task's
        // commitment and its `Done`. `Shape::reviews` counts these offsets.
        if offset % shape.review_every == shape.review_every - 3 {
            let at = builder.at();
            builder.node(
                "Review",
                None,
                body([("budget", Payload::Integer(40)), ("at", at)]),
            );
            builder.window += 1;
        } else if let Some(finished) = tasks.next(&mut builder)
            && let Some(variation) = &mut variation
        {
            variation.finished(&builder, finished);
        }
        offset += 1;
        if let Some(variation) = &mut variation
            && variation.rng.one_in(2)
        {
            variation.event(&mut builder);
        }
    }
    // An event may run past the end; every prefix of a valid history is
    // valid.
    builder.entries.truncate(shape.entries);
    serde_json::from_value(json!({
        "id": DATABASE,
        "entries": builder.entries,
        "suite": suite,
    }))
    .expect("generated history passes structural validation")
}

struct Builder {
    entries: Vec<Entry>,
    window: u128,
    // Every edge triple appended, so variation never duplicates one.
    edges: HashSet<(EntryId, EntryId, String)>,
}

impl Builder {
    // `EntryId` has no public constructor; its serialized form is the import
    // boundary, as for a loaded snapshot.
    fn next_id(&self) -> EntryId {
        serde_json::from_value(json!({
            "position": self.entries.len().to_string(),
            "origin": Uuid::from_u128(ORIGINS + self.window),
        }))
        .expect("canonical entry identity")
    }
    // The timestamp of the next entry. Positions are unique, so timestamped
    // payloads never duplicate one another.
    fn at(&self) -> Payload {
        let position = i64::try_from(self.entries.len()).expect("position fits i64");
        Payload::Timestamp(
            Timestamp::from_second(START_SECOND + ENTRY_SECONDS * position)
                .expect("timestamp in range"),
        )
    }
    fn node(&mut self, ty_name: &str, predecessor: Option<EntryId>, payload: Body) -> EntryId {
        let id = self.next_id();
        self.entries.push(Entry::Node(Node {
            id,
            ty_name: ty_name.into(),
            predecessor,
            payload,
        }));
        id
    }
    fn edge(&mut self, ty_name: &str, from: EntryId, to: EntryId) {
        self.edges.insert((from, to, ty_name.into()));
        let id = self.next_id();
        self.entries.push(Entry::Edge(Edge {
            id,
            ty_name: ty_name.into(),
            from,
            to,
        }));
    }
}

// The task stream, one entry per call. Both `About` edges target the
// revision, which stays current, so no edge has a stale endpoint and no
// revision leaves an edge behind for Reconnect.
#[derive(Default)]
struct Tasks {
    number: usize,
    step: usize,
    task: Option<EntryId>,
    revision: Option<EntryId>,
    about: Option<EntryId>,
}

impl Tasks {
    const STEPS: usize = 6;

    // Returns the task's root and revision once its last entry is appended.
    fn next(&mut self, builder: &mut Builder) -> Option<(EntryId, EntryId)> {
        if self.step == 0 {
            self.number += 1;
        }
        let title = || Payload::Text(format!("Task {}", self.number));
        match self.step {
        0 => {
            self.task = Some(builder.node("Task", None, body([("title", title())])));
        }
        1 => {
            let payload = body([
                ("title", title()),
                ("note", Payload::Text("Revised".into())),
            ]);
            self.revision = Some(builder.node("Task", self.task, payload));
        }
        2 => {
            let payload = body([
                ("kind", Payload::Text("committed".into())),
                ("at", builder.at()),
                ("cost", Payload::Integer(1)),
            ]);
            self.about = Some(builder.node("Stance", None, payload));
        }
        4 => self.about = Some(builder.node("Done", None, body([("at", builder.at())]))),
        _ => builder.edge(
            "About",
            self.about.expect("a stance or done precedes its edge"),
            self.revision.expect("the revision precedes its edges"),
        ),
        }
        self.step = (self.step + 1) % Self::STEPS;
        (self.step == 0).then(|| {
            (
                self.task.expect("a finished task"),
                self.revision.expect("a finished task"),
            )
        })
    }
}

// splitmix64: small, deterministic, and enough to vary a history.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
}

// A current node variation may revise, split, or link.
#[derive(Clone)]
struct Item {
    id: EntryId,
    ty_name: String,
}

// The item types variation appends, each with its labels in the planning
// declarations, so most links are permitted and some are not.
const ITEM_TYPES: [&str; 6] = ["Task", "Project", "Goal", "Habit", "External", "Stance"];
const LABELS: [&str; 4] = ["Enables", "Helps", "Until", "About"];

// The variation of `varied_history`. Everything it references is current:
// it tracks which of its items and declarations it has superseded, and the
// task stream never supersedes what it hands over.
struct Variation {
    rng: Rng,
    items: Vec<Item>,
    // The current revision of each planning declaration, and each second
    // declaration that claims one of their names.
    declarations: Vec<EntryId>,
    spares: Vec<(EntryId, Type)>,
    // Superseded roots, which may be appended again with the same content.
    superseded_roots: Vec<(String, Body)>,
    // Every node payload variation writes carries a fresh number, so no two
    // current nodes have equal content by accident.
    number: usize,
}

impl Variation {
    fn new(seed: u64, declarations: [EntryId; DECLARATIONS]) -> Self {
        Self {
            rng: Rng(seed),
            items: Vec::new(),
            declarations: declarations.into(),
            spares: Vec::new(),
            superseded_roots: Vec::new(),
            number: 0,
        }
    }

    // A finished task's revision becomes an item, and its root, superseded
    // since its second entry, may be appended again.
    fn finished(&mut self, builder: &Builder, (root, revision): (EntryId, EntryId)) {
        let root = builder.entries[root.position()]
            .as_node()
            .expect("a task is a node");
        self.superseded_roots
            .push((root.ty_name.clone(), root.payload.clone()));
        self.items.push(Item {
            id: revision,
            ty_name: "Task".into(),
        });
    }

    fn fresh(&mut self) -> usize {
        self.number += 1;
        self.number
    }

    fn pick(&mut self, len: usize) -> Option<usize> {
        (len > 0).then(|| self.rng.below(len))
    }

    // A payload valid for `ty_name` under the original declarations.
    fn payload(&mut self, builder: &Builder, ty_name: &str) -> Body {
        let title = Payload::Text(format!("Item {}", self.fresh()));
        match ty_name {
        "Habit" | "Concern" => body([("title", title), ("quota", Payload::Integer(3))]),
        "Stance" => body([
            ("kind", Payload::Text("committed".into())),
            ("at", builder.at()),
            ("reason", title),
        ]),
        _ => body([("title", title)]),
        }
    }

    fn event(&mut self, builder: &mut Builder) {
        match self.rng.below(20) {
        0..=4 => self.add_item(builder),
        5..=8 => self.revise(builder),
        9 => self.split(builder),
        10..=14 => self.link(builder),
        15..=16 => self.revise_declaration(builder),
        17 => self.add_spare(builder),
        18 => self.repeat_root(builder),
        _ => self.misfit(builder),
        }
    }

    fn add_item(&mut self, builder: &mut Builder) {
        let ty_name = ITEM_TYPES[self.rng.below(ITEM_TYPES.len())];
        let payload = self.payload(builder, ty_name);
        let id = builder.node(ty_name, None, payload);
        self.items.push(Item {
            id,
            ty_name: ty_name.into(),
        });
    }

    // Supersedes item `index` with `successors`, each of the item's type or,
    // rarely, of another (which TypeCheck rejects as a transition).
    fn supersede(&mut self, builder: &mut Builder, index: usize, successors: usize) {
        let item = self.items.swap_remove(index);
        let predecessor = builder.entries[item.id.position()]
            .as_node()
            .expect("an item is a node")
            .clone();
        if predecessor.predecessor.is_none() {
            self.superseded_roots
                .push((predecessor.ty_name.clone(), predecessor.payload.clone()));
        }
        let mut added = Vec::new();
        for _ in 0..successors {
            let ty_name = if self.rng.one_in(8) {
                ITEM_TYPES[self.rng.below(ITEM_TYPES.len())].to_owned()
            } else {
                item.ty_name.clone()
            };
            let payload = self.payload(builder, &ty_name);
            let id = builder.node(&ty_name, Some(item.id), payload);
            added.push(Item { id, ty_name });
        }
        // Carries some of the predecessor's edges over to the first
        // successor, whose current far endpoint keeps them valid; the rest
        // are left behind for Reconnect.
        let edges = builder
            .entries
            .iter()
            .filter_map(Entry::as_edge)
            .filter(|edge| edge.from == item.id || edge.to == item.id)
            .cloned()
            .collect::<Vec<_>>();
        let current = |id: EntryId, builder: &Builder| {
            !builder
                .entries
                .iter()
                .filter_map(Entry::as_node)
                .any(|node| node.predecessor == Some(id))
        };
        for edge in edges {
            if !self.rng.one_in(2) {
                continue;
            }
            let (from, to) = if edge.from == item.id {
                (added[0].id, edge.to)
            } else {
                (edge.from, added[0].id)
            };
            let other = if edge.from == item.id {
                edge.to
            } else {
                edge.from
            };
            if current(other, builder) && !builder.edges.contains(&(from, to, edge.ty_name.clone()))
            {
                builder.edge(&edge.ty_name, from, to);
            }
        }
        self.items.extend(added);
    }

    // One revision, so items grow chains of every length.
    fn revise(&mut self, builder: &mut Builder) {
        if let Some(index) = self.pick(self.items.len()) {
            self.supersede(builder, index, 1);
        }
    }

    fn split(&mut self, builder: &mut Builder) {
        if let Some(index) = self.pick(self.items.len()) {
            self.supersede(builder, index, 2);
        }
    }

    // An edge between two current items, permitted by their declarations or
    // not, and possibly closing a cycle.
    fn link(&mut self, builder: &mut Builder) {
        let (Some(a), Some(b)) = (self.pick(self.items.len()), self.pick(self.items.len())) else {
            return;
        };
        let label = LABELS[self.rng.below(LABELS.len())];
        let (from, to) = (self.items[a].id, self.items[b].id);
        if from != to && !builder.edges.contains(&(from, to, label.into())) {
            builder.edge(label, from, to);
        }
    }

    // Revises a declaration: back to its original, with an added optional
    // field, with every field mandatory (so current nodes miss some), or
    // malformed. A spare is renamed instead, ending its claim.
    fn revise_declaration(&mut self, builder: &mut Builder) {
        if !self.spares.is_empty() && self.rng.one_in(2) {
            let index = self.rng.below(self.spares.len());
            let (id, mut ty) = self.spares.swap_remove(index);
            ty.name = format!("Spare {}", self.fresh());
            builder.node(Type::TYPE_NAME, Some(id), ty.to_body());
            return;
        }
        let index = self.rng.below(self.declarations.len());
        let original = declarations()[index].clone();
        let payload = match self.rng.below(4) {
        0 => original.to_body(),
        1 => {
            let mut revised = original.clone();
            revised.fields.insert(
                format!("extra{}", self.fresh()),
                FieldDecl {
                    status: FieldStatus::Optional,
                    shape: ValueType::Text,
                },
            );
            revised.to_body()
        }
        2 => {
            let mut revised = original.clone();
            for field in revised.fields.values_mut() {
                field.status = FieldStatus::Mandatory;
            }
            revised.to_body()
        }
        _ => {
            let mut malformed = original.to_body();
            malformed.insert(format!("unknown{}", self.fresh()), Payload::Integer(1));
            malformed
        }
        };
        // Declarations never split, so a revision has no sibling whose equal
        // content would make it a duplicate.
        self.declarations[index] =
            builder.node(Type::TYPE_NAME, Some(self.declarations[index]), payload);
    }

    // A second root declaration of a planning name, which makes the name
    // ambiguous until the spare is renamed.
    fn add_spare(&mut self, builder: &mut Builder) {
        let index = self.rng.below(DECLARATIONS);
        let mut spare = declarations()[index].clone();
        spare
            .successors
            .insert(format!("Successor {}", self.fresh()));
        let id = builder.node(Type::TYPE_NAME, None, spare.to_body());
        self.spares.push((id, spare));
    }

    // A root with the content of a superseded root: permitted, and the
    // content index then holds a stale and a current node.
    fn repeat_root(&mut self, builder: &mut Builder) {
        let Some(index) = self.pick(self.superseded_roots.len()) else {
            return;
        };
        let (ty_name, payload) = self.superseded_roots.swap_remove(index);
        let id = builder.node(&ty_name, None, payload);
        self.items.push(Item { id, ty_name });
    }

    // A node TypeCheck rejects: an undeclared type, a missing mandatory
    // field, an undeclared field, or a mistyped one.
    fn misfit(&mut self, builder: &mut Builder) {
        let number = Payload::Text(format!("Item {}", self.fresh()));
        let (ty_name, payload) = match self.rng.below(4) {
        0 => ("Ghost", body([("title", number)])),
        1 => ("Task", body([("note", number)])),
        2 => (
            "Task",
            body([("title", number), ("colour", Payload::Boolean(true))]),
        ),
        _ => (
            "Habit",
            body([("title", number), ("quota", Payload::Text("three".into()))]),
        ),
        };
        let id = builder.node(ty_name, None, payload);
        self.items.push(Item {
            id,
            ty_name: ty_name.into(),
        });
    }
}

fn body<const N: usize>(fields: [(&str, Payload); N]) -> Body {
    fields
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

#[cfg(test)]
mod test;
