use std::marker::PhantomData;

use lifecore::{
    EventContext, LifeCore, LifeCoreError, PointedEventContext,
    analysis::{self, Analyzer, AnalyzerFactory, InnerMessageType, InnerOutput, SuggestedAction},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InteractiveMessage {
    offending_event: PointedEventContext,
    relevance: Vec<PointedEventContext>,
    action: SuggestedAction,
    text: String,
}

impl InnerMessageType for InteractiveMessage {
    fn ty(&self) -> analysis::MessageType {
        analysis::MessageType::Suggestion
    }
}

#[derive(Clone)]
pub struct InteractiveOutput(pub Vec<InteractiveMessage>);

impl InnerOutput<InteractiveMessage> for InteractiveOutput {
    fn push(&mut self, value: InteractiveMessage) {
        self.0.push(value);
    }
}

pub struct Interactive<LC: LifeCore> {
    config: String,
    _lifecore: PhantomData<LC>,
}

pub struct InteractiveFactory<LC: LifeCore> {
    _lifecore: PhantomData<LC>,
}

impl<LC: LifeCore> InteractiveFactory<LC> {
    pub const fn name() -> &'static str {
        "Interactive"
    }
}

impl<LC: LifeCore> Default for InteractiveFactory<LC> {
    fn default() -> Self {
        Self {
            _lifecore: PhantomData,
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
            _lifecore: PhantomData,
        }))
    }

    fn analyzer_name(&self) -> &'static str {
        Self::name()
    }
}

impl<LC: LifeCore> Analyzer<LC> for Interactive<LC> {
    type MSG
        = InteractiveMessage
    where
        Self: Sized;
    type OUTPUT
        = InteractiveOutput
    where
        Self: Sized;

    fn initialize_inner(&mut self, _lifecore: &LC) -> Self::OUTPUT {
        InteractiveOutput(Vec::new())
    }

