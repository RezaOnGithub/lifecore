use crate::{
    AppError,
    proposal::{PlanId, Proposal, ProposalLocation, present},
    storage::{self, LockedSnapshot, Publication, Snapshot, StatePath, SyncDirectory},
};
use lifecore::{
    BasicValidationFailure, Entry, EntryId, LifeCoreError, LifeCoreList, Timestamp, View, ViewId,
    analysis::{
        AnalysisOutput, AnalysisRunner, AnalyzerFactory, Config, EvaluationInputs, SuiteConfig,
    },
    inspection::{Currentness, EdgeSelection, NodeSelection},
    transaction::{self, Change, Checked},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

// Called once per operation, so each request gets fresh analyzer instances
// and its own frozen evaluation inputs. Only this constructor must be
// `Send + Sync`, so one service can answer requests on several threads;
// the factories and analyzers it builds stay on the calling thread.
pub type Factories = Box<dyn Fn() -> Vec<Box<dyn AnalyzerFactory>> + Send + Sync>;

// The synchronous application service shared by the CLI and RPC. It keeps no
// graph or analysis state between calls; every call reopens the snapshot.
pub struct Service {
    state: StatePath,
    factories: Factories,
    busy_retry: Duration,
    sync_directory: SyncDirectory,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

// RPC decodes the proposal member separately, as raw JSON, so its failures
// report a location inside the proposal; see `rpc`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalRequest<P = Proposal> {
    pub proposal: P,
    #[serde(default, deserialize_with = "present")]
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
    #[serde(default, deserialize_with = "present")]
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
    #[serde(default, deserialize_with = "present")]
    pub kind: Option<EntryKind>,
    #[serde(default, rename = "type", deserialize_with = "present")]
    pub ty_name: Option<String>,
    #[serde(default)]
    pub currentness: Currentness,
    // Case-sensitive substring over node text values; edges have no payload.
    #[serde(default, deserialize_with = "present")]
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

// The complete context of one check: the exact base it was prepared against,
// the operation-to-entry mapping (provisional for previews), and the full
// unfiltered analysis of every prefix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalReport {
    pub database: Uuid,
    // Absent only for `init`, whose empty base was never published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<Uuid>,
    pub base: ViewId,
    pub operations: Vec<OperationEntry>,
    pub analysis: AnalysisOutput,
}

// A preview is a result whether or not it would be accepted; `apply` of the
// same proposal against the same base would reject it exactly when
// `accepted` is false.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub accepted: bool,
    pub report: ProposalReport,
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

// The result a mutation would have returned. An uncertain publication
// carries it, since the new state may already be visible. Tagged by `kind`,
// because an apply result and an added analyzer differ only by `suite`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Published {
    Initialized(Initialized),
    Applied(ApplyResult),
    AnalyzerAdded(AnalyzerAdded),
}

impl From<Initialized> for Published {
    fn from(result: Initialized) -> Self {
        Self::Initialized(result)
    }
}

impl From<ApplyResult> for Published {
    fn from(result: ApplyResult) -> Self {
        Self::Applied(result)
    }
}

impl From<AnalyzerAdded> for Published {
    fn from(result: AnalyzerAdded) -> Self {
        Self::AnalyzerAdded(result)
    }
}

fn settle<T: Into<Published>>(publication: Publication, result: T) -> Result<T, AppError> {
    match publication {
    Publication::Complete => Ok(result),
    Publication::Uncertain(message) => Err(AppError::PublicationUncertain {
        message,
        outcome: Box::new(result.into()),
    }),
    }
}

// A `ProposalReport` without its analysis, which is moved out of the checked
// plan once nothing else needs it, rather than copied.
struct ReportContext {
    database: Uuid,
    base_revision: Option<Uuid>,
    base: ViewId,
    operations: Vec<OperationEntry>,
}

impl ReportContext {
    fn report(self, analysis: AnalysisOutput) -> ProposalReport {
        ProposalReport {
            database: self.database,
            base_revision: self.base_revision,
            base: self.base,
            operations: self.operations,
            analysis,
        }
    }
}

// Commit an accepted plan into the loaded snapshot, returning whether
// graph/suite state changed. Every node, edge, or analyzer operation does and
// gets a fresh revision; an empty plan is a no-op that keeps it.
fn commit(
    snapshot: &mut Snapshot,
    checked: Checked,
    context: ReportContext,
) -> Result<(ProposalReport, bool), AppError> {
    let prepared = match checked {
    Checked::Accepted(prepared) => prepared,
    rejected => {
        let report = context.report(rejected.into_analysis());
        return Err(AppError::Rejected(Box::new(report)));
    }
    };
    let changed = !prepared.operations().is_empty();
    let analysis = prepared
        .commit(&mut snapshot.graph)
        .map_err(|error| preparation_error(error, None, 0))?;
    if changed {
        snapshot.graph_revision = Uuid::new_v4();
    }
    Ok((context.report(analysis), changed))
}

// A stored entry plus its currentness in the selected view. The core entry
// already serializes in proposal vocabulary, so the record is that entry's
// fields with `current` beside them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryRecord {
    #[serde(flatten)]
    pub entry: Entry,
    pub current: bool,
}

