use crate::{
    Body, DiagnosticLocation, EntryId, LifeCoreError, LifeCoreList, View, ViewId,
    analysis::{
        AnalysisRunner, Analyzer, AnalyzerCertificate, AnalyzerFactory, AnalyzerIdentity, Config,
        EvaluatedView, EvaluationInputs, InnerMessageType, Message, MessageKind, MessageType,
        SuiteConfig, TypedAnalyzer,
    },
    transaction::{Change, EntryChange, NewEdge, NewNode, RevisionIntent, prepare},
};
use semver::Version;
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
struct Probe {
    observed: Arc<Mutex<Vec<ViewId>>>,
    future: EntryId,
    reject: Option<usize>,
}
impl TypedAnalyzer for Probe {
    type MSG = Finding;
    fn version(&self) -> Version {
        Version::new(1, 0, 0)
    }
    fn initialize_inner(&mut self, _inputs: &EvaluationInputs) -> Result<Vec<Self::MSG>, String> {
        Ok(vec![])
    }
    fn analyze_inner(
        &self,
        view: View<'_>,
        _inputs: &EvaluationInputs,
    ) -> Result<Vec<Self::MSG>, String> {
        self.observed.lock().unwrap().push(view.id());
        assert!(view.prefix(view.len() + 1).is_err());
        if self.future.position() >= view.len() {
            assert!(view.get(self.future).is_none());
        }
        assert_eq!(view.entries().count(), view.len());
        assert!(view.entries().all(|e| e.id().position() < view.len()));
        Ok(if self.reject == Some(view.len()) {
            vec![Finding {
                location: view.location(None).unwrap(),
            }]
        } else {
            vec![]
        })
    }
    fn convert_message(&self, finding: Self::MSG) -> Message {
        Message {
            analyzer_certificate: AnalyzerCertificate("rejected_prefix".into()),
            subjects: Default::default(),
            offending_event: Some(finding.location),
            relevance: Default::default(),
            message: "rejected prefix".into(),
            kind: MessageKind::Inconsistency,
            sticky: false,
        }
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
    let changes = ["A", "B", "C"].map(|name| {
        Change::Entry(EntryChange::AddNode(NewNode {
            ty_name: name.into(),
            body: Body::new(),
            parent: None,
            intent: None,
        }))
    });
    let plan = prepare(&graph, &changes, &mut AnalysisRunner::default())
        .unwrap()
        .accepted()
        .unwrap();
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
            .map(|v| v.end_exclusive)
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
            .map(|v| v.end_exclusive)
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
            .map(|e| e.offending_event.unwrap().view.end_exclusive)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert!(report.has_error());
    assert!(!AnalysisRunner::default().run(&graph).unwrap().has_error());
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
    AdvisoryFilter, DismissalReason, Dismissals, ReportOptions, ReportSelection,
};
use crate::analysis::{FindingKey, Observation};
use crate::transaction::Reference::{Added, Existing};
use std::collections::BTreeSet;

type InvocationLog = Arc<Mutex<Vec<(i64, Option<usize>)>>>;

#[derive(Clone)]
struct AdvisoryFactory {
    version: &'static str,
    fail: Option<EvaluatedView>,
    invalid: Option<DiagnosticLocation>,
    log: InvocationLog,
    received_configs: Arc<Mutex<Vec<String>>>,
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
        self.received_configs.lock().unwrap().push(config.into());
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
    fn version(&self) -> Version {
        self.factory.version.parse().unwrap()
    }
    fn initialize(&mut self, _inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        self.factory.log.lock().unwrap().push((self.config, None));
        if self.factory.fail == Some(EvaluatedView::Initialization) {
            return Err("setup failed".into());
        }
        Ok(vec![Message {
            analyzer_certificate: local_certificate("setup", &[], ""),
            subjects: BTreeSet::new(),
            offending_event: self.factory.invalid,
            relevance: BTreeSet::new(),
            message: "configured".into(),
            kind: MessageKind::Warning,
            sticky: false,
        }])
    }
    fn analyze(&self, view: View<'_>, inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        self.factory
            .log
            .lock()
            .unwrap()
            .push((self.config, Some(view.len())));
        if self.factory.fail == Some(EvaluatedView::Prefix(view.id())) {
            return Err("prefix failed".into());
        }
        let mut output = Vec::new();
        if let Some(node) = view.nodes().next()
            && view.is_stale(node.id) == Some(false)
        {
            let message = Message {
                analyzer_certificate: local_certificate("missing_summary", &[node.id], "summary"),
                subjects: BTreeSet::from([node.id]),
                offending_event: view.location(Some(node.id)),
                relevance: BTreeSet::from([node.id]),
                message: format!(
                    "Missing summary at {} with {:?}",
                    view.len(),
                    inputs.evaluation_time
                ),
                kind: MessageKind::Warning,
                sticky: false,
            };
            output.extend([message.clone(), message]);
        }
        if let Some(child) = view.entries().next_back().and_then(|entry| entry.as_node())
            && let Some(parent) = child.parent
        {
            output.push(Message {
                analyzer_certificate: local_certificate(
                    "review_connections",
                    &[parent, child.id],
                    "",
                ),
                subjects: BTreeSet::from([parent, child.id]),
                offending_event: view.location(Some(child.id)),
                relevance: BTreeSet::from([parent, child.id]),
                message: "Decide which relationships this successor should retain".into(),
                kind: MessageKind::Suggestion(vec![EntryChange::AddNode(NewNode {
                    ty_name: "Review".into(),
                    body: Body::new(),
                    parent: None,
                    intent: None,
                })]),
                sticky: true,
            });
        }
        Ok(output)
    }
}

fn advisory_factory() -> AdvisoryFactory {
    AdvisoryFactory {
        version: "1.0.0",
        fail: None,
        invalid: None,
        log: Arc::default(),
        received_configs: Arc::default(),
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
    let received = factory.received_configs.clone();
    let mut runner = advisory_runner(factory, EvaluationInputs::default());
    let report = runner
        .run_view(graph.view(), &advisory_suite(&["7", "7"]))
        .unwrap();
    assert_eq!(report.observations().count(), 4);
    let dismissals = Dismissals::default();
    let groups = report.groups(&dismissals, ReportOptions::default());
    assert_eq!(groups.len(), 2);
    let warning = groups
        .iter()
        .find(|g| g.key.analyzer_certificate.0.contains("missing_summary"))
        .unwrap();
    assert_eq!(warning.observations.len(), 3);
    assert_eq!(warning.selected.len(), 1);
    assert_eq!(
        warning.selected[0].evaluated_view,
        EvaluatedView::Prefix(graph.view().id())
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
            (7, Some(3)),
        ]
    );
    assert_eq!(*received.lock().unwrap(), ["7", "7"]);
    let different_spelling = runner
        .run_view(graph.view(), &advisory_suite(&["7", " 007 "]))
        .unwrap();
    assert_eq!(different_spelling.observations().count(), 8);
    assert_eq!(
        different_spelling
            .groups(&dismissals, ReportOptions::default())
            .len(),
        4
    );
    assert_eq!(*received.lock().unwrap(), ["7", "7", "7", " 007 "]);
    assert_eq!(different_spelling.implementations[1].configuration, " 007 ");
    let reloaded: crate::analysis::AnalysisOutput =
        serde_json::from_slice(&serde_json::to_vec(&report).unwrap()).unwrap();
    assert_eq!(reloaded, report);
    assert_eq!(
        reloaded.render(&dismissals, history_options()),
        report.render(&dismissals, history_options())
    );
    let encoded = serde_json::to_value(&report).unwrap();
    let observation = &encoded["observations"][0];
    assert!(observation.get("diagnostic").is_none());
    assert!(observation.get("analyzer_certificate").is_none());
    assert!(observation["key"].get("finding").is_none());
    assert!(observation["key"]["analyzer_certificate"].is_string());
}

#[test]
fn dismissal_identity_ignores_observation_details_but_distinguishes_complete_keys() {
    let (graph, _) = fixture();
    let mut runner = advisory_runner(advisory_factory(), EvaluationInputs::default());
    let report = runner
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    let original = report
        .warnings()
        .find(|o| o.key.analyzer_certificate.0.contains("missing_summary"))
        .unwrap();
    let key = original.key.clone();
    let mut dismissals = Dismissals::default();
    dismissals
        .dismiss(
            key.clone(),
            DismissalReason::Acknowledged,
            Some("intentional".into()),
        )
        .unwrap();
    let inputs = timed_inputs();
    let mut runner = advisory_runner(advisory_factory(), inputs.clone());
    let replay = runner
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    let observation = replay
        .warnings()
        .find(|o| o.key.analyzer_certificate.0.contains("missing_summary"))
        .unwrap();
    assert_eq!(observation.key, key);
    assert_eq!(observation.inputs, inputs);
    assert_ne!(observation.message, original.message);
    assert!(
        replay
            .groups(&dismissals, history_options())
            .iter()
            .all(|g| g.key != &key)
    );
    for raw in [" 007 ", "8"] {
        let altered = runner
            .run_view(graph.view(), &advisory_suite(&[raw]))
            .unwrap();
        assert!(
            altered
                .groups(&dismissals, ReportOptions::default())
                .iter()
                .any(|g| g.key.analyzer_certificate == key.analyzer_certificate)
        );
    }
    for version in ["2.0.0", "1.0.0+build.1", "1.0.0-alpha"] {
        let mut factory = advisory_factory();
        factory.version = version;
        let newer = advisory_runner(factory, inputs.clone())
            .run_view(graph.view(), &advisory_suite(&["7"]))
            .unwrap();
        let changed = &newer
            .warnings()
            .find(|o| o.key.analyzer_certificate == key.analyzer_certificate)
            .unwrap()
            .key;
        assert_ne!(changed, &key);
        assert!(dismissals.get(changed).is_none());
    }
    let mut subject = key.clone();
    subject.analyzer_certificate = local_certificate(
        "missing_summary",
        &[graph.view().at(1).unwrap().id()],
        "summary",
    );
    let mut detail = key.clone();
    detail.analyzer_certificate = local_certificate(
        "missing_summary",
        &[graph.view().at(0).unwrap().id()],
        "other field",
    );
    let mut database = key.clone();
    database.database = uuid::Uuid::new_v4();
    let mut analyzer = key.clone();
    analyzer.analyzer.name = "OtherAnalyzer".into();
    let mut category = key.clone();
    category.kind = MessageType::Suggestion;
    for changed in [subject, detail, database, analyzer, category] {
        assert_ne!(changed, key);
        assert!(dismissals.get(&changed).is_none());
    }
}

#[test]
fn dismiss_restore_round_trips_all_reasons_and_notes_without_current_emission() {
    let (mut graph, _) = fixture();
    let mut runner = advisory_runner(advisory_factory(), EvaluationInputs::default());
    let report = runner
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    let key = report
        .warnings()
        .find(|o| o.key.analyzer_certificate.0.contains("missing_summary"))
        .unwrap()
        .key
        .clone();
    let mut dismissals = Dismissals::default();
    for reason in [
        DismissalReason::Acknowledged,
        DismissalReason::FalsePositive,
        DismissalReason::NoLongerRelevant,
    ] {
        for note in [None, Some("known issue".into())] {
            dismissals
                .dismiss(key.clone(), reason, note.clone())
                .unwrap();
            let reloaded: Dismissals =
                serde_json::from_slice(&serde_json::to_vec(&dismissals).unwrap()).unwrap();
            assert_eq!(dismissals, reloaded);
            assert_eq!(reloaded.records().count(), 1);
            assert_eq!(reloaded.get(&key).unwrap().reason, reason);
            assert_eq!(reloaded.get(&key).unwrap().note, note);
            assert!(
                report
                    .groups(&reloaded, history_options())
                    .iter()
                    .all(|g| g.key != &key)
            );
            assert!(
                !report
                    .render(
                        &reloaded,
                        ReportOptions {
                            include_dismissed: true,
                            ..history_options()
                        }
                    )
                    .contains("resolved")
            );
        }
    }
    let root = graph.view().nodes().next().unwrap().id;
    prepare(
        &graph,
        &[Change::Entry(EntryChange::AddNode(NewNode {
            ty_name: "Revised".into(),
            body: Body::new(),
            parent: Some(Existing(root)),
            intent: Some(RevisionIntent::Branch),
        }))],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap()
    .commit(&mut graph)
    .unwrap();
    let report = runner
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    let mut reloaded: Dismissals =
        serde_json::from_slice(&serde_json::to_vec(&dismissals).unwrap()).unwrap();
    assert!(reloaded.restore(&key));
    assert!(!reloaded.restore(&key));
    assert!(
        report
            .groups(&reloaded, ReportOptions::default())
            .iter()
            .all(|g| g.key != &key)
    );
    assert_eq!(
        report
            .groups(&reloaded, history_options())
            .iter()
            .find(|g| g.key == &key)
            .unwrap()
            .observations
            .len(),
        3
    );
    let before = reloaded.clone();
    let mut blocking = key;
    blocking.kind = MessageType::Inconsistency;
    assert!(
        reloaded
            .dismiss(blocking, DismissalReason::FalsePositive, None)
            .is_err()
    );
    assert_eq!(reloaded, before);
}

#[test]
fn sticky_transition_advisories_survive_later_prefixes_and_dismiss_restore_round_trips() {
    let mut graph = LifeCoreList::default();
    let node = |name: &str, parent| {
        Change::Entry(EntryChange::AddNode(NewNode {
            ty_name: name.into(),
            body: Body::new(),
            parent,
            intent: parent.map(|_| RevisionIntent::Branch),
        }))
    };
    let plan = prepare(
        &graph,
        &[
            node("A", None),
            node("B", None),
            Change::Entry(EntryChange::AddEdge(NewEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            })),
            node("A1", Some(Added(0))),
            node("A2", Some(Added(0))),
            Change::Entry(EntryChange::AddEdge(NewEdge {
                from: Added(3),
                ty_name: "link".into(),
                to: Added(1),
            })),
            node("Unrelated", None),
        ],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap();
    let ids = plan.added().to_vec();
    plan.commit(&mut graph).unwrap();
    let mut runner = advisory_runner(advisory_factory(), EvaluationInputs::default());
    for end_exclusive in [4, 5, 6, 7] {
        let at = graph.view().prefix(end_exclusive).unwrap();
        let report = runner.run_view(at, &advisory_suite(&["7"])).unwrap();
        let dismissals = Dismissals::default();
        let groups = report.groups(&dismissals, ReportOptions::default());
        let events: Vec<_> = groups
            .iter()
            .filter(|g| g.key.kind == MessageType::Suggestion)
            .collect();
        assert_eq!(events.len(), if end_exclusive == 4 { 1 } else { 2 });
        assert_eq!(
            events[0].observations[0].evaluated_view,
            EvaluatedView::Prefix(graph.view().prefix(4).unwrap().id())
        );
        if end_exclusive > 4 {
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

    let output = runner
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    let key = output.suggestions().next().unwrap().key.clone();
    assert!(output.suggestions().all(|observation| {
        observation.sticky && observation.evaluated_view != EvaluatedView::Prefix(output.view)
    }));
    let encoded = serde_json::to_value(&output).unwrap();
    let loaded: crate::analysis::AnalysisOutput = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(loaded, output);
    assert!(
        encoded["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["sticky"] == true)
    );
    let mut dismissals = Dismissals::default();
    dismissals
        .dismiss(
            key.clone(),
            DismissalReason::Acknowledged,
            Some("Reviewed the split".into()),
        )
        .unwrap();
    let mut dismissals: Dismissals =
        serde_json::from_value(serde_json::to_value(dismissals).unwrap()).unwrap();
    assert!(
        loaded
            .groups(&dismissals, ReportOptions::default())
            .iter()
            .all(|g| g.key != &key)
    );
    assert!(
        loaded
            .groups(
                &dismissals,
                ReportOptions {
                    include_dismissed: true,
                    ..Default::default()
                }
            )
            .iter()
            .any(|g| g.key == &key)
    );
    assert!(dismissals.restore(&key));
    assert!(
        loaded
            .groups(&dismissals, ReportOptions::default())
            .iter()
            .any(|g| g.key == &key)
    );

    let mut without_sticky = loaded.clone();
    for observation in &mut without_sticky.observations {
        observation.sticky = false;
    }
    assert!(
        without_sticky
            .groups(&dismissals, ReportOptions::default())
            .iter()
            .all(|g| g.key != &key)
    );
    assert!(
        without_sticky
            .groups(&dismissals, history_options())
            .iter()
            .any(|g| g.key == &key)
    );
    assert!(
        loaded
            .groups(
                &dismissals,
                ReportOptions {
                    advisories: AdvisoryFilter::None,
                    ..Default::default()
                }
            )
            .is_empty()
    );
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
    for reason in [
        DismissalReason::Acknowledged,
        DismissalReason::FalsePositive,
        DismissalReason::NoLongerRelevant,
    ] {
        assert!(dismissals.dismiss(key.clone(), reason, None).is_err());
        let imported: Dismissals = serde_json::from_value(serde_json::json!({
            "records": [{ "key": key, "reason": reason, "note": "invalid imported dismissal" }]
        }))
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
            groups[0].selected[0].evaluated_view,
            EvaluatedView::Prefix(graph.view().prefix(1).unwrap().id())
        );
        assert!(report.has_error());
    }
    assert_eq!(dismissals.records().count(), 0);
}

#[test]
fn execution_failures_record_inputs_and_prefix_and_never_publish_a_candidate() {
    let (graph, _) = fixture();
    let inputs = timed_inputs();
    for at in [
        EvaluatedView::Initialization,
        EvaluatedView::Prefix(graph.view().prefix(0).unwrap().id()),
        EvaluatedView::Prefix(graph.view().prefix(1).unwrap().id()),
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
                Change::Entry(EntryChange::AddNode(NewNode {
                    ty_name: "Candidate".into(),
                    body: Body::new(),
                    parent: None,
                    intent: None,
                })),
            ],
            &mut runner,
        ) {
        Ok(_) => panic!("execution failure must reject preparation"),
        Err(crate::transaction::Error::Core(LifeCoreError::AnalyzerExecution(error))) => error,
        Err(error) => panic!("unexpected error: {error}"),
        };
        assert_eq!(error.evaluated_view, at);
        assert_eq!(error.inputs, inputs);
        assert_eq!(error.analyzer.version, Version::new(1, 0, 0));
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
fn hash_collisions_do_not_merge_complete_opaque_keys_or_dismiss_other_findings() {
    let (graph, _) = fixture();
    let mut report = advisory_runner(advisory_factory(), EvaluationInputs::default())
        .run_view(graph.view(), &advisory_suite(&["7"]))
        .unwrap();
    let mut different = report.observations[0].clone();
    different
        .key
        .analyzer_certificate
        .0
        .push_str(" opaque, not JSON \u{0}");
    report.observations.push(different.clone());
    let mut dismissals = Dismissals::default();
    let hasher = std::hash::BuildHasherDefault::<CollidingHasher>::default();
    let groups = report.groups_with_hasher(&dismissals, history_options(), hasher.clone());
    assert_eq!(groups.len(), 3);
    for group in groups {
        let encoded = serde_json::to_vec(group.key).unwrap();
        let decoded: FindingKey = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(group.key, &decoded);
        assert_eq!(encoded, serde_json::to_vec(&decoded).unwrap());
    }
    dismissals
        .dismiss(different.key.clone(), DismissalReason::Acknowledged, None)
        .unwrap();
    let groups = report.groups_with_hasher(&dismissals, history_options(), hasher);
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(|g| g.key != &different.key));
}

fn repair_observation(view: View<'_>, actions: Vec<EntryChange>) -> Observation {
    Observation {
        key: FindingKey {
            database: view.id().database,
            analyzer: AnalyzerIdentity {
                name: "RepairFixture".into(),
                version: Version::new(1, 0, 0),
                configuration: String::new(),
            },
            kind: MessageType::Suggestion,
            analyzer_certificate: AnalyzerCertificate("explicit_repair".into()),
        },
        inputs: EvaluationInputs::default(),
        evaluated_view: EvaluatedView::Prefix(view.id()),
        subjects: BTreeSet::new(),
        offending_event: view.location(None),
        relevance: BTreeSet::new(),
        message: "Explicit repair".into(),
        kind: MessageKind::Suggestion(actions),
        sticky: true,
    }
}

#[test]
fn explicit_suggestion_preserves_order_payloads_exact_references_and_requires_commit() {
    use crate::analysis::suggestion::prepare_suggestion;
    let (graph, _) = fixture();
    let root = graph.view().at(0).unwrap().id();
    let target = graph.view().at(1).unwrap().id();
    let body = Body::from([
        ("title".into(), "quoted text".into()),
        ("at".into(), crate::Timestamp::MIN.into()),
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
            EntryChange::AddNode(NewNode {
                ty_name: "Repaired".into(),
                body: body.clone(),
                parent: Some(Existing(root)),
                intent: Some(RevisionIntent::Revise),
            }),
            EntryChange::AddEdge(NewEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Existing(target),
            }),
        ],
    );
    let mut runner = AnalysisRunner::new(timed_inputs());
    let checked = prepare_suggestion(&graph, &observation, &mut runner)
        .unwrap()
        .accepted()
        .unwrap();
    assert_eq!(graph.view().len(), 3);
    let ids = checked.added().to_vec();
    assert_eq!(checked.view().node(ids[0]).unwrap().body, body);
    assert_eq!(checked.view().node(ids[0]).unwrap().parent, Some(root));
    assert_eq!(checked.view().edge(ids[1]).unwrap().from, ids[0]);
    assert_eq!(checked.view().edge(ids[1]).unwrap().to, target);
    assert_eq!(checked.view().prefix(4).unwrap().is_stale(root), Some(true));
    assert!(checked.view().prefix(4).unwrap().get(ids[1]).is_none());
    assert_eq!(checked.analysis().inputs, *runner.inputs());
    let mut receiver = graph.clone();
    checked.commit(&mut receiver).unwrap();
    assert_eq!(receiver.view().len(), 5);
}

#[test]
fn repair_checks_stale_intent_references_source_and_every_prefix() {
    use crate::analysis::suggestion;
    let (mut graph, _) = fixture();
    let root = graph.view().at(0).unwrap().id();
    let stale_repair = repair_observation(
        graph.view(),
        vec![EntryChange::AddNode(NewNode {
            ty_name: "Repair".into(),
            body: Body::new(),
            parent: Some(Existing(root)),
            intent: Some(RevisionIntent::Revise),
        })],
    );
    prepare(
        &graph,
        &[Change::Entry(EntryChange::AddNode(NewNode {
            ty_name: "Successor".into(),
            body: Body::new(),
            parent: Some(Existing(root)),
            intent: Some(RevisionIntent::Branch),
        }))],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap()
    .commit(&mut graph)
    .unwrap();
    let snapshot = serde_json::to_vec(&graph).unwrap();
    assert!(matches!(
        suggestion::prepare_suggestion(&graph, &stale_repair, &mut AnalysisRunner::default()),
        Err(suggestion::Error::Check(
            crate::transaction::Error::RevisesSuperseded { operation: 0 }
        ))
    ));
    let mut branch = stale_repair.clone();
    let MessageKind::Suggestion(actions) = &mut branch.kind else {
        unreachable!()
    };
    let EntryChange::AddNode(NewNode { intent, .. }) = &mut actions[0] else {
        unreachable!()
    };
    *intent = Some(RevisionIntent::Branch);
    assert!(
        suggestion::prepare_suggestion(&graph, &branch, &mut AnalysisRunner::default()).is_ok()
    );
    assert_eq!(branch.key, stale_repair.key);
    assert_ne!(branch, stale_repair);
    let foreign = LifeCoreList::default();
    assert!(matches!(
        suggestion::prepare_suggestion(&foreign, &branch, &mut AnalysisRunner::default()),
        Err(suggestion::Error::View(_))
    ));
    let forward = repair_observation(
        graph.view(),
        vec![EntryChange::AddEdge(NewEdge {
            from: Added(0),
            ty_name: "link".into(),
            to: Existing(root),
        })],
    );
    assert!(matches!(
        suggestion::prepare_suggestion(&graph, &forward, &mut AnalysisRunner::default()),
        Err(suggestion::Error::Check(
            crate::transaction::Error::InvalidPlanReference { .. }
        ))
    ));
    let out_of_profile = repair_observation(
        graph.view(),
        vec![EntryChange::AddNode(NewNode {
            ty_name: "Time".into(),
            body: Body::from([("at".into(), crate::Timestamp::MIN.into())]),
            parent: None,
            intent: None,
        })],
    );
    assert!(
        suggestion::prepare_suggestion(&graph, &out_of_profile, &mut AnalysisRunner::default())
            .is_ok()
    );
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
    let rejected = suggestion::prepare_suggestion(&configured, &branch, &mut runner).unwrap();
    assert!(rejected.analysis().has_error());
    assert!(rejected.accepted().is_err());
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
    fn version(&self) -> Version {
        Version::new(1, 0, 0)
    }
    fn initialize(&mut self, _inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        Ok(Vec::new())
    }
    fn analyze(&self, _view: View<'_>, _inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        Ok(vec![self.0.clone()])
    }
}

#[test]
fn normalization_rejects_future_and_foreign_locations_subjects_and_relevance() {
    let (graph, future) = fixture();
    let foreign = LifeCoreList::default();
    let base = Message {
        analyzer_certificate: AnalyzerCertificate("opaque key".into()),
        subjects: BTreeSet::new(),
        offending_event: None,
        relevance: BTreeSet::new(),
        message: "invalid output".into(),
        kind: MessageKind::Warning,
        sticky: false,
    };
    let mut bad_location = base.clone();
    bad_location.offending_event = graph.view().location(Some(future));
    let mut foreign_location = base.clone();
    foreign_location.offending_event = foreign.view().location(None);
    let mut bad_subject = base.clone();
    bad_subject.offending_event = None;
    bad_subject.subjects.insert(future);
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

fn local_certificate(code: &str, subjects: &[EntryId], detail: &str) -> AnalyzerCertificate {
    AnalyzerCertificate(serde_json::to_string(&(code, subjects, detail)).unwrap())
}

fn timed_inputs() -> EvaluationInputs {
    EvaluationInputs {
        evaluation_time: Some("2026-09-14T12:00:00.123456789Z".parse().unwrap()),
    }
}

#[derive(Clone, Default)]
struct TimedAnalyzer {
    initialization_time: Option<crate::Timestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TimeFinding {
    evaluation_time: crate::Timestamp,
}
impl InnerMessageType for TimeFinding {
    fn ty(&self) -> MessageType {
        MessageType::Warning
    }
}

impl AnalyzerFactory for TimedAnalyzer {
    fn analyzer_name(&self) -> &'static str {
        "Timed"
    }
    fn analyzer_with_config(&mut self, _config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        Ok(Box::new(Self::default()))
    }
}
impl TimedAnalyzer {
    fn findings(inputs: &EvaluationInputs) -> Result<Vec<TimeFinding>, String> {
        let evaluation_time = inputs
            .evaluation_time
            .ok_or("evaluation time is required")?;
        Ok(vec![TimeFinding { evaluation_time }])
    }
}
impl TypedAnalyzer for TimedAnalyzer {
    type MSG = TimeFinding;
    fn version(&self) -> Version {
        Version::new(1, 0, 0)
    }
    fn initialize_inner(&mut self, inputs: &EvaluationInputs) -> Result<Vec<Self::MSG>, String> {
        self.initialization_time = inputs.evaluation_time;
        Self::findings(inputs)
    }
    fn analyze_inner(
        &self,
        _view: View<'_>,
        inputs: &EvaluationInputs,
    ) -> Result<Vec<Self::MSG>, String> {
        assert_eq!(inputs.evaluation_time, self.initialization_time);
        Self::findings(inputs)
    }
    fn convert_message(&self, finding: Self::MSG) -> Message {
        assert_eq!(self.initialization_time, Some(finding.evaluation_time));
        Message {
            analyzer_certificate: AnalyzerCertificate("time advisory".into()),
            subjects: BTreeSet::new(),
            offending_event: None,
            relevance: BTreeSet::new(),
            message: format!("Evaluated at {}", finding.evaluation_time),
            kind: MessageKind::Warning,
            sticky: false,
        }
    }
}

#[test]
fn time_is_explicit_frozen_and_deterministic_with_distinct_initialization_and_prefix_zero() {
    let (graph, _) = fixture();
    let suite = SuiteConfig(vec![Config {
        name: "Timed".into(),
        config: "".into(),
    }]);
    let make_runner = |inputs| {
        let mut runner = AnalysisRunner::new(inputs);
        runner
            .register_analyzer_kind(Box::new(TimedAnalyzer::default()))
            .unwrap();
        runner
    };
    let mut missing = make_runner(EvaluationInputs::default());
    let Err(LifeCoreError::AnalyzerExecution(error)) = missing.run_view(graph.view(), &suite)
    else {
        panic!("missing time must fail initialization")
    };
    assert_eq!(error.evaluated_view, EvaluatedView::Initialization);
    assert_eq!(error.inputs.evaluation_time, None);
    assert_eq!(error.message, "evaluation time is required");
    assert!(
        TimedAnalyzer::default()
            .analyze(graph.view(), &EvaluationInputs::default())
            .is_err()
    );
    let inputs = timed_inputs();
    let mut analyzer = TimedAnalyzer::default();
    let expected = vec![TimeFinding {
        evaluation_time: inputs.evaluation_time.unwrap(),
    }];
    assert_eq!(analyzer.initialize_inner(&inputs).unwrap(), expected);
    assert_eq!(
        analyzer.analyze_inner(graph.view(), &inputs).unwrap(),
        expected
    );
    let mut runner = make_runner(inputs.clone());
    let report = runner.run_view(graph.view(), &suite).unwrap();
    assert_eq!(report, runner.run_view(graph.view(), &suite).unwrap());
    assert_eq!(
        report,
        make_runner(inputs.clone())
            .run_view(graph.view(), &suite)
            .unwrap()
    );
    let observations: Vec<_> = report.observations().collect();
    assert_eq!(observations.len(), 5);
    assert_eq!(
        observations[0].evaluated_view,
        EvaluatedView::Initialization
    );
    assert_eq!(
        observations[1].evaluated_view,
        EvaluatedView::Prefix(graph.view().prefix(0).unwrap().id())
    );
    assert_eq!(observations[0].key, observations[1].key);
    assert_ne!(observations[0], observations[1]);
    assert!(observations.iter().all(|o| o.inputs == inputs));
    let encoded = serde_json::to_value(&report).unwrap();
    assert_eq!(
        encoded["observations"][0]["evaluated_view"],
        "Initialization"
    );
    assert_eq!(
        encoded["observations"][1]["evaluated_view"]["Prefix"],
        serde_json::to_value(graph.view().prefix(0).unwrap().id()).unwrap()
    );
    assert_eq!(report.inputs, inputs);
    let reloaded: crate::analysis::AnalysisOutput =
        serde_json::from_slice(&serde_json::to_vec(&report).unwrap()).unwrap();
    assert_eq!(reloaded, report);
    let empty = runner
        .run_view(graph.view().prefix(0).unwrap(), &suite)
        .unwrap();
    let dismissals = Dismissals::default();
    assert_eq!(
        empty.groups(&dismissals, ReportOptions::default())[0]
            .selected
            .len(),
        2
    );
    assert_eq!(
        report.groups(&dismissals, ReportOptions::default())[0]
            .selected
            .len(),
        2
    );
}

#[test]
fn suggestion_rejects_divergent_sources_and_inconsistent_or_newly_stale_intent() {
    use crate::{
        analysis::suggestion::{Error, prepare_suggestion},
        transaction::Error as TransactionError,
    };
    let (graph, _) = fixture();
    let root = graph.view().at(0).unwrap().id();
    let mut runner = AnalysisRunner::default();
    let node = |parent, intent| {
        EntryChange::AddNode(NewNode {
            ty_name: "Repair".into(),
            body: Body::new(),
            parent,
            intent,
        })
    };
    let observation =
        repair_observation(graph.view(), vec![node(None, Some(RevisionIntent::Revise))]);
    assert!(matches!(
        prepare_suggestion(&graph, &observation, &mut runner),
        Err(Error::Check(TransactionError::UnexpectedIntent {
            operation: 0
        }))
    ));
    let observation = repair_observation(graph.view(), vec![node(Some(Existing(root)), None)]);
    assert!(matches!(
        prepare_suggestion(&graph, &observation, &mut runner),
        Err(Error::Check(TransactionError::MissingIntent {
            operation: 0
        }))
    ));
    let two_revisions = repair_observation(
        graph.view(),
        vec![
            node(Some(Existing(root)), Some(RevisionIntent::Revise)),
            EntryChange::AddNode(NewNode {
                ty_name: "Second repair".into(),
                body: Body::new(),
                parent: Some(Existing(root)),
                intent: Some(RevisionIntent::Revise),
            }),
        ],
    );
    assert!(matches!(
        prepare_suggestion(&graph, &two_revisions, &mut runner),
        Err(Error::Check(TransactionError::RevisesSuperseded {
            operation: 1
        }))
    ));
    let change = [Change::Entry(EntryChange::AddNode(NewNode {
        ty_name: "Candidate".into(),
        body: Body::new(),
        parent: None,
        intent: None,
    }))];
    let first = prepare(&graph, &change, &mut runner)
        .unwrap()
        .accepted()
        .unwrap();
    let second = prepare(&graph, &change, &mut runner).unwrap();
    let observation = repair_observation(first.view(), vec![node(None, None)]);
    let mut receiver = graph.clone();
    second.accepted().unwrap().commit(&mut receiver).unwrap();
    let before = receiver.clone();
    assert!(matches!(
        prepare_suggestion(&receiver, &observation, &mut runner),
        Err(Error::View(_))
    ));
    assert_eq!(receiver, before);
    let mut ordinary_warning = repair_observation(graph.view(), vec![]);
    ordinary_warning.kind = MessageKind::Warning;
    ordinary_warning.key.kind = MessageType::Warning;
    assert!(matches!(
        prepare_suggestion(&graph, &ordinary_warning, &mut runner),
        Err(Error::NotSuggestion)
    ));
}

#[test]
fn suggestion_checks_an_invalid_intermediate_prefix_even_with_a_valid_final_view() {
    use crate::analysis::suggestion::prepare_suggestion;
    let (graph, future) = fixture();
    let mut stored = serde_json::to_value(&graph).unwrap();
    stored["suite"] = serde_json::to_value(suite("4")).unwrap();
    let graph: LifeCoreList = serde_json::from_value(stored).unwrap();
    let observation = repair_observation(
        graph.view(),
        vec![
            EntryChange::AddNode(NewNode {
                ty_name: "Intermediate".into(),
                body: Body::new(),
                parent: None,
                intent: None,
            }),
            EntryChange::AddNode(NewNode {
                ty_name: "Final".into(),
                body: Body::new(),
                parent: Some(Added(0)),
                intent: Some(RevisionIntent::Revise),
            }),
        ],
    );
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(Factory {
            observed: Arc::default(),
            future,
        }))
        .unwrap();
    let before = graph.clone();
    let checked = prepare_suggestion(&graph, &observation, &mut runner).unwrap();
    //A The rejection's complete output covers the final boundary and names
    //A only the invalid intermediate prefix.
    let Err(rejected) = checked.accepted() else {
        panic!("the invalid intermediate prefix must prevent commit")
    };
    let errors = rejected.analysis().errors().collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    let EvaluatedView::Prefix(view) = errors[0].evaluated_view else {
        panic!("expected the intermediate prefix")
    };
    assert_eq!(view.end_exclusive, 4);
    assert_eq!(rejected.analysis().view.end_exclusive, 5);
    assert_eq!(rejected.analysis().implementations.len(), 1);
    assert_eq!(graph, before);
}
