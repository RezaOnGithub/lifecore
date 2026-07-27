use crate::{
    Body, Context, DiagnosticLocation, EntryContext, EntryIndex, LifeCore, LifeCoreAccess,
    LifeCoreError, LifeCoreList, NodeQuery, NodeQueryExpression, NodeQueryPredicate,
    analysis::{
        AnalysisRunner, Analyzer, AnalyzerFactory, Inconsistency, InnerMessageType, InnerOutput,
        LifeCoreAnalysisHarness, MessageType, Output,
    },
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct LookaheadMessage {
    offending_event: EntryContext,
}

impl InnerMessageType for LookaheadMessage {
    fn ty(&self) -> MessageType {
        MessageType::Inconsistency
    }
}

#[derive(Clone)]
struct LookaheadOutput(Vec<LookaheadMessage>);

impl InnerOutput<LookaheadMessage> for LookaheadOutput {
    fn push(&mut self, value: LookaheadMessage) {
        self.0.push(value);
    }
}

struct LookaheadAnalyzer;

impl Analyzer<LifeCoreList> for LookaheadAnalyzer {
    type MSG = LookaheadMessage;
    type OUTPUT = LookaheadOutput;

    fn initialize_inner(&mut self) -> Self::OUTPUT {
        LookaheadOutput(vec![])
    }

    fn analyze_inner(
        &self,
        lifecore: LifeCoreAnalysisHarness<'_, LifeCoreList>,
        window: Context,
    ) -> Self::OUTPUT {
        let last = window.last().unwrap();
        let next = EntryIndex(last.index().0 + 1).absolute();
        let leaked = lifecore.get(next).is_some()
            || lifecore
                .nodes(Context::universe())
                .any(|node| node.e.index() > last.index());

        LookaheadOutput(if leaked {
            vec![LookaheadMessage {
                offending_event: last,
            }]
        } else {
            vec![]
        })
    }

    fn convert_report(&self, report: Self::OUTPUT) -> Output {
        Output(
            report
                .0
                .into_iter()
                .map(|message| {
                    Inconsistency {
                        offending_event: DiagnosticLocation::Entry(message.offending_event),
                        system_name: "Lookahead".to_string(),
                        system_config: String::new(),
                        message: "Analyzer observed an entry beyond its bound".to_string(),
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

    fn analyze(
        &self,
        lifecore: LifeCoreAnalysisHarness<'_, LifeCoreList>,
        window: Context,
    ) -> Output {
        self.convert_report(self.analyze_inner(lifecore, window))
    }
}

struct LookaheadFactory;

impl AnalyzerFactory<LifeCoreList> for LookaheadFactory {
    fn analyzer_with_config(
        &mut self,
        _config: &str,
    ) -> Result<Box<dyn Analyzer<LifeCoreList>>, LifeCoreError> {
        Ok(Box::new(LookaheadAnalyzer))
    }

    fn analyzer_name(&self) -> &'static str {
        "Lookahead"
    }
}

fn family_indexes(lifecore: &impl LifeCoreAccess, context: Context) -> Vec<Vec<EntryIndex>> {
    lifecore
        .families(context)
        .into_iter()
        .map(|family| family.iter().map(|entry| entry.index()).collect())
        .collect()
}

#[test]
fn bounded_harness_matches_a_database_truncated_at_the_same_prefix() {
    let mut prefix = LifeCoreList::default();
    let ancestor = prefix.add_mock("Ancestor", Body::default(), None).unwrap();
    let left = prefix
        .add_mock("Left", Body::default(), Some(ancestor.e))
        .unwrap();
    let right = prefix
        .add_mock("Right", Body::default(), Some(ancestor.e))
        .unwrap();
    let peer = prefix.add_mock("Peer", Body::default(), None).unwrap();
    let edge = prefix.connect_mock(left.e, "Related", peer.e).unwrap();
    let bound = edge.e.context();

    let mut full = prefix.clone();
    let future_left = full
        .add_mock("Future_Left", Body::default(), Some(left.e))
        .unwrap();
    full.add_mock("Future_Peer", Body::default(), Some(peer.e))
        .unwrap();

    let harness = LifeCoreAnalysisHarness::new(&full, bound);
    let left_at_bound = bound.look_at(left.e);
    let left_unbounded = Context::universe().look_at(left.e);
    let edge_unbounded = Context::universe().look_at(edge.e);

    assert_eq!(harness.get(left_unbounded), prefix.get(left_at_bound));
    assert!(harness.get(future_left.e.absolute()).is_none());
    assert_eq!(
        harness
            .window(Context::universe().items())
            .collect::<Vec<_>>(),
        prefix.window(bound.items()).collect::<Vec<_>>()
    );
    assert_eq!(
        harness.nodes(Context::universe()).collect::<Vec<_>>(),
        prefix.nodes(bound).collect::<Vec<_>>()
    );
    assert_eq!(
        harness.edges(Context::universe()).collect::<Vec<_>>(),
        prefix.edges(bound).collect::<Vec<_>>()
    );
    assert_eq!(harness.is_stale(left_unbounded), Some(false));
    assert_eq!(full.is_stale(left_unbounded), Some(true));
    assert_eq!(harness.is_stale(edge_unbounded), Some(false));
    assert_eq!(full.is_stale(edge_unbounded), Some(true));
    assert_eq!(
        harness.fresh_nodes(Context::universe()).collect::<Vec<_>>(),
        prefix.fresh_nodes(bound).collect::<Vec<_>>()
    );
    assert_eq!(
        harness.fresh_edges(Context::universe()).collect::<Vec<_>>(),
        prefix.fresh_edges(bound).collect::<Vec<_>>()
    );
    assert_eq!(
        harness.fresh_edges_of(left_unbounded).collect::<Vec<_>>(),
        prefix.fresh_edges_of(left_at_bound).collect::<Vec<_>>()
    );
    assert_eq!(
        harness.root_of(left_unbounded),
        prefix.root_of(left_at_bound)
    );
    assert_eq!(
        harness.fresh_successors(Context::universe().look_at(ancestor.e)),
        prefix.fresh_successors(bound.look_at(ancestor.e))
    );
    assert_eq!(
        harness.relative_degree(left_unbounded, Context::universe().look_at(right.e)),
        prefix.relative_degree(left_at_bound, bound.look_at(right.e))
    );
    assert_eq!(
        family_indexes(&harness, Context::universe()),
        family_indexes(&prefix, bound)
    );

    let query = || NodeQuery {
        context: Context::universe(),
        expression: NodeQueryExpression::All(vec![
            NodeQueryPredicate::Fresh,
            NodeQueryPredicate::HasFreshOutgoingEdgeOfType("Related".to_string()),
        ]),
    };
    assert_eq!(
        harness.query(query()),
        prefix.query(NodeQuery {
            context: bound,
            ..query()
        })
    );
}

#[test]
fn analysis_runner_prevents_analyzers_from_looking_into_later_prefixes() {
    let mut lifecore = LifeCoreList::default();
    lifecore.add_mock("First", Body::default(), None).unwrap();
    lifecore.add_mock("Second", Body::default(), None).unwrap();
    lifecore.add_analyzer("Lookahead", "");
    let mut runner = AnalysisRunner::<LifeCoreList>::default();
    runner
        .register_analyzer_kind(Box::new(LookaheadFactory))
        .unwrap();

    assert!(runner.run(&lifecore).unwrap().0.is_empty());
}
