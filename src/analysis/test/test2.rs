use crate::{
    Body, DiagnosticLocation, EntryId, LifeCoreError, LifeCoreList, View, ViewId,
    analysis::{
        AnalysisRunner, Analyzer, AnalyzerFactory, AnalyzerIdentity, Config, EvaluationInputs,
        Fact, InnerMessageType, InnerOutput, Message, MessageKind, MessageType, ObservationScope,
        Output, SuiteConfig, TypedAnalyzer,
    },
    transaction::{Change, prepare},
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Finding {
    location: DiagnosticLocation,
}
impl InnerMessageType for Finding {
    fn ty(&self) -> MessageType {
        MessageType::Inconsistency
    }
}
#[derive(Clone)]
struct Report(Vec<Finding>);
impl InnerOutput<Finding> for Report {
    fn push(&mut self, value: Finding) {
        self.0.push(value);
    }
}

struct Probe {
    observed: Arc<Mutex<Vec<ViewId>>>,
    future: EntryId,
    reject: Option<usize>,
}
impl TypedAnalyzer for Probe {
    type MSG = Finding;
    type OUTPUT = Report;
    fn identity(&self) -> AnalyzerIdentity {
        AnalyzerIdentity {
            name: "Probe".into(),
            version: "1".into(),
            configuration: Fact::from_typed(&self.reject).unwrap(),
        }
    }
    fn initialize_inner(&mut self, _inputs: &EvaluationInputs) -> Result<Report, String> {
        Ok(Report(vec![]))
    }
    fn analyze_inner(&self, view: View<'_>, _inputs: &EvaluationInputs) -> Result<Report, String> {
        self.observed.lock().unwrap().push(view.id());
        assert!(view.prefix(view.len() + 1).is_err());
        if self.future.position() >= view.len() {
            assert!(view.get(self.future).is_none());
        }
        assert_eq!(view.entries().count(), view.len());
        assert!(view.entries().all(|e| e.id().position() < view.len()));
        Ok(Report(if self.reject == Some(view.len()) {
            vec![Finding {
                location: view.location(None).unwrap(),
            }]
        } else {
            vec![]
        }))
    }
    fn convert_report(&self, report: Report) -> Output {
        Output(
            report
                .0
                .into_iter()
                .map(|finding| Message {
                    finding: crate::analysis::Finding {
                        code: "rejected_prefix".into(),
                        subjects: Default::default(),
                        facts: Fact::from_typed(&finding.location.view).unwrap(),
                    },
                    offending_event: Some(finding.location),
                    relevance: Default::default(),
                    message: "rejected prefix".into(),
                    kind: MessageKind::Inconsistency,
                    scope: ObservationScope::State,
                })
                .collect(),
        )
    }
}
struct Factory {
    observed: Arc<Mutex<Vec<ViewId>>>,
    future: EntryId,
}
impl AnalyzerFactory for Factory {
    fn analyzer_name(&self) -> &'static str {
        "Probe"
    }
    fn analyzer_with_config(&mut self, config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        let reject = if config.is_empty() {
            None
        } else {
            Some(
                config
                    .parse()
                    .map_err(|_| LifeCoreError::InvalidAnalyzerConfiguration {
                        analyzer_name: "Probe".into(),
                        message: "expected a boundary".into(),
                    })?,
            )
        };
        Ok(Box::new(Probe {
            observed: self.observed.clone(),
            future: self.future,
            reject,
        }))
    }
}
fn fixture() -> (LifeCoreList, EntryId) {
    let mut graph = LifeCoreList::default();
    let changes = ["A", "B", "C"].map(|name| Change::AddNode {
        ty_name: name.into(),
        body: Body::new(),
        parent: None,
    });
    let plan = prepare(&graph, &changes, &mut AnalysisRunner::default()).unwrap();
    let future = plan.added()[2];
    plan.commit(&mut graph).unwrap();
    (graph, future)
}
fn suite(config: &str) -> SuiteConfig {
    SuiteConfig(vec![Config {
        name: "Probe".into(),
        config: config.into(),
    }])
}

