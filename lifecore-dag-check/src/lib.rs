use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};

use lifecore::{
    Context, DiagnosticLocation, EntryIndex, LifeCoreAccess, LifeCoreError,
    analysis::{
        Analyzer, AnalyzerFactory, Inconsistency, InnerMessageType, InnerOutput,
        LifeCoreAnalysisHarness, MessageType, Output, Warning,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DagCheckConfig {
    #[serde(default)]
    pub filter: Option<SubgraphFilter>,
    pub on_cycle: CycleDisposition,
    #[serde(default)]
    pub range: Option<IndexRange>,
}

impl DagCheckConfig {
    pub fn from_json(value: &str) -> Result<Self, DagCheckConfigError> {
        let config = serde_json::from_str::<Self>(value)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), DagCheckConfigError> {
        if let Some(range) = &self.range
            && let (Some(from), Some(through)) = (range.from, range.through)
            && from > through
        {
            return Err(DagCheckConfigError::ReversedRange { from, through });
        }

        Ok(())
    }

    fn applies_to(&self, latest: Option<EntryIndex>) -> bool {
        let Some(latest) = latest.map(|entry| entry.0) else {
            return false;
        };
        let Some(range) = &self.range else {
            return true;
        };

        range.from.is_none_or(|from| latest >= from)
            && range.through.is_none_or(|through| latest <= through)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubgraphFilter {
    #[serde(default)]
    pub node_types: Option<BTreeSet<String>>,
    #[serde(default)]
    pub edge_types: Option<BTreeSet<String>>,
}

impl SubgraphFilter {
    fn includes_node(&self, ty_name: &str) -> bool {
        self.node_types
            .as_ref()
            .is_none_or(|types| types.contains(ty_name))
    }

    fn includes_edge(&self, ty_name: &str) -> bool {
        self.edge_types
            .as_ref()
            .is_none_or(|types| types.contains(ty_name))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexRange {
    #[serde(default)]
    pub from: Option<usize>,
    #[serde(default)]
    pub through: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CycleDisposition {
    Inconsistency,
    Warning,
}

#[derive(Debug, thiserror::Error)]
pub enum DagCheckConfigError {
    #[error("invalid JSON configuration: {0}")]
    Json(#[from] serde_json::Error),
    #[error("range start {from} is after inclusive range end {through}")]
    ReversedRange { from: usize, through: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DagCheckReport {
    pub checked: bool,
    pub components: Vec<CyclicComponent>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CyclicComponent {
    pub nodes: Vec<EntryIndex>,
    pub edges: Vec<EntryIndex>,
}

#[derive(Clone)]
struct SelectedEdge {
    index: EntryIndex,
    from: EntryIndex,
    to: EntryIndex,
}

pub fn check_dag<LC: LifeCoreAccess>(
    lifecore: &LC,
    window: Context,
    config: &DagCheckConfig,
) -> DagCheckReport {
    //A Contexts may extend past the stored graph, so range selection uses the
    //A latest entry actually visible through the supplied access boundary.
    let latest = lifecore
        .window(window.items())
        .last()
        .map(|entry| entry.entry_context().index());
    if !config.applies_to(latest) {
        return DagCheckReport {
            checked: false,
            components: Vec::new(),
        };
    }

    let filter = config.filter.as_ref().cloned().unwrap_or_default();
    let nodes = lifecore
        .fresh_nodes(window)
        .filter(|node| filter.includes_node(&node.ty_name))
        .map(|node| node.e.index())
        .collect::<BTreeSet<_>>();
    let edges = lifecore
        .fresh_edges(window)
        .filter(|edge| {
            filter.includes_edge(&edge.ty_name)
                && nodes.contains(&edge.from)
                && nodes.contains(&edge.to)
        })
        .map(|edge| SelectedEdge {
            index: edge.e.index(),
            from: edge.from,
            to: edge.to,
        })
        .collect::<Vec<_>>();

    DagCheckReport {
        checked: true,
        components: cyclic_components(&nodes, &edges),
    }
}

fn cyclic_components(nodes: &BTreeSet<EntryIndex>, edges: &[SelectedEdge]) -> Vec<CyclicComponent> {
    let mut outgoing = nodes
        .iter()
        .copied()
        .map(|node| (node, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    let mut incoming = outgoing.clone();
    for edge in edges {
        outgoing.get_mut(&edge.from).unwrap().push(edge.to);
        incoming.get_mut(&edge.to).unwrap().push(edge.from);
    }
    for neighbors in outgoing.values_mut().chain(incoming.values_mut()) {
        neighbors.sort_unstable();
        neighbors.dedup();
    }

    //A The iterative two-pass traversal avoids making graph size a call-stack
    //A limit while preserving stable visitation order.
    let mut visited = BTreeSet::new();
    let mut finish_order = Vec::with_capacity(nodes.len());
    for start in nodes {
        if visited.contains(start) {
            continue;
        }
        let mut stack = vec![(*start, false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                finish_order.push(node);
                continue;
            }
            if !visited.insert(node) {
                continue;
            }

            stack.push((node, true));
            for neighbor in outgoing.get(&node).unwrap().iter().rev() {
                if !visited.contains(neighbor) {
                    stack.push((*neighbor, false));
                }
            }
        }
    }

    let mut assigned = BTreeSet::new();
    let mut raw_components = Vec::<Vec<EntryIndex>>::new();
    for start in finish_order.into_iter().rev() {
        if !assigned.insert(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            component.push(node);
            for neighbor in incoming.get(&node).unwrap().iter().rev() {
                if assigned.insert(*neighbor) {
                    stack.push(*neighbor);
                }
            }
        }
        component.sort_unstable();
        raw_components.push(component);
    }

    let mut component_by_node = BTreeMap::new();
    for (component_index, component) in raw_components.iter().enumerate() {
        for node in component {
            component_by_node.insert(*node, component_index);
        }
    }

    let mut internal_edges = vec![Vec::new(); raw_components.len()];
    let mut has_self_loop = vec![false; raw_components.len()];
    for edge in edges {
        let from_component = component_by_node[&edge.from];
        if from_component != component_by_node[&edge.to] {
            continue;
        }
        internal_edges[from_component].push(edge.index);
        has_self_loop[from_component] |= edge.from == edge.to;
    }

    let mut components = raw_components
        .into_iter()
        .enumerate()
        .filter_map(|(index, nodes)| {
            if nodes.len() == 1 && !has_self_loop[index] {
                return None;
            }
            let mut edges = std::mem::take(&mut internal_edges[index]);
            edges.sort_unstable();
            edges.dedup();
            Some(CyclicComponent { nodes, edges })
        })
        .collect::<Vec<_>>();
    components.sort_by(|left, right| {
        left.nodes
            .cmp(&right.nodes)
            .then_with(|| left.edges.cmp(&right.edges))
    });
    components
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DagCheckMessage {
    disposition: CycleDisposition,
    location: DiagnosticLocation,
    relevance: Vec<EntryIndex>,
    component: CyclicComponent,
}

impl InnerMessageType for DagCheckMessage {
    fn ty(&self) -> MessageType {
        match self.disposition {
        CycleDisposition::Inconsistency => MessageType::Inconsistency,
        CycleDisposition::Warning => MessageType::Warning,
        }
    }
}

#[derive(Clone)]
pub struct DagCheckOutput(pub Vec<DagCheckMessage>);

impl InnerOutput<DagCheckMessage> for DagCheckOutput {
    fn push(&mut self, value: DagCheckMessage) {
        self.0.push(value);
    }
}

pub struct DagCheck<LC: LifeCoreAccess> {
    config: DagCheckConfig,
    raw_config: String,
    _lc: PhantomData<LC>,
}

impl<LC: LifeCoreAccess> DagCheck<LC> {
    fn warning_location(
        window: Context,
        component: &CyclicComponent,
        latest_node: Option<EntryIndex>,
    ) -> DiagnosticLocation {
        let location = latest_node
            .filter(|latest| component.nodes.binary_search(latest).is_ok())
            .unwrap_or(component.nodes[0]);
        DiagnosticLocation::Entry(window.look_at(location.absolute()))
    }

    fn message_text(component: &CyclicComponent) -> String {
        let nodes = component
            .nodes
            .iter()
            .map(|entry| entry.0.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let edges = component
            .edges
            .iter()
            .map(|entry| entry.0.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Filtered graph contains a cyclic strongly connected component with nodes [{nodes}] and edges [{edges}]"
        )
    }

    fn convert_message(&self, message: DagCheckMessage) -> lifecore::analysis::Message {
        let text = Self::message_text(&message.component);
        match message.disposition {
        CycleDisposition::Inconsistency => Inconsistency {
            offending_event: message.location,
            system_name: DagCheckFactory::<LC>::name().to_string(),
            system_config: self.raw_config.clone(),
            message: text,
        }
        .into(),
        CycleDisposition::Warning => Warning {
            offending_event: message.location,
            system_name: DagCheckFactory::<LC>::name().to_string(),
            system_config: self.raw_config.clone(),
            relevance: message.relevance,
            message: text,
        }
        .into(),
        }
    }
}

impl<LC: LifeCoreAccess> Analyzer<LC> for DagCheck<LC> {
    type MSG = DagCheckMessage;
    type OUTPUT = DagCheckOutput;

    fn initialize_inner(&mut self) -> Self::OUTPUT {
        DagCheckOutput(Vec::new())
    }

    fn analyze_inner(
        &self,
        lifecore: LifeCoreAnalysisHarness<'_, LC>,
        window: Context,
    ) -> Self::OUTPUT {
        let report = check_dag(&lifecore, window, &self.config);
        if !report.checked {
            return DagCheckOutput(Vec::new());
        }

        let latest_node = window
            .last()
            .and_then(|latest| lifecore.node(latest))
            .map(|node| node.e.index());
        DagCheckOutput(
            report
                .components
                .into_iter()
                .map(|component| {
                    let location = match self.config.on_cycle {
                    CycleDisposition::Inconsistency => DiagnosticLocation::Context(window),
                    CycleDisposition::Warning => {
                        Self::warning_location(window, &component, latest_node)
                    }
                    };
                    let mut relevance = component
                        .nodes
                        .iter()
                        .chain(&component.edges)
                        .copied()
                        .collect::<Vec<_>>();
                    relevance.sort_unstable();
                    relevance.dedup();
                    DagCheckMessage {
                        disposition: self.config.on_cycle,
                        location,
                        relevance,
                        component,
                    }
                })
                .collect(),
        )
    }

    fn convert_report(&self, report: Self::OUTPUT) -> Output {
        Output(
            report
                .0
                .into_iter()
                .map(|message| self.convert_message(message))
                .collect(),
        )
    }

    fn initialize(&mut self) -> Output {
        let report = self.initialize_inner();
        self.convert_report(report)
    }

    fn analyze(&self, lifecore: LifeCoreAnalysisHarness<'_, LC>, window: Context) -> Output {
        self.convert_report(self.analyze_inner(lifecore, window))
    }
}

pub struct DagCheckFactory<LC: LifeCoreAccess> {
    _lc: PhantomData<LC>,
}

impl<LC: LifeCoreAccess> DagCheckFactory<LC> {
    pub fn name() -> &'static str {
        "DAG_Check"
    }
}

impl<LC: LifeCoreAccess> Default for DagCheckFactory<LC> {
    fn default() -> Self {
        Self { _lc: PhantomData }
    }
}

impl<LC: LifeCoreAccess + 'static> AnalyzerFactory<LC> for DagCheckFactory<LC> {
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> Result<Box<dyn Analyzer<LC>>, LifeCoreError> {
        let parsed = DagCheckConfig::from_json(config).map_err(|error| {
            LifeCoreError::InvalidAnalyzerConfiguration {
                analyzer_name: Self::name().to_string(),
                message: error.to_string(),
            }
        })?;
        Ok(Box::new(DagCheck {
            config: parsed,
            raw_config: config.to_string(),
            _lc: PhantomData,
        }))
    }

    fn analyzer_name(&self) -> &'static str {
        Self::name()
    }
}

#[cfg(test)]
mod test;