    fn analyze_inner(&self, lifecore: &LC, window: EventContext) -> Self::OUTPUT {
        let Some(last) = window.last() else {
            return InteractiveOutput(Vec::new());
        };
        let Some(child) = lifecore.node(last) else {
            return InteractiveOutput(Vec::new());
        };
        let Some(parent) = child.parent else {
            return InteractiveOutput(Vec::new());
        };

        let preceding = last.shrink_to_exclusive_left();
        let parent_before_child = preceding.look_at(parent);
        let parent_now = window.look_at(parent);
        let successors = lifecore.fresh_successors(parent_now);
        let mut messages = Vec::new();

        for edge in lifecore.edges(preceding).filter(|edge| {
            edge.from.absolute() == parent_before_child.absolute()
                || edge.to.absolute() == parent_before_child.absolute()
        }) {
            let outgoing = edge.from.absolute() == parent_before_child.absolute();
            let other_before_child = if outgoing { edge.to } else { edge.from };
            let other_now = window.look_at(other_before_child);
            if lifecore.is_stale(other_now) != Some(false) {
                continue;
            }

            let already_recreated = successors.iter().any(|successor| {
                lifecore.edges(window).any(|candidate| {
                    if candidate.ty_name != edge.ty_name
                        || lifecore.is_stale(candidate.e) != Some(false)
                    {
                        return false;
                    }
                    if outgoing {
                        candidate.from.absolute() == successor.absolute()
                            && candidate.to.absolute() == other_now.absolute()
                    } else {
                        candidate.to.absolute() == successor.absolute()
                            && candidate.from.absolute() == other_now.absolute()
                    }
                })
            });
            if already_recreated {
                continue;
            }

            let action = if outgoing {
                SuggestedAction::AddEdge {
                    from: child.e,
                    ty_name: edge.ty_name.clone(),
                    to: other_now,
                }
            } else {
                SuggestedAction::AddEdge {
                    from: other_now,
                    ty_name: edge.ty_name.clone(),
                    to: child.e,
                }
            };
            messages.push(InteractiveMessage {
                offending_event: child.e,
                relevance: vec![edge.e, parent_now, other_now, child.e],
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
                        offending_event: message.offending_event.into(),
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

    fn analyze(&self, lifecore: &LC, window: EventContext) -> analysis::Output {
        self.convert_report(self.analyze_inner(lifecore, window))
    }
}

#[cfg(test)]
mod tests {
    use lifecore::{LifeCoreList, NodeBodyExt, PayloadExt, analysis::Analyzer};

    use super::*;

    #[test]
    fn suggests_recreating_an_outgoing_parent_connection() {
        let mut lifecore = LifeCoreList::default();
        let parent = lifecore.add("Thing", NodeBodyExt::default(), None).unwrap();
        let other = lifecore.add("Other", NodeBodyExt::default(), None).unwrap();
        lifecore.connect(parent.e, "related", other.e).unwrap();
        let child = lifecore
            .add("Thing", NodeBodyExt::default(), Some(parent.e))
            .unwrap();
        let analyzer = Interactive::<LifeCoreList> {
            config: String::new(),
            _lifecore: PhantomData,
        };

        let output = analyzer.analyze(&lifecore, child.e.window());

        assert_eq!(output.suggestions().len(), 1);
        assert_eq!(
            output.suggestions()[0].action,
            vec![SuggestedAction::AddEdge {
                from: child.e,
                ty_name: "related".into(),
                to: child.e.look_at(other.e),
            }]
        );
    }

    #[test]
    fn ignores_parent_connection_when_other_endpoint_is_stale() {
        let mut lifecore = LifeCoreList::default();
        let parent = lifecore.add("Thing", NodeBodyExt::default(), None).unwrap();
        let other = lifecore.add("Other", NodeBodyExt::default(), None).unwrap();
        lifecore.connect(parent.e, "related", other.e).unwrap();
        lifecore
            .add("Other", NodeBodyExt::default(), Some(other.e))
            .unwrap();
        let child = lifecore
            .add("Thing", NodeBodyExt::default(), Some(parent.e))
            .unwrap();
        let analyzer = Interactive::<LifeCoreList> {
            config: String::new(),
            _lifecore: PhantomData,
        };

        assert!(analyzer.analyze(&lifecore, child.e.window()).0.is_empty());
    }

    #[test]
    fn suggests_recreating_an_incoming_parent_connection() {
        let mut lifecore = LifeCoreList::default();
        let other = lifecore.add("Other", NodeBodyExt::default(), None).unwrap();
        let parent = lifecore.add("Thing", NodeBodyExt::default(), None).unwrap();
        lifecore.connect(other.e, "related", parent.e).unwrap();
        let child = lifecore
            .add("Thing", NodeBodyExt::default(), Some(parent.e))
            .unwrap();
        let analyzer = Interactive::<LifeCoreList> {
            config: String::new(),
            _lifecore: PhantomData,
        };

        let output = analyzer.analyze(&lifecore, child.e.window());

        assert_eq!(
            output.suggestions()[0].action,
            vec![SuggestedAction::AddEdge {
                from: child.e.look_at(other.e),
                ty_name: "related".into(),
                to: child.e,
            }]
        );
    }

    #[test]
    fn suppresses_connection_already_recreated_by_a_fresh_sibling() {
        let mut lifecore = LifeCoreList::default();
        let parent = lifecore.add("Thing", NodeBodyExt::default(), None).unwrap();
        let other = lifecore.add("Other", NodeBodyExt::default(), None).unwrap();
        lifecore.connect(parent.e, "related", other.e).unwrap();
        let first_child = lifecore
            .add(
                "Thing",
                NodeBodyExt::from(&[("version", PayloadExt::Number(1))]),
                Some(parent.e),
            )
            .unwrap();
        lifecore.connect(first_child.e, "related", other.e).unwrap();
        let second_child = lifecore
            .add(
                "Thing",
                NodeBodyExt::from(&[("version", PayloadExt::Number(2))]),
                Some(parent.e),
            )
            .unwrap();
        let analyzer = Interactive::<LifeCoreList> {
            config: String::new(),
            _lifecore: PhantomData,
        };

        assert!(
            analyzer
                .analyze(&lifecore, second_child.e.window())
                .0
                .is_empty()
        );
    }
}
