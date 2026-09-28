use crate::{
    AppError,
    proposal::{PlanId, Proposal, ProposalLocation},
    storage::{self, Snapshot, StatePath},
};
use lifecore::{
    BasicValidationFailure, Body, Entry, EntryId, LifeCoreError, LifeCoreList, Timestamp, View,
    ViewId,
    analysis::{
        AnalysisOutput, AnalysisRunner, AnalyzerFactory, Config, EvaluationInputs, SuiteConfig,
    },
    inspection::{Currentness, EdgeSelection, NodeSelection},
    transaction::{self, Change, CheckedCandidate},
};
use serde::{Deserialize, Serialize};
use std::fs;
use uuid::Uuid;

//A Called once per operation, so each request gets fresh analyzer instances
//A and its own frozen evaluation inputs.
pub type Factories = Box<dyn Fn() -> Vec<Box<dyn AnalyzerFactory>>>;

//A The synchronous application service shared by the CLI and RPC. It keeps no
//A graph or analysis state between calls; every call reopens the snapshot.
pub struct Service {
    state: StatePath,
    factories: Factories,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalRequest {
    pub proposal: Proposal,
    #[serde(default)]
    pub evaluation_time: Option<Timestamp>,
    #[serde(default)]
    pub now: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddAnalyzerRequest {
    pub name: String,
    #[serde(default = "empty_configuration")]
    pub configuration: String,
    #[serde(default)]
    pub evaluation_time: Option<Timestamp>,
    #[serde(default)]
    pub now: bool,
}

fn empty_configuration() -> String {
    "{}".into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Node,
    Edge,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryListRequest {
    #[serde(default)]
    pub kind: Option<EntryKind>,
    #[serde(default, rename = "type")]
    pub ty_name: Option<String>,
    #[serde(default)]
    pub currentness: Currentness,
    //A Case-sensitive substring over node text values; edges have no payload.
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryGetRequest {
    pub id: EntryId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Initialized {
    pub path: String,
    pub database: Uuid,
    pub graph_revision: Uuid,
    pub suite: SuiteConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Description {
    pub database: Uuid,
    pub graph_revision: Uuid,
    pub view: ViewId,
    pub suite: SuiteConfig,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MintedPlanId {
    pub plan_id: PlanId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEntry {
    pub name: String,
    pub entry: EntryId,
}

//A The complete context of one check: the exact base it was prepared against,
//A the operation-to-entry mapping (provisional for previews), and the full
//A unfiltered analysis of every prefix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalReport {
    pub database: Uuid,
    pub base_revision: Uuid,
    pub base: ViewId,
    pub operations: Vec<OperationEntry>,
    pub analysis: AnalysisOutput,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyResult {
    pub graph_revision: Uuid,
    pub report: ProposalReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerAdded {
    pub graph_revision: Uuid,
    pub suite: SuiteConfig,
    pub report: ProposalReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryRecord {
    Node {
        id: EntryId,
        #[serde(rename = "type")]
        ty_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        predecessor: Option<EntryId>,
        payload: Body,
        current: bool,
    },
    Edge {
        id: EntryId,
        #[serde(rename = "type")]
        ty_name: String,
        from: EntryId,
        to: EntryId,
        current: bool,
    },
}

impl EntryRecord {
    fn new(view: View<'_>, entry: &Entry) -> Self {
        let current = view.is_stale(entry.id()) == Some(false);
        match entry {
        Entry::Node(node) => Self::Node {
            id: node.id,
            ty_name: node.ty_name.clone(),
            predecessor: node.parent,
            payload: node.body.clone(),
            current,
        },
        Entry::Edge(edge) => Self::Edge {
            id: edge.id,
            ty_name: edge.ty_name.clone(),
            from: edge.from,
            to: edge.to,
            current,
        },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryList {
    pub database: Uuid,
    pub graph_revision: Uuid,
    pub view: ViewId,
    pub entries: Vec<EntryRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryLookup {
    pub view: ViewId,
    pub entry: EntryRecord,
}

//A The host captures `now` once, before evaluation; neither runner nor
//A analyzers read the clock.
fn evaluation_inputs(time: Option<Timestamp>, now: bool) -> Result<EvaluationInputs, AppError> {
    let evaluation_time = match (time, now) {
    (Some(_), true) => {
        return Err(AppError::InvalidParams {
            message: "choose either an explicit evaluation time or now, not both".into(),
        });
    }
    (_, true) => Some(Timestamp::now()),
    (time, false) => time,
    };
    Ok(EvaluationInputs { evaluation_time })
}

fn database(graph: &LifeCoreList) -> Uuid {
    graph.view().id().database
}

fn verify_plan_id(snapshot: &Snapshot, plan_id: Option<PlanId>) -> Result<(), AppError> {
    let Some(plan_id) = plan_id else {
        return Ok(());
    };
    let actual = database(&snapshot.graph);
    if plan_id.database != actual {
        return Err(AppError::ForeignDatabase {
            expected: plan_id.database,
            actual,
        });
    }
    if plan_id.graph_revision != snapshot.graph_revision {
        return Err(AppError::StaleBase {
            expected: plan_id.graph_revision,
            actual: snapshot.graph_revision,
        });
    }
    Ok(())
}

//A Translate core preparation failures into application errors, attaching the
//A proposal entry when one is identifiable. Each proposal entry appends exactly
//A one history entry, so a staged position maps back to its operation.
fn preparation_error(
    error: transaction::Error,
    proposal: Option<&Proposal>,
    base_len: usize,
) -> AppError {
    use transaction::Error as E;
    let at = |operation: usize, field: Option<&str>| {
        let entry = proposal?.entries.get(operation)?;
        Some(ProposalLocation {
            operation,
            name: entry.name().into(),
            field: field.map(Into::into),
        })
    };
    let message = error.to_string();
    match error {
    E::Core(LifeCoreError::BasicValidationFailure(failure)) => {
        let position = match failure {
        BasicValidationFailure::InvalidIdentity { position } => Some(position),
        BasicValidationFailure::InvalidReference { entry, .. }
        | BasicValidationFailure::Duplicate { entry }
        | BasicValidationFailure::StaleEndpoint { entry }
        | BasicValidationFailure::SelfEdge { entry } => Some(entry.position()),
        BasicValidationFailure::InvalidDatabase => None,
        };
        AppError::InvalidProposal {
            location: position
                .and_then(|position| position.checked_sub(base_len))
                .and_then(|operation| at(operation, None)),
            message,
        }
    }
    E::Core(error) => AppError::Analyzer(error),
    E::MissingIntent { operation }
    | E::UnexpectedIntent { operation }
    | E::RevisesSuperseded { operation } => AppError::InvalidProposal {
        location: at(operation, Some("intent")),
        message,
    },
    E::InvalidPlanReference { .. } => AppError::InvalidProposal {
        location: None,
        message,
    },
    E::StaleBase => AppError::Internal { message },
    }
}

impl Service {
    pub fn new(state: StatePath, factories: Factories) -> Self {
        Self { state, factories }
    }

    fn runner(&self, inputs: EvaluationInputs) -> Result<AnalysisRunner, AppError> {
        let mut runner = AnalysisRunner::new(inputs);
        for factory in (self.factories)() {
            runner
                .register_analyzer_kind(factory)
                .map_err(AppError::Analyzer)?;
        }
        Ok(runner)
    }

    //A Stage and check changes against one loaded snapshot; nothing is stored.
    fn prepare(
        &self,
        snapshot: &Snapshot,
        proposal: Option<&Proposal>,
        changes: &[Change],
        inputs: EvaluationInputs,
    ) -> Result<(CheckedCandidate, ProposalReport), AppError> {
        let mut runner = self.runner(inputs)?;
        let base = snapshot.graph.view();
        let checked = transaction::prepare(&snapshot.graph, changes, &mut runner)
            .map_err(|error| preparation_error(error, proposal, base.len()))?;
        let operations = proposal
            .map(|proposal| {
                proposal
                    .entries
                    .iter()
                    .zip(checked.operations())
                    .filter_map(|(entry, id)| {
                        Some(OperationEntry {
                            name: entry.name().into(),
                            entry: (*id)?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let report = ProposalReport {
            database: database(&snapshot.graph),
            base_revision: snapshot.graph_revision,
            base: base.id(),
            operations,
            analysis: checked.analysis().clone(),
        };
        Ok((checked, report))
    }

    fn prepare_proposal(
        &self,
        snapshot: &Snapshot,
        proposal: &Proposal,
        inputs: EvaluationInputs,
    ) -> Result<(CheckedCandidate, ProposalReport), AppError> {
        verify_plan_id(snapshot, proposal.plan_id)?;
        let changes = proposal
            .lower(snapshot.graph.view())
            .map_err(|(location, error)| AppError::InvalidProposal {
                location: Some(location),
                message: error.to_string(),
            })?;
        self.prepare(snapshot, Some(proposal), &changes, inputs)
    }

    //A Commit a checked candidate into the loaded snapshot and publish it under
    //A the caller's writer lock. A fresh revision marks every graph/suite change.
    fn publish(
        &self,
        mut snapshot: Snapshot,
        checked: CheckedCandidate,
        report: ProposalReport,
        lock: &storage::WriterLock,
    ) -> Result<(Snapshot, ProposalReport), AppError> {
        let Ok(prepared) = checked.into_prepared() else {
            return Err(AppError::Rejected(Box::new(report)));
        };
        //A Every node, edge, or analyzer operation changes graph/suite state; an
        //A empty plan is a no-op that keeps the revision and skips publication.
        let changed = !prepared.checked().operations().is_empty();
        prepared
            .commit(&mut snapshot.graph)
            .map_err(|error| preparation_error(error, None, 0))?;
        if changed {
            snapshot.graph_revision = Uuid::new_v4();
            storage::publish(&self.state, &snapshot, lock)?;
        }
        Ok((snapshot, report))
    }

    pub fn init(&self, suite: &[Config]) -> Result<Initialized, AppError> {
        let lock = storage::lock(&self.state)?;
        if fs::symlink_metadata(self.state.path()).is_ok() {
            return Err(AppError::DatabaseExists {
                path: self.state.path().into(),
            });
        }
        let mut graph = LifeCoreList::default();
        let changes = suite
            .iter()
            .map(|config| Change::AddAnalyzer {
                name: config.name.clone(),
                config: config.config.clone(),
            })
            .collect::<Vec<_>>();
        //A The initial suite is added through the ordinary checked path, so
        //A missing factories and invalid configurations are rejected here too.
        let checked = transaction::prepare(&graph, &changes, &mut self.runner(Default::default())?)
            .map_err(|error| preparation_error(error, None, 0))?;
        let prepared = checked.into_prepared().map_err(|_| AppError::Internal {
            message: "the initial suite emitted inconsistencies on an empty history".into(),
        })?;
        prepared
            .commit(&mut graph)
            .map_err(|error| preparation_error(error, None, 0))?;
        let snapshot = Snapshot::new(graph);
        storage::publish_new(&self.state, &snapshot, &lock)?;
        Ok(Initialized {
            path: self.state.path().to_string_lossy().into_owned(),
            database: database(&snapshot.graph),
            graph_revision: snapshot.graph_revision,
            suite: snapshot.graph.analysis_suite().clone(),
        })
    }

    pub fn describe(&self, _: NoParams) -> Result<Description, AppError> {
        let snapshot = storage::read(&self.state)?;
        Ok(Description {
            database: database(&snapshot.graph),
            graph_revision: snapshot.graph_revision,
            view: snapshot.graph.view().id(),
            suite: snapshot.graph.analysis_suite().clone(),
        })
    }

    pub fn list_entries(&self, request: EntryListRequest) -> Result<EntryList, AppError> {
        let snapshot = storage::read(&self.state)?;
        let view = snapshot.graph.view();
        let ty_name = request.ty_name.as_deref();
        let mut entries = vec![];
        if request.kind != Some(EntryKind::Edge) {
            let selection = NodeSelection {
                ty_name,
                currentness: request.currentness,
                fields: vec![],
                text: request.text.as_deref(),
            };
            entries.extend(view.select_nodes(&selection).map(|node| node.id));
        }
        if request.kind != Some(EntryKind::Node) && request.text.is_none() {
            let selection = EdgeSelection {
                ty_name,
                currentness: request.currentness,
            };
            entries.extend(view.select_edges(selection).map(|edge| edge.id));
        }
        //A Entry collections use append order.
        entries.sort_by_key(|id| id.position());
        Ok(EntryList {
            database: database(&snapshot.graph),
            graph_revision: snapshot.graph_revision,
            view: view.id(),
            entries: entries
                .into_iter()
                .filter_map(|id| Some(EntryRecord::new(view, view.get(id)?)))
                .collect(),
        })
    }

    pub fn get_entry(&self, request: EntryGetRequest) -> Result<EntryLookup, AppError> {
        let snapshot = storage::read(&self.state)?;
        let view = snapshot.graph.view();
        let entry = view
            .get(request.id)
            .ok_or(AppError::EntryNotFound { id: request.id })?;
        Ok(EntryLookup {
            view: view.id(),
            entry: EntryRecord::new(view, entry),
        })
    }

    pub fn mint_plan_id(&self, _: NoParams) -> Result<MintedPlanId, AppError> {
        let snapshot = storage::read(&self.state)?;
        Ok(MintedPlanId {
            plan_id: PlanId {
                database: database(&snapshot.graph),
                graph_revision: snapshot.graph_revision,
                proposal: Uuid::new_v4(),
            },
        })
    }

    //A A preview: it reads one snapshot without the writer lock and stores
    //A nothing. A rejected proposal returns its complete report as an error.
    pub fn check(&self, request: ProposalRequest) -> Result<ProposalReport, AppError> {
        if request.proposal.acknowledges.is_some() {
            return Err(AppError::AcknowledgmentUnsupported);
        }
        let inputs = evaluation_inputs(request.evaluation_time, request.now)?;
        let snapshot = storage::read(&self.state)?;
        let (_, report) = self.prepare_proposal(&snapshot, &request.proposal, inputs)?;
        if report.analysis.has_error() {
            return Err(AppError::Rejected(Box::new(report)));
        }
        Ok(report)
    }

    //A Always checks anew under writer exclusion, from reload through
    //A publication, so the concrete base cannot change underneath it.
    pub fn apply(&self, request: ProposalRequest) -> Result<ApplyResult, AppError> {
        //A TODO: verify `acknowledges` by replay (PLAN 4.4). Until then it is
        //A rejected rather than silently ignored.
        if request.proposal.acknowledges.is_some() {
            return Err(AppError::AcknowledgmentUnsupported);
        }
        let inputs = evaluation_inputs(request.evaluation_time, request.now)?;
        storage::require_existing(&self.state)?;
        let lock = storage::lock(&self.state)?;
        let snapshot = storage::read(&self.state)?;
        let (checked, report) = self.prepare_proposal(&snapshot, &request.proposal, inputs)?;
        let (snapshot, report) = self.publish(snapshot, checked, report, &lock)?;
        Ok(ApplyResult {
            graph_revision: snapshot.graph_revision,
            report,
        })
    }

    //A Appends to the ordered suite through a one-operation transaction, so the
    //A full replay under the extended suite must pass for every prefix.
    pub fn add_analyzer(&self, request: AddAnalyzerRequest) -> Result<AnalyzerAdded, AppError> {
        let inputs = evaluation_inputs(request.evaluation_time, request.now)?;
        storage::require_existing(&self.state)?;
        let lock = storage::lock(&self.state)?;
        let snapshot = storage::read(&self.state)?;
        let changes = [Change::AddAnalyzer {
            name: request.name,
            config: request.configuration,
        }];
        let (checked, report) = self.prepare(&snapshot, None, &changes, inputs)?;
        let (snapshot, report) = self.publish(snapshot, checked, report, &lock)?;
        Ok(AnalyzerAdded {
            graph_revision: snapshot.graph_revision,
            suite: snapshot.graph.analysis_suite().clone(),
            report,
        })
    }
}
