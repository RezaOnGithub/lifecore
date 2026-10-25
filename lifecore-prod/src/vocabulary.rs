//! Planning's vocabulary (PLANNING §3): ten node types, four edge labels,
//! six stance kinds, and the canonical declarations of §11.

use lifecore::typing::{
    FieldStatus::{Mandatory, Optional},
    Type,
    ValueType::{Integer, Text, Timestamp},
};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum PlanningType {
    Goal,
    Concern,
    Project,
    Task,
    Habit,
    External,
    Stance,
    Done,
    Did,
    Review,
}

impl PlanningType {
    pub const ALL: [Self; 10] = [
        Self::Goal,
        Self::Concern,
        Self::Project,
        Self::Task,
        Self::Habit,
        Self::External,
        Self::Stance,
        Self::Done,
        Self::Did,
        Self::Review,
    ];

    pub fn name(self) -> &'static str {
        match self {
        Self::Goal => "Goal",
        Self::Concern => "Concern",
        Self::Project => "Project",
        Self::Task => "Task",
        Self::Habit => "Habit",
        Self::External => "External",
        Self::Stance => "Stance",
        Self::Done => "Done",
        Self::Did => "Did",
        Self::Review => "Review",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|ty| ty.name() == name)
    }

    // Items are what the plan is about; the other four are bookkeeping nodes.
    pub fn is_item(self) -> bool {
        !matches!(self, Self::Stance | Self::Done | Self::Did | Self::Review)
    }

    // The declarations of PLANNING §11, which bootstrap suggests and
    // section 3.4 measures amendments against.
    pub fn canonical(self) -> Type {
        let title = ("title", (Text, Mandatory));
        match self {
        Self::Goal => Type::new(
            "Goal",
            &[
                title,
                ("criterion", (Text, Optional)),
                ("deadline", (Timestamp, Optional)),
            ],
            &[],
            &["Enables", "Helps", "About"],
            &[],
        ),
        Self::Concern => Type::new(
            "Concern",
            &[title, ("quota", (Integer, Mandatory))],
            &[],
            &["Helps", "About"],
            &[],
        ),
        Self::Project => Type::new(
            "Project",
            &[
                title,
                ("deliverable", (Text, Optional)),
                ("deadline", (Timestamp, Optional)),
            ],
            &["Enables", "Helps"],
            &["Enables", "Helps", "About", "Until"],
            &[],
        ),
        Self::Task => Type::new(
            "Task",
            &[title, ("note", (Text, Optional))],
            &["Enables", "Helps"],
            &["Enables", "Helps", "About", "Until"],
            &[],
        ),
        Self::Habit => Type::new(
            "Habit",
            &[title, ("quota", (Integer, Mandatory))],
            &["Helps"],
            &["About"],
            &[],
        ),
        Self::External => Type::new(
            "External",
            &[title],
            &["Enables", "Helps"],
            &["About", "Until"],
            &[],
        ),
        Self::Stance => Type::new(
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
        Self::Done => Type::new(
            "Done",
            &[("at", (Timestamp, Mandatory)), ("note", (Text, Optional))],
            &["About"],
            &[],
            &[],
        ),
        Self::Did => Type::new(
            "Did",
            &[("at", (Timestamp, Mandatory)), ("note", (Text, Optional))],
            &["About"],
            &[],
            &[],
        ),
        Self::Review => Type::new(
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
        }
    }
}

impl fmt::Display for PlanningType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Label {
    Enables,
    Helps,
    About,
    Until,
}

impl Label {
    pub const ALL: [Self; 4] = [Self::Enables, Self::Helps, Self::About, Self::Until];

    pub fn name(self) -> &'static str {
        match self {
        Self::Enables => "Enables",
        Self::Helps => "Helps",
        Self::About => "About",
        Self::Until => "Until",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|label| label.name() == name)
    }

    // Whether section 3.2 permits this label from a node of type `from` to
    // one of type `to`. A stance's `About` may target any item here; its kind
    // narrows the type separately (`StanceKind::applies_to`).
    pub fn permits(self, from: PlanningType, to: PlanningType) -> bool {
        use PlanningType::*;
        match self {
        Self::Enables => {
            matches!(from, Task | Project | External) && matches!(to, Task | Project | Goal)
        }
        Self::Helps => {
            matches!(from, Task | Project | Habit | External)
                && matches!(to, Task | Project | Goal | Concern)
        }
        Self::About => match from {
        Stance => to.is_item(),
        Done => matches!(to, Task | Project | External),
        Did => matches!(to, Habit | Concern),
        _ => false,
        },
        Self::Until => from == Stance && matches!(to, Task | Project | External),
        }
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StanceKind {
    Committed,
    Deferred,
    Waiting,
    Resumed,
    Sufficient,
    Abandoned,
}

impl StanceKind {
    pub const ALL: [Self; 6] = [
        Self::Committed,
        Self::Deferred,
        Self::Waiting,
        Self::Resumed,
        Self::Sufficient,
        Self::Abandoned,
    ];

    pub fn name(self) -> &'static str {
        match self {
        Self::Committed => "committed",
        Self::Deferred => "deferred",
        Self::Waiting => "waiting",
        Self::Resumed => "resumed",
        Self::Sufficient => "sufficient",
        Self::Abandoned => "abandoned",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    // The item types a stance of this kind may be about (section 3.3).
    pub fn applies_to(self, item: PlanningType) -> bool {
        match self {
        Self::Committed => item == PlanningType::Task,
        Self::Waiting => item == PlanningType::External,
        Self::Sufficient => item == PlanningType::Goal,
        Self::Deferred | Self::Resumed | Self::Abandoned => item.is_item(),
        }
    }

    // The conditional fields of section 3.3: `cost` belongs to `committed`
    // alone, and `horizon` to `deferred` and `waiting`.
    pub fn takes_cost(self) -> bool {
        self == Self::Committed
    }
    pub fn takes_horizon(self) -> bool {
        matches!(self, Self::Deferred | Self::Waiting)
    }
}

impl fmt::Display for StanceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
