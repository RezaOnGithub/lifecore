use std::{
    collections::{BTreeSet, HashSet},
    convert::Infallible,
};

use lifecore::{
    Entry, EntryId, LifeCoreError, View,
    analysis::{
        Analyzer, AnalyzerCertificate, AnalyzerFactory, Diagnostic, EvaluationInputs,
        InnerMessageType, Message, MessageKind, MessageType, TypedAnalyzer,
    },
    transaction::{EntryChange, NewEdge, Reference},
};
use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReconnectConfig {
    #[serde(default)]
    pub filter: Option<SubgraphFilter>,
}

impl ReconnectConfig {
    // The empty string configures the analyzer without a filter; anything else
    // must be a JSON object.
    pub fn from_json(value: &str) -> Result<Self, ReconnectConfigError> {
        if value.is_empty() {
            return Ok(Self::default());
        }
        // The derived parse rejects unknown and repeated fields but also
        // accepts an array for a struct and `null` for an absent field, so the
        // same text is checked again for its shape. Parsing to `Value` alone
        // would keep the last of two repeated fields.
        let config = serde_json::from_str(value)?;
        Self::check_shape(&serde_json::from_str(value)?)?;
        Ok(config)
    }

    fn check_shape(value: &serde_json::Value) -> Result<(), ReconnectConfigError> {
        use serde_json::Value;
        let Value::Object(config) = value else {
            return Err(ReconnectConfigError::Shape("the configuration", "object"));
        };
        let Some(filter) = config.get("filter") else {
            return Ok(());
        };
        let Value::Object(filter) = filter else {
            return Err(ReconnectConfigError::Shape("`filter`", "object"));
        };
        for (name, value) in [
            ("`node_types`", "node_types"),
            ("`edge_types`", "edge_types"),
        ]
        .into_iter()
        .filter_map(|(name, key)| Some((name, filter.get(key)?)))
        {
            if !value.is_array() {
                return Err(ReconnectConfigError::Shape(name, "array"));
            }
        }
        Ok(())
    }

    fn includes_node(&self, ty_name: &str) -> bool {
        self.filter.as_ref().is_none_or(|filter| {
            filter
                .node_types
                .as_ref()
                .is_none_or(|types| types.contains(ty_name))
        })
    }

