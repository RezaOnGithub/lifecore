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
    typing::Type,
};
use serde_json::json;
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
    };
    for declaration in declarations() {
        builder.node(Type::TYPE_NAME, None, declaration.to_body());
    }
    builder.window = 1;
    let mut tasks = Tasks::default();
    for offset in 0..shape.entries - DECLARATIONS {
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
        } else {
            tasks.next(&mut builder);
        }
    }
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

    fn next(&mut self, builder: &mut Builder) {
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