#[test]
fn every_prefix_including_empty_is_read_only_and_cannot_expand() {
    let (graph, future) = fixture();
    let observed = Arc::new(Mutex::new(vec![]));
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(Factory {
            observed: observed.clone(),
            future,
        }))
        .unwrap();
    assert!(
        runner
            .run_view(graph.view(), &suite(""))
            .unwrap()
            .observations()
            .next()
            .is_none()
    );
    assert_eq!(
        observed
            .lock()
            .unwrap()
            .iter()
            .map(|v| v.end)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    observed.lock().unwrap().clear();
    runner
        .run_view(graph.view().prefix(1).unwrap(), &suite(""))
        .unwrap();
    assert_eq!(
        observed
            .lock()
            .unwrap()
            .iter()
            .map(|v| v.end)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn changed_analyzer_rejects_an_old_prefix_and_does_not_only_check_the_head() {
    let (graph, future) = fixture();
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(Factory {
            observed: Arc::default(),
            future,
        }))
        .unwrap();
    let report = runner.run_view(graph.view(), &suite("1")).unwrap();
    assert_eq!(
        report
            .errors()
            .map(|e| e.diagnostic.offending_event.unwrap().view.end)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert!(report.into_result().is_err());
    assert!(
        AnalysisRunner::default()
            .run(&graph)
            .unwrap()
            .into_result()
            .is_ok()
    );
}

#[test]
fn missing_duplicate_and_invalid_factories_are_typed_errors() {
    let (graph, future) = fixture();
    let mut runner = AnalysisRunner::default();
    assert!(matches!(
        runner.run_view(graph.view(), &suite("")),
        Err(LifeCoreError::AnalyzerNotFound { .. })
    ));
    runner
        .register_analyzer_kind(Box::new(Factory {
            observed: Arc::default(),
            future,
        }))
        .unwrap();
    assert!(matches!(
        runner.register_analyzer_kind(Box::new(Factory {
            observed: Arc::default(),
            future
        })),
        Err(LifeCoreError::DuplicateAnalyzer { .. })
    ));
    assert!(matches!(
        runner.run_view(graph.view(), &suite("bad")),
        Err(LifeCoreError::InvalidAnalyzerConfiguration { .. })
    ));
}

use crate::analysis::presentation::{
    AdvisoryFilter, DismissalStore, Dismissals, ReportOptions, ReportSelection,
};
use crate::analysis::{
    Finding as SemanticFinding, Invocation, Observation, RevisionIntent, SuggestedAction,
};
use crate::transaction::Reference::{Added, Existing};
use std::collections::{BTreeMap, BTreeSet};

type InvocationLog = Arc<Mutex<Vec<(i64, Option<usize>)>>>;

#[derive(Clone)]
struct AdvisoryFactory {
    version: &'static str,
    fail: Option<Invocation>,
    invalid: Option<DiagnosticLocation>,
    log: InvocationLog,
}

struct Advisories {
    factory: AdvisoryFactory,
    config: i64,
}

impl AnalyzerFactory for AdvisoryFactory {
    fn analyzer_name(&self) -> &'static str {
        "Advisories"
    }
    fn analyzer_with_config(&mut self, config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        let config =
            config
                .trim()
                .parse()
                .map_err(|_| LifeCoreError::InvalidAnalyzerConfiguration {
                    analyzer_name: "Advisories".into(),
                    message: "expected an integer".into(),
                })?;
        Ok(Box::new(Advisories {
            factory: self.clone(),
            config,
        }))
    }
}

impl Analyzer for Advisories {
    fn identity(&self) -> AnalyzerIdentity {
        AnalyzerIdentity {
            name: "Advisories".into(),
            version: self.factory.version.into(),
            configuration: Fact::Integer(self.config),
        }
    }
    fn initialize(&mut self, _inputs: &EvaluationInputs) -> Result<Output, String> {
        self.factory.log.lock().unwrap().push((self.config, None));
        if self.factory.fail == Some(Invocation::Initialization) {
            return Err("setup failed".into());
        }
        Ok(Output(vec![Message {
            finding: SemanticFinding {
                code: "setup".into(),
                subjects: BTreeSet::new(),
                facts: Fact::Null,
            },
            offending_event: self.factory.invalid,
            relevance: BTreeSet::new(),
            message: "configured".into(),
            kind: MessageKind::Warning,
            scope: ObservationScope::State,
        }]))
    }
    fn analyze(&self, view: View<'_>, inputs: &EvaluationInputs) -> Result<Output, String> {
        self.factory
            .log
            .lock()
            .unwrap()
            .push((self.config, Some(view.len())));
        if self.factory.fail == Some(Invocation::Prefix(view.id())) {
            return Err("prefix failed".into());
        }
        let mut output = Output::default();
        if let Some(node) = view.nodes().next()
            && view.is_stale(node.id) == Some(false)
        {
            let message = Message {
                finding: SemanticFinding {
                    code: "missing_summary".into(),
                    subjects: BTreeSet::from([node.id]),
                    facts: Fact::Record(BTreeMap::from([(
                        "field".into(),
                        Fact::Text("summary".into()),
                    )])),
                },
                offending_event: view.location(Some(node.id)),
                relevance: BTreeSet::from([node.id]),
                message: format!("Missing summary at {} with {:?}", view.len(), inputs.values),
                kind: MessageKind::Warning,
                scope: ObservationScope::State,
            };
            output.0.extend([message.clone(), message]);
        }
        if let Some(child) = view.entries().next_back().and_then(|entry| entry.as_node())
            && let Some(parent) = child.parent
        {
            output.0.push(Message {
                finding: SemanticFinding {
                    code: "review_connections".into(),
                    subjects: BTreeSet::from([parent, child.id]),
                    facts: Fact::Null,
                },
                offending_event: view.location(Some(child.id)),
                relevance: BTreeSet::from([parent, child.id]),
                message: "Decide which relationships this successor should retain".into(),
                kind: MessageKind::Suggestion(vec![SuggestedAction::AddNode {
                    ty_name: "Review".into(),
                    payload: Body::new(),
                    parent: None,
                    intent: None,
                }]),
                scope: ObservationScope::Event,
            });
        }
        Ok(output)
    }
}

fn advisory_factory() -> AdvisoryFactory {
    AdvisoryFactory {
        version: "1",
        fail: None,
        invalid: None,
        log: Arc::default(),
    }
}
fn advisory_suite(configs: &[&str]) -> SuiteConfig {
    SuiteConfig(
        configs
            .iter()
            .map(|config| Config {
                name: "Advisories".into(),
                config: (*config).into(),
            })
            .collect(),
    )
}
fn advisory_runner(factory: AdvisoryFactory, inputs: EvaluationInputs) -> AnalysisRunner {
    let mut runner = AnalysisRunner::new(inputs);
    runner.register_analyzer_kind(Box::new(factory)).unwrap();
    runner
}
fn history_options() -> ReportOptions {
    ReportOptions {
        selection: ReportSelection::History,
        ..Default::default()
    }
}

#[test]
fn observations_deduplicate_separately_from_findings_without_skipping_replay() {
    let (graph, _) = fixture();
    let factory = advisory_factory();
    let log = factory.log.clone();
    let mut runner = advisory_runner(factory, EvaluationInputs::default());
    let report = runner
        .run_view(graph.view(), &advisory_suite(&["7", " 007 "]))
        .unwrap();
    assert_eq!(report.observations().count(), 4);
    let dismissals = Dismissals::default();
    let groups = report.groups(&dismissals, ReportOptions::default());
    assert_eq!(groups.len(), 2);
    let warning = groups
        .iter()
        .find(|g| g.key.finding.code == "missing_summary")
        .unwrap();
    assert_eq!(warning.observations.len(), 3);
    assert_eq!(warning.selected.len(), 1);
    assert_eq!(
        warning.selected[0].invocation,
        Invocation::Prefix(graph.view().id())
    );
    assert_eq!(
        log.lock().unwrap().as_slice(),
        &[
            (7, None),
            (7, None),
            (7, Some(0)),
            (7, Some(1)),
            (7, Some(2)),
            (7, Some(3)),
            (7, Some(0)),
            (7, Some(1)),
            (7, Some(2)),
            (7, Some(3))
        ]
    );
    let reloaded: crate::analysis::Report =
        serde_json::from_slice(&serde_json::to_vec(&report).unwrap()).unwrap();
    assert_eq!(reloaded, report);
    assert_eq!(
        reloaded.render(&dismissals, history_options()),
        report.render(&dismissals, history_options())
    );
}

#[test]
fn dismissal_identity_ignores_wording_and_ordinary_evaluation_inputs_but_distinguishes_semantic_changes()
 {
    let (graph, _) = fixture();
    let mut runner = advisory_runner(advisory_factory(), EvaluationInputs::default());
    let report = runner
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    let key = report
        .warnings()
        .find(|o| o.key.finding.code == "missing_summary")
        .unwrap()
        .key
        .clone();
    let mut dismissals = Dismissals::default();
    dismissals
        .dismiss(key.clone(), "intentional".into())
        .unwrap();
    let inputs = EvaluationInputs {
        values: Body::from([(
            "evaluation_time".into(),
            "2026-09-14T12:00:00.123456789Z"
                .parse::<crate::Timestamp>()
                .unwrap()
                .into(),
        )]),
    };
    let mut runner = advisory_runner(advisory_factory(), inputs.clone());
    let replay = runner
        .run_view(graph.view(), &advisory_suite(&[" 007 "]))
        .unwrap();
    let observation = replay
        .warnings()
        .find(|o| o.key.finding.code == "missing_summary")
        .unwrap();
    assert_eq!(observation.key, key);
    assert_eq!(observation.inputs, inputs);
    assert_ne!(
        observation.diagnostic.message,
        report
            .warnings()
            .find(|o| o.key == key)
            .unwrap()
            .diagnostic
            .message
    );
    assert!(
        replay
            .groups(&dismissals, history_options())
            .iter()
            .all(|g| g.key != &key)
    );
    let altered = runner
        .run_view(graph.view(), &advisory_suite(&["8"]))
        .unwrap();
    assert!(
        altered
            .groups(&dismissals, ReportOptions::default())
            .iter()
            .any(|g| g.key.finding.code == "missing_summary")
    );
    let mut factory = advisory_factory();
    factory.version = "2";
    let newer = advisory_runner(factory, inputs)
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    assert!(
        newer
            .groups(&dismissals, ReportOptions::default())
            .iter()
            .any(|g| g.key.finding.code == "missing_summary")
    );
    let mut subject = key.clone();
    subject.finding.subjects = BTreeSet::from([graph.view().at(1).unwrap().id()]);
    let mut facts = key.clone();
    facts.finding.facts = Fact::Text("different field".into());
    assert!(dismissals.get(&subject).is_none());
    assert!(dismissals.get(&facts).is_none());
    for different in [subject, facts] {
        assert_ne!(different.encoding(), key.encoding());
    }
}

#[test]
fn dismiss_restore_survives_files_reload_and_absent_current_emission() {
    let (mut graph, _) = fixture();
    let mut runner = advisory_runner(advisory_factory(), EvaluationInputs::default());
    let report = runner
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    let key = report
        .warnings()
        .find(|o| o.key.finding.code == "missing_summary")
        .unwrap()
        .key
        .clone();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("dismissals.json");
    let store = DismissalStore::new(&path);
    store.dismiss(key.clone(), "known".into()).unwrap();
    let root = graph.view().nodes().next().unwrap().id;
    prepare(
        &graph,
        &[Change::AddNode {
            ty_name: "Revised".into(),
            body: Body::new(),
            parent: Some(Existing(root)),
        }],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .commit(&mut graph)
    .unwrap();
    let report = runner
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    let restored_store = DismissalStore::new(&path);
    let dismissals = restored_store.load().unwrap();
    assert_eq!(dismissals.get(&key).unwrap().reason, "known");
    assert!(
        report
            .groups(&dismissals, history_options())
            .iter()
            .all(|g| g.key != &key)
    );
    assert!(restored_store.restore(&key).unwrap());
    assert!(!restored_store.restore(&key).unwrap());
    let dismissals = store.load().unwrap();
    assert!(
        report
            .groups(&dismissals, ReportOptions::default())
            .iter()
            .all(|g| g.key != &key)
    );
    assert_eq!(
        report
            .groups(&dismissals, history_options())
            .iter()
            .find(|g| g.key == &key)
            .unwrap()
            .observations
            .len(),
        3
    );
    assert!(
        !report
            .render(&dismissals, history_options())
            .contains("resolved")
    );
    let snapshot = std::fs::read(&path).unwrap();
    let mut blocking = key;
    blocking.kind = MessageType::Inconsistency;
    assert!(store.dismiss(blocking, "cannot hide".into()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), snapshot);
}

#[test]
fn transition_events_remain_visible_after_unrelated_appends_and_partial_split_reconnection() {
    let mut graph = LifeCoreList::default();
    let node = |name: &str, parent| Change::AddNode {
        ty_name: name.into(),
        body: Body::new(),
        parent,
    };
    let plan = prepare(
        &graph,
        &[
            node("A", None),
            node("B", None),
            Change::AddEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            },
            node("A1", Some(Added(0))),
            node("A2", Some(Added(0))),
            Change::AddEdge {
                from: Added(3),
                ty_name: "link".into(),
                to: Added(1),
            },
            node("Unrelated", None),
        ],
        &mut AnalysisRunner::default(),
    )
    .unwrap();
    let ids = plan.added().to_vec();
    plan.commit(&mut graph).unwrap();
    let mut runner = advisory_runner(advisory_factory(), EvaluationInputs::default());
    for end in [4, 5, 6, 7] {
        let at = graph.view().prefix(end).unwrap();
        let report = runner.run_view(at, &advisory_suite(&["7"])).unwrap();
        let dismissals = Dismissals::default();
        let groups = report.groups(&dismissals, ReportOptions::default());
        let events: Vec<_> = groups
            .iter()
            .filter(|g| g.key.kind == MessageType::Suggestion)
            .collect();
        assert_eq!(events.len(), if end == 4 { 1 } else { 2 });
        assert_eq!(
            events[0].observations[0].invocation,
            Invocation::Prefix(graph.view().prefix(4).unwrap().id())
        );
        if end > 4 {
            assert!(
                report
                    .render(&dismissals, ReportOptions::default())
                    .contains("current applicability unconfirmed")
            );
        }
    }
    assert_eq!(
        graph
            .view()
            .fresh_edges()
            .map(|edge| edge.from)
            .collect::<Vec<_>>(),
        vec![ids[3]]
    );
    assert!(graph.view().fresh_nodes().any(|node| node.id == ids[4]));
}

#[test]
fn display_and_even_imported_dismissals_cannot_hide_historical_inconsistencies() {
    let (graph, future) = fixture();
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(Factory {
            observed: Arc::default(),
            future,
        }))
        .unwrap();
    let report = runner.run_view(graph.view(), &suite("1")).unwrap();
    let key = report.errors().next().unwrap().key.clone();
    let mut dismissals = Dismissals::default();
    assert!(dismissals.dismiss(key.clone(), "no".into()).is_err());
    let imported: Dismissals = serde_json::from_value(
        serde_json::json!({"records": [{"key": key, "reason": "invalid imported dismissal"}]}),
    )
    .unwrap();
    let groups = report.groups(
        &imported,
        ReportOptions {
            advisories: AdvisoryFilter::None,
            ..Default::default()
        },
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].selected[0].invocation,
        Invocation::Prefix(graph.view().prefix(1).unwrap().id())
    );
    assert!(report.into_result().is_err());
}