    fn includes_edge(&self, ty_name: &str) -> bool {
        self.filter.as_ref().is_none_or(|filter| {
            filter
                .edge_types
                .as_ref()
                .is_none_or(|types| types.contains(ty_name))
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReconnectConfigError {
    #[error("invalid JSON configuration: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0} must be a JSON {1}")]
    Shape(&'static str, &'static str),
}

// `node_types` selects the successors judged and `edge_types` the lost edges
// considered. Neither restricts which siblings can claim an edge.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubgraphFilter {
    #[serde(default)]
    pub node_types: Option<BTreeSet<String>>,
    #[serde(default)]
    pub edge_types: Option<BTreeSet<String>>,
}

// An edge of `predecessor` that `successor` does not carry. The suggested
// action is its equivalent edge: the same label, direction, and exact
// `other` endpoint, with `successor` in the predecessor's place.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct LostConnection {
    pub edge: EntryId,
    pub ty_name: String,
    pub predecessor: EntryId,
    pub successor: EntryId,
    pub other: EntryId,
    // Whether the predecessor is the lost edge's `from` endpoint.
    pub outgoing: bool,
}

impl InnerMessageType for LostConnection {
    fn ty(&self) -> MessageType {
        MessageType::Suggestion
    }
}

impl LostConnection {
    pub fn action(&self) -> EntryChange {
        let (from, to) = if self.outgoing {
            (self.successor, self.other)
        } else {
            (self.other, self.successor)
        };
        EntryChange::Edge(NewEdge {
            ty_name: self.ty_name.clone(),
            from: Reference::Existing(from),
            to: Reference::Existing(to),
        })
    }

    pub fn certificate(&self) -> LostEdge {
        LostEdge {
            edge: self.edge,
            successor: self.successor,
        }
    }

    // Presentation only. The text names no boundary or entry, so it never
    // distinguishes findings.
    fn description(&self) -> String {
        format!(
            "revision does not carry its predecessor's {} edge {:?}",
            if self.outgoing {
                "outgoing"
            } else {
                "incoming"
            },
            self.ty_name
        )
    }
}

// The complete finding identity. The edge determines its label and other
// endpoint, so neither is repeated, and no boundary is included.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct LostEdge {
    pub edge: EntryId,
    pub successor: EntryId,
}

// Judges the supersession whose successor is the newest entry of `view`, and
// nothing at any other prefix. Every condition reads `before`, the view ending
// immediately before the successor, so the findings for one supersession
// never depend on what was appended after it.
pub fn lost_connections(view: View<'_>, config: &ReconnectConfig) -> Vec<LostConnection> {
    let Some(successor) = view.entries().next_back().and_then(Entry::as_node) else {
        return Vec::new();
    };
    let Some(predecessor) = successor.predecessor else {
        return Vec::new();
    };
    if !config.includes_node(&successor.ty_name) {
        return Vec::new();
    }
    let before = view
        .prefix(successor.id.position())
        .expect("successor is inside view");
    // The predecessor need not be current: a split's second branch and a late
    // branch find it superseded and are judged like the first successor.
    // Siblings are its current descendants of any type; the successor is not
    // in `before`, so it is never its own sibling.
    let siblings = before
        .descendants(predecessor)
        .filter(|node| before.is_stale(node.id) == Some(false))
        .map(|node| node.id)
        .collect::<Vec<_>>();
    // The predecessor's edges, each as (edge, whether the predecessor is
    // `from`, far endpoint), in append order.
    let candidates = before.edges_of(predecessor).map(|edge| {
        if edge.from == predecessor {
            (edge, true, edge.to)
        } else {
            (edge, false, edge.from)
        }
    });
    // Whether a sibling carries the equivalent edge. With indexes, each
    // question is one lookup of the edge triple (SPEC §9, "Indexes");
    // without them, one scan collects what every sibling carries, as
    // (label, whether the near endpoint is `from`, far endpoint), rather
    // than scanning once per question. Both answer the same.
    let claimed: Box<dyn Fn(&str, bool, EntryId) -> bool> = match before.indexes() {
    Some(indexes) => Box::new(move |label, outgoing, other| {
        siblings.iter().any(|&sibling| {
            let (from, to) = if outgoing {
                (sibling, other)
            } else {
                (other, sibling)
            };
            indexes.edge_between(from, to, label).is_some()
        })
    }),
    None => {
        let siblings = siblings.into_iter().collect::<HashSet<_>>();
        let mut carried = HashSet::new();
        for edge in before.edges() {
            for (near, outgoing, far) in [(edge.from, true, edge.to), (edge.to, false, edge.from)] {
                if siblings.contains(&near) {
                    carried.insert((edge.ty_name.as_str(), outgoing, far));
                }
            }
        }
        Box::new(move |label, outgoing, other| carried.contains(&(label, outgoing, other)))
    }
    };
    // The equivalent edge on the successor cannot already exist, since the
    // successor is newer than every edge in `before`.
    candidates
        .filter(|(edge, outgoing, other)| {
            config.includes_edge(&edge.ty_name)
                // A superseded other endpoint would fail the endpoint check,
                // and choosing among its successors is never done.
                && before.is_stale(*other) == Some(false)
                // Only the equivalent edge claims; another label or direction
                // does not. An edge to a revision of `other` cannot arise
                // here, since that revision supersedes `other`.
                && !claimed(&edge.ty_name, *outgoing, *other)
        })
        .map(|(edge, outgoing, other)| LostConnection {
            edge: edge.id,
            ty_name: edge.ty_name.clone(),
            predecessor,
            successor: successor.id,
            other,
            outgoing,
        })
        .collect()
}

pub struct Reconnect {
    config: ReconnectConfig,
}

impl Reconnect {
    pub fn new(config: ReconnectConfig) -> Self {
        Self { config }
    }
}

impl TypedAnalyzer for Reconnect {
    type MSG = LostConnection;
    type Error = Infallible;

    fn version(&self) -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("valid package version")
    }

    fn initialize_inner(
        &mut self,
        _inputs: &EvaluationInputs,
    ) -> Result<Vec<Self::MSG>, Self::Error> {
        Ok(Vec::new())
    }

    fn analyze_inner(
        &self,
        view: View<'_>,
        _inputs: &EvaluationInputs,
    ) -> Result<Vec<Self::MSG>, Self::Error> {
        Ok(lost_connections(view, &self.config))
    }

    fn convert_message(&self, lost: Self::MSG) -> Message {
        Message {
            analyzer_certificate: AnalyzerCertificate(
                serde_json::to_string(&lost.certificate()).expect("typed lost edge"),
            ),
            diagnostic: Diagnostic {
                subjects: [lost.edge, lost.predecessor, lost.successor, lost.other].into(),
                focus: Some(lost.successor),
                message: lost.description(),
                kind: MessageKind::Suggestion(vec![lost.action()]),
                // Each supersession is judged once, at the successor's prefix,
                // so the suggestion must outlive that prefix in current reports.
                sticky: true,
            },
        }
    }
}

#[derive(Default)]
pub struct ReconnectFactory;

impl ReconnectFactory {
    pub fn name() -> &'static str {
        "Reconnect"
    }
}

impl AnalyzerFactory for ReconnectFactory {
    fn analyzer_with_config(
        &mut self,
        configuration: &str,
    ) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        let config = ReconnectConfig::from_json(configuration).map_err(|error| {
            LifeCoreError::InvalidAnalyzerConfiguration {
                analyzer_name: Self::name().to_string(),
                message: error.to_string(),
            }
        })?;
        Ok(Box::new(Reconnect::new(config)))
    }

    fn analyzer_name(&self) -> &'static str {
        Self::name()
    }
}

#[cfg(test)]
mod test;