impl EntryRecord {
    fn new(view: View<'_>, entry: &Entry) -> Self {
        Self {
            entry: entry.clone(),
            current: view.is_stale(entry.id()) == Some(false),
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

// The host captures `now` once, before evaluation; neither runner nor
// analyzers read the clock.
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

// Translate core preparation failures into application errors, attaching the
// proposal entry when one is identifiable. Each proposal entry appends exactly
// one history entry, so a staged position maps back to its operation.
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
        Self {
            state,
            factories,
            busy_retry: storage::DEFAULT_BUSY_RETRY,
            sync_directory: storage::sync_directory,
        }
    }

    // How long a writer keeps retrying a held lock before reporting `busy`.
    pub fn with_busy_retry(self, busy_retry: Duration) -> Self {
        Self { busy_retry, ..self }
    }

    #[cfg(test)]
    pub(crate) fn with_sync_directory(self, sync_directory: SyncDirectory) -> Self {
        Self {
            sync_directory,
            ..self
        }
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

    // Stage and check changes against one loaded snapshot; nothing is stored.
    fn prepare(
        &self,
        snapshot: &Snapshot,
        proposal: Option<&Proposal>,
        changes: &[Change],
        inputs: EvaluationInputs,
    ) -> Result<(Checked, ReportContext), AppError> {
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
        let context = ReportContext {
            database: database(&snapshot.graph),
            base_revision: Some(snapshot.graph_revision),
            base: base.id(),
            operations,
        };
        Ok((checked, context))
    }

    fn prepare_proposal(
        &self,
        snapshot: &Snapshot,
        proposal: &Proposal,
        inputs: EvaluationInputs,
    ) -> Result<(Checked, ReportContext), AppError> {
        verify_plan_id(snapshot, proposal.plan_id)?;
        let changes = proposal
            .lower(snapshot.graph.view())
            .map_err(|(location, error)| AppError::InvalidProposal {
                location: Some(location),
                message: error.to_string(),
            })?
            .into_iter()
            .map(Change::from)
            .collect::<Vec<_>>();
        self.prepare(snapshot, Some(proposal), &changes, inputs)
    }

    // Publish a changed snapshot under the writer lock it was read with.
    fn publish(&self, locked: &LockedSnapshot, changed: bool) -> Result<Publication, AppError> {
        if !changed {
            return Ok(Publication::Complete);
        }
        locked.publish(self.sync_directory)
    }

    pub fn init(&self, suite: &[Config]) -> Result<Initialized, AppError> {
        storage::require_initializable(&self.state)?;
        let mut snapshot = Snapshot::new(LifeCoreList::default());
        let changes = suite
            .iter()
            .map(|config| Change::AddAnalyzer {
                name: config.name.clone(),
                config: config.config.clone(),
            })
            .collect::<Vec<_>>();
        // The initial suite is added through the ordinary checked path, so
        // missing factories and invalid configurations are rejected here too.
        let (checked, mut context) = self.prepare(&snapshot, None, &changes, Default::default())?;
        context.base_revision = None;
        commit(&mut snapshot, checked, context)?;
        let lock = storage::lock(&self.state, self.busy_retry)?;
        let publication = lock.publish_new(&snapshot, self.sync_directory)?;
        let initialized = Initialized {
            path: self.state.path().to_string_lossy().into_owned(),
            database: database(&snapshot.graph),
            graph_revision: snapshot.graph_revision,
            suite: snapshot.graph.analysis_suite().clone(),
        };
        settle(publication, initialized)
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
        // Entry collections use append order.
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

    // A preview: it reads one snapshot without the writer lock and stores
    // nothing. A proposal that `apply` would reject is still a result.
    pub fn check(&self, request: ProposalRequest) -> Result<CheckResult, AppError> {
        if request.proposal.acknowledges.is_some() {
            return Err(AppError::AcknowledgmentUnsupported);
        }
        let inputs = evaluation_inputs(request.evaluation_time, request.now)?;
        let snapshot = storage::read(&self.state)?;
        let (checked, context) = self.prepare_proposal(&snapshot, &request.proposal, inputs)?;
        Ok(CheckResult {
            accepted: matches!(checked, Checked::Accepted(_)),
            report: context.report(checked.into_analysis()),
        })
    }

    // Always checks anew under writer exclusion, from reload through
    // publication, so the concrete base cannot change underneath it.
    pub fn apply(&self, request: ProposalRequest) -> Result<ApplyResult, AppError> {
        // TODO: verify `acknowledges` by replay (PLAN 4.4). Until then it is
        // rejected rather than silently ignored.
        if request.proposal.acknowledges.is_some() {
            return Err(AppError::AcknowledgmentUnsupported);
        }
        let inputs = evaluation_inputs(request.evaluation_time, request.now)?;
        storage::require_existing(&self.state)?;
        let mut locked = storage::lock(&self.state, self.busy_retry)?.read()?;
        let (checked, context) =
            self.prepare_proposal(locked.snapshot(), &request.proposal, inputs)?;
        let (report, changed) = commit(locked.snapshot_mut(), checked, context)?;
        let publication = self.publish(&locked, changed)?;
        let applied = ApplyResult {
            graph_revision: locked.snapshot().graph_revision,
            report,
        };
        settle(publication, applied)
    }

    // Appends to the ordered suite through a one-operation transaction, so the
    // full replay under the extended suite must pass for every prefix.
    pub fn add_analyzer(&self, request: AddAnalyzerRequest) -> Result<AnalyzerAdded, AppError> {
        let inputs = evaluation_inputs(request.evaluation_time, request.now)?;
        storage::require_existing(&self.state)?;
        let mut locked = storage::lock(&self.state, self.busy_retry)?.read()?;
        let changes = [Change::AddAnalyzer {
            name: request.name,
            config: request.configuration,
        }];
        let (checked, context) = self.prepare(locked.snapshot(), None, &changes, inputs)?;
        let (report, changed) = commit(locked.snapshot_mut(), checked, context)?;
        let publication = self.publish(&locked, changed)?;
        let snapshot = locked.snapshot();
        let added = AnalyzerAdded {
            graph_revision: snapshot.graph_revision,
            suite: snapshot.graph.analysis_suite().clone(),
            report,
        };
        settle(publication, added)
    }
}
