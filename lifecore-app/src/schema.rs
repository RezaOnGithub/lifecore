use lifecore::{
    View, ViewId,
    typing::{Type, TypeResolution},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// Nodes whose `ty_name` is the summarized name, by currentness in the view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCounts {
    #[serde(with = "lifecore::decimal")]
    pub current: usize,
    #[serde(with = "lifecore::decimal")]
    pub historical: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeSummary {
    pub name: String,
    pub resolution: TypeResolution,
    pub nodes: NodeCounts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeList {
    pub view: ViewId,
    pub types: Vec<TypeSummary>,
}

// One summary per name the view knows, sorted by name: the built-in `Type`,
// every name a current declaration claims, and every name a node uses, each
// resolved by the view's declaration index as `View::resolve_type` does.
pub fn list_types(view: View<'_>) -> TypeList {
    let mut counts = BTreeMap::<String, NodeCounts>::new();
    for node in view.nodes() {
        let counts = counts.entry(node.ty_name.clone()).or_default();
        if view.is_stale(node.id) == Some(true) {
            counts.historical += 1;
        } else {
            counts.current += 1;
        }
    }
    let declarations = view.declarations();
    let mut names = BTreeSet::from([Type::TYPE_NAME.to_string()]);
    names.extend(counts.keys().cloned());
    names.extend(declarations.names().map(str::to_string));
    let types = names
        .into_iter()
        .map(|name| {
            let resolution = declarations.resolve(&name);
            let nodes = counts.remove(&name).unwrap_or_default();
            TypeSummary {
                name,
                resolution,
                nodes,
            }
        })
        .collect();
    TypeList {
        view: view.id(),
        types,
    }
}
