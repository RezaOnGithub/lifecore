use crate::{
    AppError,
    proposal::{PlanId, Proposal, ProposalLocation, present},
    query::{self, EdgeFilter, EntryInspection, Expansion, Seed, ViewQuery},
    render::{self, RenderFormat, Rendered},
    schema::{self, TypeList},
    storage::{self, LockedSnapshot, Publication, Snapshot, StatePath, SyncDirectory},
};
use lifecore::{
    BasicValidationFailure, Edge, Entry, EntryId, LifeCoreError, LifeCoreList, Node, Timestamp,
    View, ViewError, ViewId,
    analysis::{
        AnalysisOutput, AnalysisRunner, AnalyzerFactory, Config, EvaluationInputs, FindingKey,
        MessageType, SuiteConfig,
        presentation::{CannotDismissInconsistency, Dismissal, DismissalReason},
    },
    inspection::{Currentness, EdgeSelection, NodeSelection},
    transaction::{self, Change, Checked},
};
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::{BTreeSet, HashSet},
    time::Duration,
};
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
    #[serde(default, deserialize_with = "present")]
    pub ty_name: Option<String>,
    #[serde(default)]
    pub currentness: Currentness,
    // Case-sensitive substring over node text values; edges have no payload.
    #[serde(default, deserialize_with = "present")]
    pub text: Option<String>,
    #[serde(default, deserialize_with = "boundary")]
    pub as_of: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryGetRequest {
    pub id: EntryId,
    #[serde(default, deserialize_with = "boundary")]
    pub as_of: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryInspectRequest {
    pub id: EntryId,
    #[serde(default, deserialize_with = "boundary")]
    pub as_of: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeListRequest {
    #[serde(default, deserialize_with = "boundary")]
    pub as_of: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewQueryRequest {
    pub seeds: Vec<Seed>,
    #[serde(default)]
    pub expand: Vec<Expansion>,
    #[serde(default)]
    pub edges: EdgeFilter,
    #[serde(default, deserialize_with = "boundary")]
    pub as_of: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisRunRequest {
    #[serde(default, deserialize_with = "boundary")]
    pub as_of: Option<usize>,
    #[serde(default, deserialize_with = "present")]
    pub evaluation_time: Option<Timestamp>,
    #[serde(default)]
    pub now: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DismissRequest {
    pub key: FindingKey,
    pub reason: DismissalReason,
    #[serde(default, deserialize_with = "present")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    pub key: FindingKey,
}

// The `views.query` parameters as one member, since Serde cannot deny
// unknown fields through a flattened one.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewRenderRequest {
    pub query: ViewQueryRequest,
    pub format: RenderFormat,
}

// An optional boundary is a decimal string like every position, and null is
// not absence.
fn boundary<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<usize>, D::Error> {
    lifecore::decimal::deserialize(deserializer).map(Some)
}

// The prefix a read answers from: the whole history unless `as_of` narrows it.
fn as_of(graph: &LifeCoreList, end_exclusive: Option<usize>) -> Result<View<'_>, AppError> {
    let view = graph.view();
    let Some(end_exclusive) = end_exclusive else {
        return Ok(view);
    };
    view.prefix(end_exclusive).map_err(|error| match error {
    ViewError::BeyondBoundary { requested, visible } => {
        AppError::BeyondBoundary { requested, visible }
    }
    other => AppError::Internal {
        message: other.to_string(),
    },
    })
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
    pub registered: Vec<String>,
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

// A replayed analysis beside the stored dismissal records, from the same
// snapshot read, of the findings it emitted; presentation applies them, the
// analysis never does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisRun {
    pub analysis: AnalysisOutput,
    pub dismissals: Vec<Dismissal>,
}

// Every stored dismissal record after an edit, in storage order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DismissalList {
    pub dismissals: Vec<Dismissal>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreResult {
    pub dismissals: Vec<Dismissal>,
    pub restored: bool,
}

// The result a mutation would have returned. An uncertain publication
// carries it, since the new state may already be visible. Tagged by `kind`,
// because an apply result and an added analyzer differ only by `suite`, and
// a dismissal and a restore only by `restored`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Published {
    Initialized(Initialized),
    Applied(ApplyResult),
    AnalyzerAdded(AnalyzerAdded),
    Dismissed(DismissalList),
    Restored(RestoreResult),
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

impl From<DismissalList> for Published {
    fn from(result: DismissalList) -> Self {
        Self::Dismissed(result)
    }
}

impl From<RestoreResult> for Published {
    fn from(result: RestoreResult) -> Self {
        Self::Restored(result)
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
    fn owned(view: View<'_>, entry: Entry) -> Self {
        Self {
            current: view.is_stale(entry.id()) == Some(false),
            entry,
        }
    }
    pub(crate) fn new(view: View<'_>, entry: &Entry) -> Self {
        Self::owned(view, entry.clone())
    }
    pub(crate) fn node(view: View<'_>, node: &Node) -> Self {
        Self::owned(view, Entry::Node(node.clone()))
    }
    pub(crate) fn edge(view: View<'_>, edge: &Edge) -> Self {
        Self::owned(view, Entry::Edge(edge.clone()))
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

fn dismissals(snapshot: &Snapshot) -> Vec<Dismissal> {
    snapshot.dismissals.records().cloned().collect()
}

fn verify_plan_id(snapshot: &Snapshot, plan_id: Option<PlanId>) -> Result<(), AppError> {
    let Some(plan_id) = plan_id else {
        return Ok(());
    };
    let actual = database(&snapshot.graph);
    if plan_id.database != actual {
        return Err(AppError::ForeignDatabase {
            named_by: "plan ID",
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
                configuration: config.configuration.clone(),
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
            registered: self.registered(),
        })
    }

    // The analyzer names this host can add or replay, sorted, so a client
    // never hardcodes them. A suite entry outside it stays readable and fails
    // only when an analysis needs it.
    fn registered(&self) -> Vec<String> {
        (self.factories)()
            .iter()
            .map(|factory| factory.analyzer_name().to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn list_entries(&self, request: EntryListRequest) -> Result<EntryList, AppError> {
        let snapshot = storage::read(&self.state)?;
        let view = as_of(&snapshot.graph, request.as_of)?;
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
        let view = as_of(&snapshot.graph, request.as_of)?;
        let entry = view
            .get(request.id)
            .ok_or(AppError::EntryNotFound { id: request.id })?;
        Ok(EntryLookup {
            view: view.id(),
            entry: EntryRecord::new(view, entry),
        })
    }

    pub fn inspect_entry(&self, request: EntryInspectRequest) -> Result<EntryInspection, AppError> {
        let snapshot = storage::read(&self.state)?;
        query::inspect(as_of(&snapshot.graph, request.as_of)?, request.id)
    }

    pub fn list_types(&self, request: TypeListRequest) -> Result<TypeList, AppError> {
        let snapshot = storage::read(&self.state)?;
        Ok(schema::list_types(as_of(&snapshot.graph, request.as_of)?))
    }

    pub fn query_view(&self, request: ViewQueryRequest) -> Result<ViewQuery, AppError> {
        let snapshot = storage::read(&self.state)?;
        query::query(
            as_of(&snapshot.graph, request.as_of)?,
            &request.seeds,
            &request.expand,
            &request.edges,
        )
    }

    pub fn render_view(&self, request: ViewRenderRequest) -> Result<Rendered, AppError> {
        render::render(&self.query_view(request.query)?, request.format)
    }

    // A replay of the selected prefix with fresh analyzers; nothing is
    // stored, and the records come from the same snapshot read.
    pub fn run_analysis(&self, request: AnalysisRunRequest) -> Result<AnalysisRun, AppError> {
        let inputs = evaluation_inputs(request.evaluation_time, request.now)?;
        let snapshot = storage::read(&self.state)?;
        let view = as_of(&snapshot.graph, request.as_of)?;
        // The ordered suite is not versioned by history (INTERFACES 4), so a
        // prefix replays under the suite stored now, whatever was in force
        // when its entries were appended.
        let analysis = self
            .runner(inputs)?
            .run_view(view, snapshot.graph.analysis_suite())
            .map_err(AppError::Analyzer)?;
        // A record of a key first emitted past the boundary would reveal that
        // later history, so only records whose complete key this output
        // emitted are returned.
        let emitted = analysis
            .observations()
            .map(|observation| &observation.key)
            .collect::<HashSet<_>>();
        let dismissals = snapshot
            .dismissals
            .records()
            .filter(|record| emitted.contains(&record.key))
            .cloned()
            .collect();
        Ok(AnalysisRun {
            analysis,
            dismissals,
        })
    }

    // Dismissal edits reread under writer exclusion and publish with the
    // graph untouched, so the revision stays (INTERFACES 4).
    pub fn dismiss_finding(&self, request: DismissRequest) -> Result<DismissalList, AppError> {
        if request.key.kind == MessageType::Inconsistency {
            return Err(AppError::CannotDismissInconsistency {
                key: Box::new(request.key),
            });
        }
        storage::require_existing(&self.state)?;
        let mut locked = storage::lock(&self.state, self.busy_retry)?.read()?;
        let selected = database(&locked.snapshot().graph);
        if request.key.database != selected {
            return Err(AppError::ForeignDatabase {
                named_by: "finding key",
                expected: request.key.database,
                actual: selected,
            });
        }
        let records = &mut locked.snapshot_mut().dismissals;
        // Rewriting a record with its own reason and note publishes nothing.
        let changed = records
            .get(&request.key)
            .is_none_or(|record| (record.reason, &record.note) != (request.reason, &request.note));
        records
            .dismiss(request.key.clone(), request.reason, request.note)
            .map_err(
                |CannotDismissInconsistency| AppError::CannotDismissInconsistency {
                    key: Box::new(request.key),
                },
            )?;
        let publication = self.publish(&locked, changed)?;
        let list = DismissalList {
            dismissals: dismissals(locked.snapshot()),
        };
        settle(publication, list)
    }

    // Dismissal refuses keys of other databases, so restoring one matches no
    // record and is a no-op like any other unmatched key.
    pub fn restore_finding(&self, request: RestoreRequest) -> Result<RestoreResult, AppError> {
        storage::require_existing(&self.state)?;
        let mut locked = storage::lock(&self.state, self.busy_retry)?.read()?;
        let restored = locked.snapshot_mut().dismissals.restore(&request.key);
        // Nothing is published when no record matched.
        let publication = self.publish(&locked, restored)?;
        let result = RestoreResult {
            dismissals: dismissals(locked.snapshot()),
            restored,
        };
        settle(publication, result)
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
            configuration: request.configuration,
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