#[test]
fn execution_failures_record_inputs_and_prefix_and_never_publish_a_candidate() {
    let (graph, _) = fixture();
    let inputs = EvaluationInputs {
        values: Body::from([("rule_time".into(), 99.into())]),
    };
    for at in [
        Invocation::Initialization,
        Invocation::Prefix(graph.view().prefix(1).unwrap().id()),
    ] {
        let mut factory = advisory_factory();
        factory.fail = Some(at);
        let mut runner = advisory_runner(factory, inputs.clone());
        let error = match prepare(
            &graph,
            &[
                Change::AddAnalyzer {
                    name: "Advisories".into(),
                    config: "7".into(),
                },
                Change::AddNode {
                    ty_name: "Candidate".into(),
                    body: Body::new(),
                    parent: None,
                },
            ],
            &mut runner,
        ) {
        Ok(_) => panic!("execution failure must reject preparation"),
        Err(crate::transaction::Error::Core(LifeCoreError::AnalyzerExecution(error))) => error,
        Err(error) => panic!("unexpected error: {error}"),
        };
        assert_eq!(error.invocation, at);
        assert_eq!(error.inputs, inputs);
        assert_eq!(error.analyzer.version, "1");
        assert_eq!(graph.view().len(), 3);
    }
}

#[test]
fn initialization_cannot_claim_graph_locations_and_empty_history_still_executes() {
    let (graph, _) = fixture();
    let mut factory = advisory_factory();
    factory.invalid = graph.view().location(None);
    assert!(matches!(
        advisory_runner(factory, EvaluationInputs::default())
            .run_view(graph.view(), &advisory_suite(&["7"])),
        Err(LifeCoreError::InvalidAnalyzerOutput { .. })
    ));
    let factory = advisory_factory();
    let log = factory.log.clone();
    let empty = LifeCoreList::default();
    let report = advisory_runner(factory, EvaluationInputs::default())
        .run_view(empty.view(), &advisory_suite(&["7"]))
        .unwrap();
    assert_eq!(report.observations().count(), 1);
    assert_eq!(*log.lock().unwrap(), vec![(7, None), (7, Some(0))]);
}

