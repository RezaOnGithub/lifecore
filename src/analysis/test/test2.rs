use crate::{
    Body, DiagnosticLocation, EntryId, LifeCoreError, LifeCoreList, View, ViewId,
    analysis::{
        AnalysisRunner, Analyzer, AnalyzerFactory, Config, Inconsistency, InnerMessageType,
        InnerOutput, MessageType, Output, SuiteConfig,
    },
    transaction::{Change, check},
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
impl Analyzer for Probe {
    type MSG = Finding;
    type OUTPUT = Report;
    fn initialize_inner(&mut self) -> Report {
        Report(vec![])
    }
    fn analyze_inner(&self, view: View<'_>) -> Report {
        self.observed.lock().unwrap().push(view.id());
        assert!(view.prefix(view.len() + 1).is_err());
        if self.future.position() >= view.len() {
            assert!(view.get(self.future).is_none());
        }
        assert_eq!(view.entries().count(), view.len());
        assert!(view.entries().all(|e| e.id().position() < view.len()));
        Report(if self.reject == Some(view.len()) {
            vec![Finding {
                location: view.location(None).unwrap(),
            }]
        } else {
            vec![]
        })
    }
    fn convert_report(&self, report: Report) -> Output {
        Output(
            report
                .0
                .into_iter()
                .map(|finding| {
                    Inconsistency {
                        offending_event: finding.location,
                        system_name: "Probe".into(),
                        system_config: "".into(),
                        message: "rejected prefix".into(),
                    }
                    .into()
                })
                .collect(),
        )
    }
    fn initialize(&mut self) -> Output {
        let report = self.initialize_inner();
        self.convert_report(report)
    }
    fn analyze(&self, view: View<'_>) -> Output {
        self.convert_report(self.analyze_inner(view))
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
    let plan = check(&graph, &changes, &mut AnalysisRunner::default()).unwrap();
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
            .0
            .is_empty()
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
            .map(|e| e.offending_event.view.end)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert!(report.into_result().is_err());
    assert!(Output::default().into_result().is_ok());
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
