use std::marker::PhantomData;

use lifecore::{
    Context, EntryContext, EntryIndex, LifeCore, LifeCoreError,
    analysis::{self, Analyzer, AnalyzerFactory, InnerMessageType, InnerOutput, SuggestedAction},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InteractiveMessage {
    offending: EntryContext,
    relevance: Vec<EntryIndex>,
    action: SuggestedAction,
    text: String,
}

impl InnerMessageType for InteractiveMessage {
    fn ty(&self) -> analysis::MessageType {
        analysis::MessageType::Suggestion
    }
}

#[derive(Clone)]
pub struct InteractiveOutput(Vec<InteractiveMessage>);

impl InnerOutput<InteractiveMessage> for InteractiveOutput {
    fn push(&mut self, value: InteractiveMessage) {
        self.0.push(value);
    }
}

pub struct Interactive<LC: LifeCore> {
    config: String,
    marker: PhantomData<LC>,
}

pub struct InteractiveFactory<LC: LifeCore> {
    marker: PhantomData<LC>,
}

impl<LC: LifeCore> InteractiveFactory<LC> {
    pub const fn name() -> &'static str {
        "Interactive"
    }
}

impl<LC: LifeCore> Default for InteractiveFactory<LC> {
    fn default() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<LC: LifeCore + 'static> AnalyzerFactory<LC> for InteractiveFactory<LC> {
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> Result<Box<dyn Analyzer<LC>>, LifeCoreError> {
        Ok(Box::new(Interactive {
            config: config.to_string(),
            marker: PhantomData,
        }))
    }

    fn analyzer_name(&self) -> &'static str {
        Self::name()
    }
}

impl<LC: LifeCore> Analyzer<LC> for Interactive<LC> {
    type MSG = InteractiveMessage;
    type OUTPUT = InteractiveOutput;

    fn initialize_inner(&mut self, _lifecore: &LC) -> Self::OUTPUT {
        InteractiveOutput(Vec::new())
    }

    fn analyze_inner(&self, lifecore: &LC, window: Context) -> Self::OUTPUT {
        let Some(last) = window.last() else {
            return InteractiveOutput(Vec::new());
        };
        let Some(child) = lifecore.node(last) else {
            return InteractiveOutput(Vec::new());
        };
        let Some(parent_index) = child.parent else {
            return InteractiveOutput(Vec::new());
        };
        let preceding = last.shrink_to_exclusive_left();
        let parent_now = window.look_at(parent_index.absolute());
        let successors = lifecore.fresh_successors(parent_now);
        let mut messages = Vec::new();
        for edge in lifecore
            .edges(preceding)
            .filter(|edge| edge.from == parent_index || edge.to == parent_index)
        {
            let outgoing = edge.from == parent_index;
            let other = if outgoing { edge.to } else { edge.from };
            let other_now = window.look_at(other.absolute());
            if lifecore.is_stale(other_now) != Some(false) {
                continue;
            }
            let recreated = successors.iter().any(|successor| {
                lifecore.edges(window).any(|candidate| {
                    !lifecore.is_stale(candidate.e).unwrap_or(true)
                        && candidate.ty_name == edge.ty_name
                        && if outgoing {
                            candidate.from == successor.entry() && candidate.to == other
                        } else {
                            candidate.to == successor.entry() && candidate.from == other
                        }
                })
            });
            if recreated {
                continue;
            }
            let action = if outgoing {
                SuggestedAction::AddEdge {
                    from: child.e.entry(),
                    ty_name: edge.ty_name.clone(),
                    to: other,
                }
            } else {
                SuggestedAction::AddEdge {
                    from: other,
                    ty_name: edge.ty_name.clone(),
                    to: child.e.entry(),
                }
            };
            messages.push(InteractiveMessage {
                offending: child.e,
                relevance: vec![edge.e.entry(), parent_index, other, child.e.entry()],
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

    fn convert_report(&self, report: Self::OUTPUT) -> analysis::Output {
        analysis::Output(
            report
                .0
                .into_iter()
                .map(|message| {
                    analysis::Suggestion {
                        offending_event: message.offending.into(),
                        system_name: InteractiveFactory::<LC>::name().to_string(),
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

    fn initialize(&mut self, lifecore: &LC) -> analysis::Output {
        let output = self.initialize_inner(lifecore);
        self.convert_report(output)
    }

    fn analyze(&self, lifecore: &LC, window: Context) -> analysis::Output {
        self.convert_report(self.analyze_inner(lifecore, window))
    }
}