#[derive(Default)]
struct CollidingHasher;
impl std::hash::Hasher for CollidingHasher {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, _bytes: &[u8]) {}
}

#[test]
fn hash_collisions_do_not_merge_findings_and_key_encoding_is_deterministic() {
    let (graph, _) = fixture();
    let report = advisory_runner(advisory_factory(), EvaluationInputs::default())
        .run_view(graph.view(), &advisory_suite(&["7", "8"]))
        .unwrap();
    let dismissals = Dismissals::default();
    let groups = report.groups_with_hasher(
        &dismissals,
        history_options(),
        std::hash::BuildHasherDefault::<CollidingHasher>::default(),
    );
    assert_eq!(groups.len(), 4);
    let key = groups[1].key;
    let decoded = serde_json::from_slice::<crate::analysis::FindingKey>(
        &key.encoding()[b"lifecore.finding.v1\0".len()..],
    )
    .unwrap();
    assert_eq!(key, &decoded);
    assert_eq!(key.encoding(), decoded.encoding());
    assert_eq!(key.digest(), decoded.digest());
    let one = Fact::from_typed(&BTreeMap::from([("a", 1), ("b", 2)])).unwrap();
    let two = Fact::from_typed(&BTreeMap::from([("b", 2), ("a", 1)])).unwrap();
    assert_eq!(one, two);
    assert!(Fact::from_typed(&1.5).is_err());
}

