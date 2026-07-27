use lifecore::{
    Context, EntryContext, EntryIndex, LifeCoreAccess, LifeCoreError, LifeCoreList,
    analysis::{
        Analyzer, AnalyzerFactory, InnerMessageType, InnerOutput, LifeCoreAnalysisHarness,
        MessageType, Output, SuggestedAction, Suggestion,
    },
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct InteractiveMessage {
    offending: EntryContext,
    relevance: Vec<EntryIndex>,
    action: SuggestedAction,
    text: String,
}

impl InnerMessageType for InteractiveMessage {
    fn ty(&self) -> MessageType {
        MessageType::Suggestion
    }
}

#[derive(Clone)]
struct InteractiveOutput(Vec<InteractiveMessage>);

impl InnerOutput<InteractiveMessage> for InteractiveOutput {
    fn push(&mut self, value: InteractiveMessage) {
        self.0.push(value);
    }
}

struct InteractiveAnalyzer {
    config: String,
}

impl Analyzer<LifeCoreList> for InteractiveAnalyzer {
    type MSG = InteractiveMessage;
    type OUTPUT = InteractiveOutput;

    fn initialize_inner(&mut self) -> Self::OUTPUT {
        InteractiveOutput(Vec::new())
    }

    fn analyze_inner(
        &self,
        lifecore: LifeCoreAnalysisHarness<'_, LifeCoreList>,
        window: Context,
    ) -> Self::OUTPUT {
        let Some(last) = window.last() else {
            return InteractiveOutput(Vec::new());
        };
        let Some(child) = lifecore.node(last) else {
            return InteractiveOutput(Vec::new());
        };
        let Some(parent_index) = child.parent else {
            return InteractiveOutput(Vec::new());
        };

        //A Parent connections are inspected immediately before the child
        //A append, while freshness and equivalent successors are judged in the
        //A child-ending window that gives the suggestion its identity.
        let preceding = last.shrink_to_exclusive_left();
        let parent_now = window.look_at(parent_index.absolute());
        let successors = lifecore.fresh_successors(parent_now);
        let mut messages = Vec::new();

        for edge in lifecore
            .edges(preceding)
            .filter(|edge| edge.from == parent_index || edge.to == parent_index)
        {
            let outgoing = edge.from == parent_index;
            let opposite = if outgoing { edge.to } else { edge.from };
            let opposite_now = window.look_at(opposite.absolute());
            if lifecore.is_stale(opposite_now) != Some(false) {
                continue;
            }

            //A Any equivalent connection on a fresh successor means the
            //A relationship has already been deliberately recreated for this
            //A family, so emitting another action would be misleading.
            let recreated = successors.iter().any(|successor| {
                lifecore.edges(window).any(|candidate| {
                    !lifecore.is_stale(candidate.e).unwrap_or(true)
                        && candidate.ty_name == edge.ty_name
                        && if outgoing {
                            candidate.from == successor.index() && candidate.to == opposite
                        } else {
                            candidate.from == opposite && candidate.to == successor.index()
                        }
                })
            });
            if recreated {
                continue;
            }

            let action = if outgoing {
                SuggestedAction::AddEdge {
                    from: child.e.index(),
                    ty_name: edge.ty_name.clone(),
                    to: opposite,
                }
            } else {
                SuggestedAction::AddEdge {
                    from: opposite,
                    ty_name: edge.ty_name.clone(),
                    to: child.e.index(),
                }
            };
            messages.push(InteractiveMessage {
                offending: child.e,
                relevance: vec![edge.e.index(), parent_index, opposite, child.e.index()],
                action,
                text: format!(
                    "Recreate the parent node's {} {:?} connection for its fresh successor",
                    if outgoing { "outgoing" } else { "incoming" },
                    edge.ty_name
                ),
            });
        }

        InteractiveOutput(messages)
    }

    fn convert_report(&self, report: Self::OUTPUT) -> Output {
        Output(
            report
                .0
                .into_iter()
                .map(|message| {
                    Suggestion {
                        offending_event: message.offending.into(),
                        system_name: InteractiveFactory.analyzer_name().to_string(),
                        system_config: self.config.clone(),
                        relevance: message.relevance,
                        message: message.text,
                        action: vec![message.action],
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

#[derive(Default)]
pub(crate) struct InteractiveFactory;

impl AnalyzerFactory<LifeCoreList> for InteractiveFactory {
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> Result<Box<dyn Analyzer<LifeCoreList>>, LifeCoreError> {
        Ok(Box::new(InteractiveAnalyzer {
            config: config.to_string(),
        }))
    }

    fn analyzer_name(&self) -> &'static str {
        "Interactive"
    }
}
