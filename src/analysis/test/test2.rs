use crate::{
    Body, EntryId, LifeCoreError, LifeCoreList, View, ViewId,
    analysis::{
        AnalysisRunner, Analyzer, AnalyzerCertificate, AnalyzerFactory, AnalyzerIdentity, Config,
        Diagnostic, EvaluatedView, EvaluationInputs, InnerMessageType, Message, MessageKind,
        MessageType, SuiteConfig, TypedAnalyzer,
    },
    transaction::{Change, EntryChange, NewEdge, NewNode, RevisionIntent, prepare},
};
use semver::Version;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Finding;
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
            vec![Finding]
        } else {
            vec![]
        })
    }
    fn convert_message(&self, _finding: Self::MSG) -> Message {
        Message {
            analyzer_certificate: AnalyzerCertificate("rejected_prefix".into()),
            diagnostic: Diagnostic {
                subjects: Default::default(),
                focus: None,
                message: "rejected prefix".into(),
                kind: MessageKind::Inconsistency,
                sticky: false,
            },
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
        Change::Entry(EntryChange::Node(NewNode {
            ty_name: name.into(),
            payload: Body::new(),
            predecessor: None,
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
fn suite(configuration: &str) -> SuiteConfig {
    SuiteConfig(vec![Config {
        name: "Probe".into(),
        configuration: configuration.into(),
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
        report.errors().map(boundaries).collect::<Vec<_>>(),
        vec![Some((1, 1))]
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
    // A focus claimed at initialization, which has no graph.
    invalid: Option<EntryId>,
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
            diagnostic: Diagnostic {
                subjects: BTreeSet::new(),
                focus: self.factory.invalid,
                message: "configured".into(),
                kind: MessageKind::Warning,
                sticky: false,
            },
        }])
    }
    fn analyze(&self, view: View<'_>, inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        self.factory
            .log
            .lock()
            .unwrap()
            .push((self.config, Some(view.len())));
        if self.factory.fail == Some(EvaluatedView::prefix(view.id())) {
            return Err("prefix failed".into());
        }
        let mut output = Vec::new();
        if let Some(node) = view.nodes().next()
            && view.is_stale(node.id) == Some(false)
        {
            // The wording names the boundary, so these do not merge into a range.
            let message = Message {
                analyzer_certificate: local_certificate("missing_summary", &[node.id], "summary"),
                diagnostic: Diagnostic {
                    subjects: BTreeSet::from([node.id]),
                    focus: Some(node.id),
                    message: format!(
                        "Missing summary at {} with {:?}",
                        view.len(),
                        inputs.evaluation_time
                    ),
                    kind: MessageKind::Warning,
                    sticky: false,
                },
            };
            output.extend([message.clone(), message]);
        }
        if let Some(child) = view.entries().next_back().and_then(|entry| entry.as_node())
            && let Some(predecessor) = child.predecessor
        {
            output.push(Message {
                analyzer_certificate: local_certificate(
                    "review_connections",
                    &[predecessor, child.id],
                    "",
                ),
                diagnostic: Diagnostic {
                    subjects: BTreeSet::from([predecessor, child.id]),
                    focus: Some(child.id),
                    message: "Decide which relationships this successor should retain".into(),
                    kind: MessageKind::Suggestion(vec![EntryChange::Node(NewNode {
                        ty_name: "Review".into(),
                        payload: Body::new(),
                        predecessor: None,
                        intent: None,
                    })]),
                    sticky: true,
                },
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
fn advisory_suite(configurations: &[&str]) -> SuiteConfig {
    SuiteConfig(
        configurations
            .iter()
            .map(|configuration| Config {
                name: "Advisories".into(),
                configuration: (*configuration).into(),
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
        EvaluatedView::prefix(graph.view().id())
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
    assert!(observation["diagnostic"].is_object());
    assert!(observation.get("inputs").is_none());
    assert!(observation.get("message").is_none());
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
    assert_eq!(replay.inputs, inputs);
    assert_ne!(observation.diagnostic.message, original.diagnostic.message);
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
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Revised".into(),
            payload: Body::new(),
            predecessor: Some(Existing(root)),
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
    let node = |name: &str, predecessor| {
        Change::Entry(EntryChange::Node(NewNode {
            ty_name: name.into(),
            payload: Body::new(),
            predecessor,
            intent: predecessor.map(|_| RevisionIntent::Branch),
        }))
    };
    let plan = prepare(
        &graph,
        &[
            node("A", None),
            node("B", None),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            })),
            node("A1", Some(Added(0))),
            node("A2", Some(Added(0))),
            Change::Entry(EntryChange::Edge(NewEdge {
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
            EvaluatedView::prefix(graph.view().prefix(4).unwrap().id())
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
        observation.diagnostic.sticky
            && !matches!(observation.evaluated_view, EvaluatedView::Prefixes { last, .. } if last == output.view)
    }));
    let encoded = serde_json::to_value(&output).unwrap();
    let loaded: crate::analysis::AnalysisOutput = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(loaded, output);
    assert!(
        encoded["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["diagnostic"]["sticky"] == true)
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
        observation.diagnostic.sticky = false;
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
            EvaluatedView::prefix(graph.view().prefix(1).unwrap().id())
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
        EvaluatedView::prefix(graph.view().prefix(0).unwrap().id()),
        EvaluatedView::prefix(graph.view().prefix(1).unwrap().id()),
    ] {
        let mut factory = advisory_factory();
        factory.fail = Some(at);
        let mut runner = advisory_runner(factory, inputs.clone());
        let error = match prepare(
            &graph,
            &[
                Change::AddAnalyzer {
                    name: "Advisories".into(),
                    configuration: "7".into(),
                },
                Change::Entry(EntryChange::Node(NewNode {
                    ty_name: "Candidate".into(),
                    payload: Body::new(),
                    predecessor: None,
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
fn initialization_cannot_claim_a_focus_or_subjects_and_empty_history_still_executes() {
    let (graph, _) = fixture();
    let id = graph.view().at(0).unwrap().id();
    let mut factory = advisory_factory();
    factory.invalid = Some(id);
    assert!(matches!(
        advisory_runner(factory, EvaluationInputs::default())
            .run_view(graph.view(), &advisory_suite(&["7"])),
        Err(LifeCoreError::InvalidAnalyzerOutput { .. })
    ));
    let mut subject = scripted_message("setup", &[], MessageKind::Warning, false);
    subject.diagnostic.subjects.insert(id);
    assert!(matches!(
        setup_runner(vec![subject], |_| vec![]).run_view(graph.view(), &scripted_suite(1)),
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
fn colliding_content_hashes_neither_merge_observations_nor_extend_the_wrong_range() {
    let graph = roots(6);
    let first = graph.view().at(0).unwrap().id();
    let message = move |certificate: &str, text: &str| {
        let mut message = scripted_message(certificate, &[first], MessageKind::Warning, false);
        message.diagnostic.message = text.into();
        message
    };
    // `a` and `b` differ only in key, `a` and `c` only in diagnostic; `a`
    // also recurs after a gap, so two equal observations share the bucket.
    let script = move |view: View<'_>| {
        let at = view.len();
        let mut output = vec![];
        if matches!(at, 1..=3 | 5..=6) {
            output.push(message("a", "same"));
        }
        if matches!(at, 2..=3) {
            output.push(message("b", "same"));
        }
        if matches!(at, 2 | 4) {
            output.push(message("a", "other"));
        }
        output
    };
    let expected = [
        ("a", "same", Some((1, 3))),
        ("b", "same", Some((2, 3))),
        ("a", "other", Some((2, 2))),
        ("a", "other", Some((4, 4))),
        ("a", "same", Some((5, 6))),
    ];
    let mut runner = scripted_runner(script);
    let colliding = runner
        .run_view_with_hasher(
            graph.view(),
            &scripted_suite(2),
            std::hash::BuildHasherDefault::<CollidingHasher>::default(),
        )
        .unwrap();
    let ordinary = runner.run_view(graph.view(), &scripted_suite(2)).unwrap();
    for output in [&colliding, &ordinary] {
        assert_eq!(
            output
                .observations()
                .map(|o| (
                    o.key.analyzer_certificate.0.as_str(),
                    o.diagnostic.message.as_str(),
                    boundaries(o)
                ))
                .collect::<Vec<_>>(),
            expected
        );
    }
    assert_eq!(colliding, ordinary);
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
        evaluated_view: EvaluatedView::prefix(view.id()),
        diagnostic: Diagnostic {
            subjects: BTreeSet::new(),
            focus: None,
            message: "Explicit repair".into(),
            kind: MessageKind::Suggestion(actions),
            sticky: true,
        },
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
            EntryChange::Node(NewNode {
                ty_name: "Repaired".into(),
                payload: body.clone(),
                predecessor: Some(Existing(root)),
                intent: Some(RevisionIntent::Revise),
            }),
            EntryChange::Edge(NewEdge {
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
    assert_eq!(checked.view().node(ids[0]).unwrap().payload, body);
    assert_eq!(checked.view().node(ids[0]).unwrap().predecessor, Some(root));
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
        vec![EntryChange::Node(NewNode {
            ty_name: "Repair".into(),
            payload: Body::new(),
            predecessor: Some(Existing(root)),
            intent: Some(RevisionIntent::Revise),
        })],
    );
    prepare(
        &graph,
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Successor".into(),
            payload: Body::new(),
            predecessor: Some(Existing(root)),
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
    let MessageKind::Suggestion(actions) = &mut branch.diagnostic.kind else {
        unreachable!()
    };
    let EntryChange::Node(NewNode { intent, .. }) = &mut actions[0] else {
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
        vec![EntryChange::Edge(NewEdge {
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
        vec![EntryChange::Node(NewNode {
            ty_name: "Time".into(),
            payload: Body::from([("at".into(), crate::Timestamp::MIN.into())]),
            predecessor: None,
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

// Emits `setup` at initialization and what `script` returns for each prefix.
type Script = Arc<dyn Fn(View<'_>) -> Vec<Message> + Send + Sync>;

#[derive(Clone)]
struct Scripted {
    setup: Vec<Message>,
    script: Script,
}
impl AnalyzerFactory for Scripted {
    fn analyzer_name(&self) -> &'static str {
        "Scripted"
    }
    fn analyzer_with_config(&mut self, _config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        Ok(Box::new(self.clone()))
    }
}
impl Analyzer for Scripted {
    fn version(&self) -> Version {
        Version::new(1, 0, 0)
    }
    fn initialize(&mut self, _inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        Ok(self.setup.clone())
    }
    fn analyze(&self, view: View<'_>, _inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        Ok((self.script)(view))
    }
}
fn scripted_runner(
    script: impl Fn(View<'_>) -> Vec<Message> + Send + Sync + 'static,
) -> AnalysisRunner {
    setup_runner(vec![], script)
}
fn setup_runner(
    setup: Vec<Message>,
    script: impl Fn(View<'_>) -> Vec<Message> + Send + Sync + 'static,
) -> AnalysisRunner {
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(Scripted {
            setup,
            script: Arc::new(script),
        }))
        .unwrap();
    runner
}
fn scripted_message(
    certificate: &str,
    subjects: &[EntryId],
    kind: MessageKind,
    sticky: bool,
) -> Message {
    Message {
        analyzer_certificate: AnalyzerCertificate(certificate.into()),
        diagnostic: Diagnostic {
            subjects: subjects.iter().copied().collect(),
            focus: subjects.first().copied(),
            message: certificate.into(),
            kind,
            sticky,
        },
    }
}
// `count` root nodes, so prefixes run from 0 to `count`.
fn roots(count: usize) -> LifeCoreList {
    let mut graph = LifeCoreList::default();
    let changes = (0..count)
        .map(|i| {
            Change::Entry(EntryChange::Node(NewNode {
                ty_name: format!("N{i}"),
                payload: Body::new(),
                predecessor: None,
                intent: None,
            }))
        })
        .collect::<Vec<_>>();
    prepare(&graph, &changes, &mut AnalysisRunner::default())
        .unwrap()
        .accepted()
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    graph
}
// Each observation's certificate with its boundaries, in output order.
fn ranges(output: &crate::analysis::AnalysisOutput) -> Vec<(&str, Option<(usize, usize)>)> {
    output
        .observations()
        .map(|o| (o.key.analyzer_certificate.0.as_str(), boundaries(o)))
        .collect()
}
// `copies` identical suite entries.
fn scripted_suite(copies: usize) -> SuiteConfig {
    SuiteConfig(vec![
        Config {
            name: "Scripted".into(),
            configuration: String::new(),
        };
        copies
    ])
}
// The inclusive prefix boundaries of an observation; `None` at initialization.
fn boundaries(observation: &Observation) -> Option<(usize, usize)> {
    match observation.evaluated_view {
    EvaluatedView::Initialization => None,
    EvaluatedView::Prefixes { first, last } => Some((first.end_exclusive, last.end_exclusive)),
    }
}

#[test]
fn normalization_rejects_future_and_foreign_subjects_and_focus() {
    let (graph, future) = fixture();
    let (other, _) = fixture();
    let foreign = other.view().at(0).unwrap().id();
    let base = Message {
        analyzer_certificate: AnalyzerCertificate("opaque key".into()),
        diagnostic: Diagnostic {
            subjects: BTreeSet::new(),
            focus: None,
            message: "invalid output".into(),
            kind: MessageKind::Warning,
            sticky: false,
        },
    };
    let mut cases = vec![];
    for id in [future, foreign] {
        let mut subject = base.clone();
        subject.diagnostic.subjects.insert(id);
        let mut focus = base.clone();
        focus.diagnostic.focus = Some(id);
        cases.extend([subject, focus]);
    }
    for diagnostic in cases {
        // The future entry is visible in the head view, but not at earlier prefixes.
        let mut runner = scripted_runner(move |_| vec![diagnostic.clone()]);
        assert!(matches!(
            runner.run_view(graph.view(), &scripted_suite(1)),
            Err(LifeCoreError::InvalidAnalyzerOutput { .. })
        ));
    }
}

fn local_certificate(code: &str, subjects: &[EntryId], detail: &str) -> AnalyzerCertificate {
    AnalyzerCertificate(serde_json::to_string(&(code, subjects, detail)).unwrap())
}

fn timed_inputs() -> EvaluationInputs {
    EvaluationInputs {
        evaluation_time: Some("2026-09-14T14:00:00.123456789+02:00".parse().unwrap()),
    }
}

#[derive(Clone, Default)]
struct TimedAnalyzer {
    initialization_time: Option<crate::OffsetTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TimeFinding {
    evaluation_time: crate::OffsetTimestamp,
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
            diagnostic: Diagnostic {
                subjects: BTreeSet::new(),
                focus: None,
                message: format!("Evaluated at {}", finding.evaluation_time),
                kind: MessageKind::Warning,
                sticky: false,
            },
        }
    }
}

#[test]
fn time_is_explicit_frozen_and_deterministic_with_distinct_initialization_and_prefix_zero() {
    let (graph, _) = fixture();
    let suite = SuiteConfig(vec![Config {
        name: "Timed".into(),
        configuration: "".into(),
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
    // Initialization stays distinct from prefix zero; the same diagnostic at
    // every prefix is one range.
    let observations: Vec<_> = report.observations().collect();
    assert_eq!(observations.len(), 2);
    assert_eq!(
        observations[0].evaluated_view,
        EvaluatedView::Initialization
    );
    assert_eq!(
        observations[1].evaluated_view,
        EvaluatedView::Prefixes {
            first: graph.view().prefix(0).unwrap().id(),
            last: graph.view().id(),
        }
    );
    assert_eq!(observations[0].key, observations[1].key);
    assert_eq!(observations[0].diagnostic, observations[1].diagnostic);
    assert_ne!(observations[0], observations[1]);
    let encoded = serde_json::to_value(&report).unwrap();
    assert_eq!(
        encoded["observations"][0]["evaluated_view"],
        serde_json::json!({"kind": "initialization"})
    );
    assert_eq!(
        encoded["observations"][1]["evaluated_view"],
        serde_json::json!({
            "kind": "prefixes",
            "first": graph.view().prefix(0).unwrap().id(),
            "last": graph.view().id(),
        })
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
    let node = |predecessor, intent| {
        EntryChange::Node(NewNode {
            ty_name: "Repair".into(),
            payload: Body::new(),
            predecessor,
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
            EntryChange::Node(NewNode {
                ty_name: "Second repair".into(),
                payload: Body::new(),
                predecessor: Some(Existing(root)),
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
    let change = [Change::Entry(EntryChange::Node(NewNode {
        ty_name: "Candidate".into(),
        payload: Body::new(),
        predecessor: None,
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
    // Both ends of a range must be prefixes of the supplied graph.
    let mut diverging = observation.clone();
    diverging.evaluated_view = EvaluatedView::Prefixes {
        first: graph.view().id(),
        last: first.view().id(),
    };
    assert!(matches!(
        prepare_suggestion(&receiver, &diverging, &mut runner),
        Err(Error::View(_))
    ));
    let mut shared = observation.clone();
    shared.evaluated_view = EvaluatedView::Prefixes {
        first: graph.view().prefix(1).unwrap().id(),
        last: graph.view().id(),
    };
    assert!(prepare_suggestion(&receiver, &shared, &mut runner).is_ok());
    assert_eq!(receiver, before);
    let mut ordinary_warning = repair_observation(graph.view(), vec![]);
    ordinary_warning.diagnostic.kind = MessageKind::Warning;
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
            EntryChange::Node(NewNode {
                ty_name: "Intermediate".into(),
                payload: Body::new(),
                predecessor: None,
                intent: None,
            }),
            EntryChange::Node(NewNode {
                ty_name: "Final".into(),
                payload: Body::new(),
                predecessor: Some(Added(0)),
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
    // The rejection's complete output covers the final boundary and names
    // only the invalid intermediate prefix.
    let Err(rejected) = checked.accepted() else {
        panic!("the invalid intermediate prefix must prevent commit")
    };
    let errors = rejected.analysis().errors().collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(boundaries(errors[0]), Some((4, 4)));
    assert_eq!(rejected.analysis().view.end_exclusive, 5);
    assert_eq!(rejected.analysis().implementations.len(), 1);
    assert_eq!(graph, before);
}

#[test]
fn re_emission_extends_a_range_and_a_gap_starts_another_at_every_boundary() {
    let graph = roots(6);
    let first = graph.view().at(0).unwrap().id();
    let script = move |view: View<'_>| {
        let mut output = vec![];
        if matches!(view.len(), 1..=3 | 5..=6) {
            output.push(scripted_message(
                "held",
                &[first],
                MessageKind::Warning,
                false,
            ));
        }
        if view.len() == 4 {
            let twice = scripted_message("twice", &[first], MessageKind::Warning, false);
            output.extend([twice.clone(), twice]);
        }
        output
    };
    let mut runner = scripted_runner(script);
    for (end_exclusive, expected) in [
        (0, vec![]),
        (1, vec![("held", Some((1, 1)))]),
        (3, vec![("held", Some((1, 3)))]),
        (4, vec![("held", Some((1, 3))), ("twice", Some((4, 4)))]),
        (
            5,
            vec![
                ("held", Some((1, 3))),
                ("twice", Some((4, 4))),
                ("held", Some((5, 5))),
            ],
        ),
        (
            6,
            vec![
                ("held", Some((1, 3))),
                ("twice", Some((4, 4))),
                ("held", Some((5, 6))),
            ],
        ),
    ] {
        let view = graph.view().prefix(end_exclusive).unwrap();
        let output = runner.run_view(view, &scripted_suite(1)).unwrap();
        assert_eq!(ranges(&output), expected, "boundary {end_exclusive}");
        // Ranges name exact prefixes of the evaluated view.
        for observation in output.observations() {
            let EvaluatedView::Prefixes { first, last } = observation.evaluated_view else {
                panic!("prefix observation")
            };
            assert_eq!(view.prefix(first.end_exclusive).unwrap().id(), first);
            assert_eq!(view.prefix(last.end_exclusive).unwrap().id(), last);
        }
    }
    let output = runner.run_view(graph.view(), &scripted_suite(1)).unwrap();
    let dismissals = Dismissals::default();
    // Only the range that reaches the head is current.
    let current = output.groups(&dismissals, ReportOptions::default());
    assert_eq!(current.len(), 1);
    assert_eq!(
        current[0]
            .selected
            .iter()
            .map(|o| boundaries(o))
            .collect::<Vec<_>>(),
        [Some((5, 6))]
    );
    assert_eq!(current[0].observations.len(), 2);
    let history = output.render(&dismissals, history_options());
    assert!(
        history.contains("  boundaries 1\u{2013}3: held\n"),
        "{history}"
    );
    assert!(history.contains("  boundary 4: twice\n"), "{history}");
    assert!(
        history.contains("  boundaries 5\u{2013}6: held\n"),
        "{history}"
    );
    let reloaded: crate::analysis::AnalysisOutput =
        serde_json::from_slice(&serde_json::to_vec(&output).unwrap()).unwrap();
    assert_eq!(reloaded, output);
}

#[test]
fn a_duplicated_suite_entry_adds_no_observations_and_keeps_ranges() {
    let graph = roots(4);
    let first = graph.view().at(0).unwrap().id();
    let script = move |view: View<'_>| {
        let mut output = vec![scripted_message("always", &[], MessageKind::Warning, false)];
        if view.len() != 2 && !view.is_empty() {
            output.push(scripted_message(
                "gapped",
                &[first],
                MessageKind::Warning,
                false,
            ));
        }
        output
    };
    let mut runner = scripted_runner(script);
    let single = runner.run_view(graph.view(), &scripted_suite(1)).unwrap();
    let doubled = runner.run_view(graph.view(), &scripted_suite(2)).unwrap();
    assert_eq!(doubled.implementations.len(), 2);
    assert_eq!(
        ranges(&single),
        [
            ("always", Some((0, 4))),
            ("gapped", Some((1, 1))),
            ("gapped", Some((3, 4))),
        ]
    );
    assert_eq!(
        doubled.observations().collect::<Vec<_>>(),
        single.observations().collect::<Vec<_>>()
    );
}

#[test]
fn a_range_ending_before_the_view_is_current_only_when_sticky() {
    let graph = roots(4);
    let second = graph.view().at(1).unwrap().id();
    let script = move |view: View<'_>| {
        if view.len() != 2 {
            return vec![];
        }
        vec![
            scripted_message("sticky", &[second], MessageKind::Suggestion(vec![]), true),
            scripted_message("transient", &[second], MessageKind::Warning, false),
        ]
    };
    let mut runner = scripted_runner(script);
    let dismissals = Dismissals::default();
    let selected = |output: &crate::analysis::AnalysisOutput, options| {
        output
            .groups(&dismissals, options)
            .iter()
            .map(|g| g.key.analyzer_certificate.0.clone())
            .collect::<Vec<_>>()
    };
    // At its own boundary both are current and neither is historical.
    let at_two = runner
        .run_view(graph.view().prefix(2).unwrap(), &scripted_suite(1))
        .unwrap();
    assert_eq!(
        selected(&at_two, ReportOptions::default()),
        ["sticky", "transient"]
    );
    assert!(
        !at_two
            .render(&dismissals, ReportOptions::default())
            .contains("historical")
    );
    for end_exclusive in [3, 4] {
        let output = runner
            .run_view(
                graph.view().prefix(end_exclusive).unwrap(),
                &scripted_suite(1),
            )
            .unwrap();
        assert_eq!(
            ranges(&output),
            [("sticky", Some((2, 2))), ("transient", Some((2, 2)))]
        );
        assert_eq!(selected(&output, ReportOptions::default()), ["sticky"]);
        assert_eq!(
            selected(&output, history_options()),
            ["sticky", "transient"]
        );
        let current = output.render(&dismissals, ReportOptions::default());
        assert!(
            current.contains(
                "  boundary 2 (historical advisory; current applicability unconfirmed): sticky\n"
            ),
            "{current}"
        );
    }
}

// Findings use the shared lowercase vocabulary: a category word in the key, a
// diagnostic tagged by that same word with a suggestion's `actions` beside it,
// a `kind`-tagged evaluated view, and snake_case dismissal reasons. Only the
// encoding is fixed here; what a key contains is unchanged.
#[test]
fn findings_serialize_in_the_lowercase_kind_vocabulary() {
    let (graph, future) = fixture();
    let view = graph.view();
    let action = EntryChange::Node(NewNode {
        ty_name: "A".into(),
        payload: Body::new(),
        predecessor: Some(Existing(future)),
        intent: Some(RevisionIntent::Revise),
    });
    let mut suggestion = repair_observation(view, vec![action.clone()]);
    suggestion.diagnostic.focus = Some(future);
    suggestion.diagnostic.subjects = BTreeSet::from([future]);
    let id = serde_json::to_value(future).unwrap();
    let encoded = serde_json::to_value(&suggestion).unwrap();
    assert_eq!(encoded["key"]["kind"], "suggestion");
    assert_eq!(
        encoded["evaluated_view"],
        serde_json::json!({"kind": "prefixes", "first": view.id(), "last": view.id()})
    );
    assert_eq!(
        encoded["diagnostic"],
        serde_json::json!({"kind": "suggestion", "actions": [action], "subjects": [id],
            "focus": id, "message": "Explicit repair", "sticky": true})
    );
    assert_eq!(
        serde_json::from_value::<Observation>(encoded.clone()).unwrap(),
        suggestion
    );
    // An empty suggestion still carries its (empty) actions.
    let empty = repair_observation(view, vec![]);
    let encoded_empty = serde_json::to_value(&empty).unwrap();
    assert_eq!(
        encoded_empty["diagnostic"]["actions"],
        serde_json::json!([])
    );
    assert_eq!(
        serde_json::from_value::<Observation>(encoded_empty).unwrap(),
        empty
    );
    let mut warning = suggestion.clone();
    warning.key.kind = MessageType::Warning;
    warning.diagnostic.kind = MessageKind::Warning;
    warning.evaluated_view = EvaluatedView::Initialization;
    let encoded_warning = serde_json::to_value(&warning).unwrap();
    assert_eq!(encoded_warning["key"]["kind"], "warning");
    assert_eq!(encoded_warning["diagnostic"]["kind"], "warning");
    assert!(encoded_warning["diagnostic"].get("actions").is_none());
    assert_eq!(
        encoded_warning["evaluated_view"],
        serde_json::json!({"kind": "initialization"})
    );
    assert_eq!(
        serde_json::from_value::<Observation>(encoded_warning.clone()).unwrap(),
        warning
    );
    assert_eq!(
        serde_json::to_value(MessageType::Inconsistency).unwrap(),
        "inconsistency"
    );
    for (reason, word) in [
        (DismissalReason::Acknowledged, "acknowledged"),
        (DismissalReason::FalsePositive, "false_positive"),
        (DismissalReason::NoLongerRelevant, "no_longer_relevant"),
    ] {
        assert_eq!(serde_json::to_value(reason).unwrap(), word);
    }
    // The former spellings, a category without its actions or with foreign
    // ones, and unknown members are all rejected.
    let mutate = |base: &serde_json::Value, edit: &dyn Fn(&mut serde_json::Value)| {
        let mut changed = base.clone();
        edit(&mut changed);
        serde_json::from_value::<Observation>(changed).is_err()
    };
    let rejected = [
        mutate(&encoded, &|o| o["key"]["kind"] = "Suggestion".into()),
        mutate(&encoded, &|o| {
            o["diagnostic"].as_object_mut().unwrap().remove("actions");
        }),
        mutate(&encoded, &|o| {
            o["diagnostic"]["actions"] = serde_json::Value::Null
        }),
        mutate(&encoded, &|o| {
            o["diagnostic"]["kind"] = serde_json::json!({"Suggestion": [action]})
        }),
        mutate(&encoded, &|o| o["diagnostic"]["extra"] = true.into()),
        mutate(&encoded, &|o| o["evaluated_view"]["extra"] = true.into()),
        mutate(
            &encoded,
            &|o| {
                o["evaluated_view"] =
                    serde_json::json!({"Prefixes": {"first": view.id(), "last": view.id()}})
            },
        ),
        mutate(&encoded_warning, &|o| {
            o["diagnostic"]["actions"] = serde_json::json!([])
        }),
        mutate(&encoded_warning, &|o| {
            o["evaluated_view"]["extra"] = true.into()
        }),
        mutate(&encoded_warning, &|o| {
            o["evaluated_view"] = "Initialization".into()
        }),
    ];
    assert_eq!(rejected, [true; 10]);
    assert!(serde_json::from_value::<DismissalReason>(serde_json::json!("FalsePositive")).is_err());
}