fn repair_observation(view: View<'_>, actions: Vec<SuggestedAction>) -> Observation {
    let finding = SemanticFinding {
        code: "explicit_repair".into(),
        subjects: BTreeSet::new(),
        facts: Fact::Null,
    };
    Observation {
        key: crate::analysis::FindingKey {
            database: view.id().database,
            analyzer: AnalyzerIdentity {
                name: "RepairFixture".into(),
                version: "1".into(),
                configuration: Fact::Null,
            },
            kind: MessageType::Suggestion,
            finding: finding.clone(),
        },
        inputs: EvaluationInputs::default(),
        invocation: Invocation::Prefix(view.id()),
        diagnostic: Message {
            finding,
            offending_event: view.location(None),
            relevance: BTreeSet::new(),
            message: "Explicit repair".into(),
            kind: MessageKind::Suggestion(actions),
            scope: ObservationScope::Event,
        },
    }
}

#[test]
fn explicit_repair_exports_the_checked_order_payloads_and_exact_references_as_toml() {
    use crate::analysis::repair;
    let (graph, _) = fixture();
    let root = graph.view().at(0).unwrap().id();
    let target = graph.view().at(1).unwrap().id();
    let timestamp: crate::Timestamp = "2026-09-14T15:00:00.123456789+03:00".parse().unwrap();
    let body = Body::from([
        ("title".into(), "quote\" slash\\ newline\n".into()),
        ("at".into(), timestamp.into()),
        ("literal".into(), "2026-09-14T12:00:00Z".into()),
        ("count".into(), (-99).into()),
        ("done".into(), false.into()),
        (
            "rows".into(),
            crate::Payload::List(vec![crate::Payload::Record(Body::new())]),
        ),
    ]);
    let observation = repair_observation(
        graph.view(),
        vec![
            SuggestedAction::AddNode {
                ty_name: "Repaired".into(),
                payload: body.clone(),
                parent: Some(Existing(root)),
                intent: Some(RevisionIntent::Revise),
            },
            SuggestedAction::AddEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Existing(target),
            },
        ],
    );
    let mut runner = AnalysisRunner::new(EvaluationInputs {
        values: Body::from([("time".into(), timestamp.into())]),
    });
    let checked = repair::check(&graph, &observation, "repair-001", &mut runner).unwrap();
    assert_eq!(graph.view().len(), 3);
    let document = toml::from_str::<toml::Table>(checked.toml()).unwrap();
    assert_eq!(
        document["database"].as_str(),
        Some(graph.view().id().database.to_string().as_str())
    );
    assert_eq!(document["base"].as_integer(), Some(3));
    assert_eq!(document["plan_id"].as_str(), Some("repair-001"));
    let entries = document["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["intent"].as_str(), Some("revise"));
    assert_eq!(
        entries[0]["predecessor"].as_str(),
        Some(format!("@{}:{}", root.origin, root.position()).as_str())
    );
    assert_eq!(entries[1]["from"].as_str(), Some("$repair_0"));
    assert_eq!(
        entries[1]["to"].as_str(),
        Some(format!("@{}:{}", target.origin, target.position()).as_str())
    );
    assert_eq!(
        entries[0]["payload"]["at"]
            .as_datetime()
            .unwrap()
            .to_string()
            .parse::<crate::Timestamp>()
            .unwrap(),
        timestamp
    );
    assert_eq!(
        entries[0]["payload"]["literal"].as_str(),
        Some("2026-09-14T12:00:00Z")
    );
    assert_eq!(
        entries[0]["payload"]["title"].as_str(),
        Some("quote\" slash\\ newline\n")
    );
    assert_eq!(
        entries[0]["payload"]["rows"][0].as_table().unwrap().len(),
        0
    );
    let ids = checked.transaction().added();
    assert_eq!(
        checked.transaction().view().node(ids[0]).unwrap().body,
        body
    );
    assert_eq!(
        checked.transaction().view().edge(ids[1]).unwrap().from,
        ids[0]
    );
    assert_eq!(checked.transaction().diagnostics.inputs, *runner.inputs());
    assert_eq!(
        checked.toml(),
        repair::check(&graph, &observation, "repair-001", &mut runner)
            .unwrap()
            .toml()
    );
    let mut receiver = graph.clone();
    checked.into_transaction().commit(&mut receiver).unwrap();
    assert_eq!(receiver.view().len(), 5);
}

