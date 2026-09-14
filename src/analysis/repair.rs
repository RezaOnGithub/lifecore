use super::{AnalysisRunner, Invocation, MessageKind, Observation};
use crate::{
    Body, LifeCoreList, Payload, ViewError,
    transaction::{self, Change, PreparedTransaction, Reference},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionIntent {
    Revise,
    Branch,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SuggestedAction {
    AddNode {
        ty_name: String,
        payload: Body,
        parent: Option<Reference>,
        intent: Option<RevisionIntent>,
    },
    AddEdge {
        from: Reference,
        ty_name: String,
        to: Reference,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the selected observation has no suggested repair")]
    NotSuggestion,
    #[error("repair plan identity must not be empty")]
    EmptyPlanId,
    #[error(transparent)]
    View(#[from] ViewError),
    #[error(transparent)]
    Check(#[from] transaction::Error),
    #[error(
        "repair operation {operation} has inconsistent predecessor/intent or revises a stale predecessor"
    )]
    Intent { operation: usize },
    #[error("timestamp is outside the TOML UTC range 0000 through 9999")]
    TimestampRange,
    #[error(transparent)]
    Toml(#[from] toml::ser::Error),
}

pub struct CheckedRepair {
    toml: String,
    transaction: PreparedTransaction,
}

impl CheckedRepair {
    pub fn toml(&self) -> &str {
        &self.toml
    }
    pub fn transaction(&self) -> &PreparedTransaction {
        &self.transaction
    }
    pub fn into_transaction(self) -> PreparedTransaction {
        self.transaction
    }
}

//A Selecting an observation is explicit. Its ordered actions are checked anew
//A against the supplied base and runner inputs; checking never commits them.
pub fn check(
    graph: &LifeCoreList,
    observation: &Observation,
    plan_id: &str,
    runner: &mut AnalysisRunner,
) -> Result<CheckedRepair, Error> {
    if plan_id.trim().is_empty() {
        return Err(Error::EmptyPlanId);
    }
    if graph.view().id().database != observation.key.database {
        return Err(ViewError::DifferentHistory.into());
    }
    if let Invocation::Prefix(at) = observation.invocation
        && graph.view().prefix(at.end)?.id() != at
    {
        return Err(ViewError::DifferentHistory.into());
    }
    let MessageKind::Suggestion(actions) = &observation.diagnostic.kind else {
        return Err(Error::NotSuggestion);
    };
    let changes: Vec<_> = actions
        .iter()
        .map(|action| match action {
        SuggestedAction::AddNode {
            ty_name,
            payload,
            parent,
            ..
        } => Change::AddNode {
            ty_name: ty_name.clone(),
            body: payload.clone(),
            parent: *parent,
        },
        SuggestedAction::AddEdge { from, ty_name, to } => Change::AddEdge {
            from: *from,
            ty_name: ty_name.clone(),
            to: *to,
        },
        })
        .collect();
    let transaction = transaction::prepare(graph, &changes, runner)?;
    for (operation, (action, id)) in actions.iter().zip(transaction.added()).enumerate() {
        if let SuggestedAction::AddNode { intent, .. } = action {
            let parent = transaction.view().node(*id).expect("prepared node").parent;
            let before = transaction.view().prefix(id.position())?;
            if parent.is_some() != intent.is_some()
                || (*intent == Some(RevisionIntent::Revise)
                    && parent.is_some_and(|id| before.is_stale(id) != Some(false)))
            {
                return Err(Error::Intent { operation });
            }
        }
    }
    let entries = actions
        .iter()
        .enumerate()
        .map(|(operation, action)| encode_action(operation, action))
        .collect::<Result<Vec<_>, _>>()?;
    let envelope = toml::Table::from_iter([
        ("format_version".into(), 1.into()),
        ("value_profile".into(), "toml-basic-v1".into()),
        (
            "database".into(),
            graph.view().id().database.to_string().into(),
        ),
        ("base".into(), (graph.view().len() as i64).into()),
        ("plan_id".into(), plan_id.into()),
        ("entries".into(), toml::Value::Array(entries)),
    ]);
    let toml = toml::to_string_pretty(&envelope)?;
    Ok(CheckedRepair { toml, transaction })
}

fn encode_reference(reference: Reference) -> String {
    match reference {
    //A Full origin/position handles preserve exact identity across reloads and
    //A independent candidates. Generated aliases name earlier operations only.
    Reference::Existing(id) => format!("@{}:{}", id.origin, id.position()),
    Reference::Added(operation) => format!("$repair_{operation}"),
    }
}

fn encode_action(operation: usize, action: &SuggestedAction) -> Result<toml::Value, Error> {
    let mut entry = toml::Table::new();
    entry.insert("name".into(), format!("repair_{operation}").into());
    match action {
    SuggestedAction::AddNode {
        ty_name,
        payload,
        parent,
        intent,
    } => {
        entry.insert("kind".into(), "node".into());
        entry.insert("type".into(), ty_name.clone().into());
        entry.insert("payload".into(), encode_body(payload)?.into());
        if let Some(parent) = parent {
            entry.insert("predecessor".into(), encode_reference(*parent).into());
        }
        if let Some(intent) = intent {
            entry.insert(
                "intent".into(),
                match intent {
                RevisionIntent::Revise => "revise",
                RevisionIntent::Branch => "branch",
                }
                .into(),
            );
        }
    }
    SuggestedAction::AddEdge { from, ty_name, to } => {
        entry.insert("kind".into(), "edge".into());
        entry.insert("type".into(), ty_name.clone().into());
        entry.insert("from".into(), encode_reference(*from).into());
        entry.insert("to".into(), encode_reference(*to).into());
    }
    }
    Ok(entry.into())
}

fn encode_body(body: &Body) -> Result<toml::Table, Error> {
    body.iter()
        .map(|(name, value)| Ok((name.clone(), encode_value(value)?)))
        .collect()
}

fn encode_value(value: &Payload) -> Result<toml::Value, Error> {
    Ok(match value {
    Payload::Text(v) => v.clone().into(),
    Payload::Number(v) => (*v).into(),
    Payload::Boolean(v) => (*v).into(),
    Payload::List(v) => toml::Value::Array(v.iter().map(encode_value).collect::<Result<_, _>>()?),
    Payload::Record(v) => encode_body(v)?.into(),
    Payload::PreciseTime(v) => {
        //A Core timestamps have a wider range. Export only native UTC TOML
        //A datetimes and retain all nanoseconds; no string fallback or clamping.
        let text = v.to_string();
        let datetime = text
            .parse::<toml::value::Datetime>()
            .map_err(|_| Error::TimestampRange)?;
        if datetime.date.is_none()
            || datetime.time.is_none()
            || datetime.offset != Some(toml::value::Offset::Z)
        {
            return Err(Error::TimestampRange);
        }
        toml::Value::Datetime(datetime)
    }
    })
}