#[test]
fn repair_checks_stale_intent_references_source_and_every_prefix() {
    use crate::analysis::repair;
    let (mut graph, _) = fixture();
    let root = graph.view().at(0).unwrap().id();
    let stale_repair = repair_observation(
        graph.view(),
        vec![SuggestedAction::AddNode {
            ty_name: "Repair".into(),
            payload: Body::new(),
            parent: Some(Existing(root)),
            intent: Some(RevisionIntent::Revise),
        }],
    );
    prepare(
        &graph,
        &[Change::AddNode {
            ty_name: "Successor".into(),
            body: Body::new(),
            parent: Some(Existing(root)),
        }],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .commit(&mut graph)
    .unwrap();
    let snapshot = serde_json::to_vec(&graph).unwrap();
    assert!(matches!(
        repair::check(
            &graph,
            &stale_repair,
            "stale",
            &mut AnalysisRunner::default()
        ),
        Err(repair::Error::Intent { operation: 0 })
    ));
    let mut branch = stale_repair.clone();
    let MessageKind::Suggestion(actions) = &mut branch.diagnostic.kind else {
        unreachable!()
    };
    let SuggestedAction::AddNode { intent, .. } = &mut actions[0] else {
        unreachable!()
    };
    *intent = Some(RevisionIntent::Branch);
    assert!(repair::check(&graph, &branch, "branch", &mut AnalysisRunner::default()).is_ok());
    assert_eq!(branch.key, stale_repair.key);
    assert_ne!(branch, stale_repair);
    let foreign = LifeCoreList::default();
    assert!(matches!(
        repair::check(&foreign, &branch, "foreign", &mut AnalysisRunner::default()),
        Err(repair::Error::View(_))
    ));
    let forward = repair_observation(
        graph.view(),
        vec![SuggestedAction::AddEdge {
            from: Added(0),
            ty_name: "link".into(),
            to: Existing(root),
        }],
    );
    assert!(matches!(
        repair::check(&graph, &forward, "forward", &mut AnalysisRunner::default()),
        Err(repair::Error::Check(
            crate::transaction::Error::InvalidPlanReference { .. }
        ))
    ));
    let out_of_profile = repair_observation(
        graph.view(),
        vec![SuggestedAction::AddNode {
            ty_name: "Time".into(),
            payload: Body::from([("at".into(), crate::Timestamp::MIN.into())]),
            parent: None,
            intent: None,
        }],
    );
    assert!(matches!(
        repair::check(
            &graph,
            &out_of_profile,
            "time",
            &mut AnalysisRunner::default()
        ),
        Err(repair::Error::TimestampRange)
    ));
    assert_eq!(snapshot, serde_json::to_vec(&graph).unwrap());
    let mut stored = serde_json::to_value(&graph).unwrap();
    stored["suite"] = serde_json::to_value(suite("1")).unwrap();
    let configured: LifeCoreList = serde_json::from_value(stored).unwrap();
    let future = graph.view().at(2).unwrap().id();
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(Factory {
            observed: Arc::default(),
            future,
        }))
        .unwrap();
    assert!(matches!(
        repair::check(&configured, &branch, "recheck", &mut runner),
        Err(repair::Error::Check(crate::transaction::Error::Core(
            LifeCoreError::SystemsError(_)
        )))
    ));
}

#[test]
fn concurrent_dismissal_updates_preserve_both_frontends_records() {
    let (graph, _) = fixture();
    let report = advisory_runner(advisory_factory(), EvaluationInputs::default())
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    let keys: Vec<_> = report
        .groups(&Dismissals::default(), history_options())
        .into_iter()
        .map(|g| g.key.clone())
        .collect();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("dismissals.json");
    let barrier = Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        for key in keys.clone() {
            let path = &path;
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                DismissalStore::new(path)
                    .dismiss(key, "parallel".into())
                    .unwrap();
            });
        }
    });
    let state = DismissalStore::new(path).load().unwrap();
    assert_eq!(state.records().count(), 2);
    assert!(keys.iter().all(|key| state.get(key).is_some()));
}

#[derive(Clone)]
struct InvalidOutputFactory(Message);
impl AnalyzerFactory for InvalidOutputFactory {
    fn analyzer_name(&self) -> &'static str {
        "InvalidOutput"
    }
    fn analyzer_with_config(&mut self, _config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        Ok(Box::new(self.clone()))
    }
}
impl Analyzer for InvalidOutputFactory {
    fn identity(&self) -> AnalyzerIdentity {
        AnalyzerIdentity {
            name: "InvalidOutput".into(),
            version: "1".into(),
            configuration: Fact::Null,
        }
    }
    fn initialize(&mut self, _inputs: &EvaluationInputs) -> Result<Output, String> {
        Ok(Output::default())
    }
    fn analyze(&self, _view: View<'_>, _inputs: &EvaluationInputs) -> Result<Output, String> {
        Ok(Output(vec![self.0.clone()]))
    }
}

#[test]
fn normalization_rejects_future_and_foreign_locations_subjects_and_relevance() {
    let (graph, future) = fixture();
    let foreign = LifeCoreList::default();
    let base = repair_observation(graph.view(), vec![]).diagnostic;
    let mut bad_location = base.clone();
    bad_location.offending_event = graph.view().location(Some(future));
    let mut foreign_location = base.clone();
    foreign_location.offending_event = foreign.view().location(None);
    let mut bad_subject = base.clone();
    bad_subject.offending_event = None;
    bad_subject.finding.subjects.insert(future);
    let mut bad_relevance = base;
    bad_relevance.offending_event = None;
    bad_relevance.relevance.insert(future);
    for diagnostic in [bad_location, foreign_location, bad_subject, bad_relevance] {
        let mut runner = AnalysisRunner::default();
        runner
            .register_analyzer_kind(Box::new(InvalidOutputFactory(diagnostic)))
            .unwrap();
        assert!(matches!(
            runner.run_view(
                graph.view(),
                &SuiteConfig(vec![Config {
                    name: "InvalidOutput".into(),
                    config: "".into()
                }])
            ),
            Err(LifeCoreError::InvalidAnalyzerOutput { .. })
        ));
    }
}
