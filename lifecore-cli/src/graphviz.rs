use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as FmtWrite,
    fs::OpenOptions,
    hash::{DefaultHasher, Hash, Hasher},
    io::Write as IoWrite,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use itertools::Itertools;
use lifecore::{EventContext, LifeCore, NodeExt};

pub struct GraphVizConfig {
    pub destination_file: PathBuf,
    pub family: GraphVizConfigParentMode,
    pub subset_type: Vec<String>,
    pub subset_node_type: Vec<String>,
    pub subset_edge_type: Vec<String>,
    pub colorize: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphVizConfigParentMode {
    AllFamilyIfAnyMember,
    AllFamilyIfAnyFresh,
    AllFamilyIfAnyParent,
    OnlyFresh,
    OnlyParents,
    NoFamily,
}

pub fn graphviz<LC: LifeCore>(lc: &LC, config: GraphVizConfig) -> Result<()> {
    let dot = dot_for_graph(lc, &config)?;
    let final_output_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&config.destination_file)
        .with_context(|| {
            format!(
                "failed to open GraphViz output file {}",
                config.destination_file.display()
            )
        })?;

    let mut child = Command::new("dot")
        .arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(final_output_file)
        .spawn()
        .context("failed to spawn dot; is GraphViz installed?")?;

    {
        let stdin = child.stdin.as_mut().context("failed to open dot stdin")?;
        stdin
            .write_all(dot.as_bytes())
            .context("failed to write DOT to dot stdin")?;
    }
    drop(child.stdin.take());

    let status = child.wait().context("failed to wait for dot")?;
    if !status.success() {
        bail!("dot exited with status {status}");
    }

    Ok(())
}

pub(crate) fn dot_for_graph<LC: LifeCore>(lc: &LC, config: &GraphVizConfig) -> Result<String> {
    let ctx = EventContext::universe();
    let nodes = lc.nodes(ctx).sorted_by(|left, right| left.0.cmp(&right.0));
    let edges = lc.edges(ctx).sorted_by(|left, right| left.0.cmp(&right.0));
    let mut node_by_ctx = BTreeMap::new();
    let mut edge_by_ctx = BTreeMap::new();

    for (node_ctx, node) in nodes {
        node_by_ctx.insert(node_ctx, node);
    }
    for (edge_ctx, edge) in edges {
        edge_by_ctx.insert(edge_ctx, edge);
    }

    let selection = select_events(lc, config, &node_by_ctx, &edge_by_ctx);

    let mut w = String::new();
    writeln!(&mut w, "digraph LifeCore {{")?;
    writeln!(&mut w, "node [shape=mrecord];")?;

    for (node_ctx, node) in node_by_ctx
        .iter()
        .filter(|(node_ctx, _)| selection.nodes.contains(node_ctx))
    {
        let mut attrs = vec![format!("label=\"{}\"", node_label(node))];
        if config.colorize {
            attrs.push("style=filled".to_string());
            attrs.push(format!("fillcolor=\"{}\"", color_for_type(&node.ty_name)));
        }
        writeln!(&mut w, "{} [{}];", node_id(*node_ctx), attrs.join(", "))?;
    }

    if config.family != GraphVizConfigParentMode::NoFamily {
        for family in selected_families(lc, &selection.nodes) {
            let Some(root) = family.iter().min() else {
                continue;
            };
            writeln!(&mut w, "subgraph cluster_{} {{", cluster_id(*root))?;
            for family_node_ctx in family.iter() {
                writeln!(&mut w, "{};", node_id(*family_node_ctx))?;
            }
            for child_ctx in family.iter() {
                let Some(child) = node_by_ctx.get(child_ctx) else {
                    continue;
                };
                let Some(parent_ctx) = child.parent else {
                    continue;
                };
                let parent_ctx = ctx.look_at(parent_ctx);
                if family.contains(&parent_ctx) {
                    writeln!(
                        &mut w,
                        "{} -> {} [style=dashed, label=\"parent\"];",
                        node_id(parent_ctx),
                        node_id(*child_ctx)
                    )?;
                }
            }
            writeln!(&mut w, "}}")?;
        }
    }

    for edge in edge_by_ctx
        .values()
        .filter(|edge| selection.edges.contains(&edge.e))
    {
        let from = ctx.look_at(edge.from);
        let to = ctx.look_at(edge.to);
        if selection.nodes.contains(&from) && selection.nodes.contains(&to) {
            writeln!(
                &mut w,
                "{} -> {} [label=\"{}\"];",
                node_id(from),
                node_id(to),
                dot_escape(&edge.ty_name)
            )?;
        }
    }

    writeln!(&mut w, "}}")?;
    Ok(w)
}

struct Selection {
    nodes: BTreeSet<EventContext>,
    edges: BTreeSet<EventContext>,
}

fn select_events<LC: LifeCore>(
    lc: &LC,
    config: &GraphVizConfig,
    node_by_ctx: &BTreeMap<EventContext, NodeExt>,
    edge_by_ctx: &BTreeMap<EventContext, lifecore::EdgeExt>,
) -> Selection {
    let ctx = EventContext::universe();
    let subset_type = config.subset_type.iter().collect::<BTreeSet<_>>();
    let subset_node_type = config.subset_node_type.iter().collect::<BTreeSet<_>>();
    let subset_edge_type = config.subset_edge_type.iter().collect::<BTreeSet<_>>();
    let no_filter =
        subset_type.is_empty() && subset_node_type.is_empty() && subset_edge_type.is_empty();
    let mut selected_nodes = BTreeSet::new();
    let mut selected_edges = BTreeSet::new();

    if no_filter {
        selected_nodes.extend(node_by_ctx.keys().copied());
        selected_edges.extend(edge_by_ctx.keys().copied());
    } else {
        for (node_ctx, node) in node_by_ctx {
            if subset_type.contains(&node.ty_name) || subset_node_type.contains(&node.ty_name) {
                selected_nodes.insert(*node_ctx);
            }
        }

        for (edge_ctx, edge) in edge_by_ctx {
            let from = ctx.look_at(edge.from);
            let to = ctx.look_at(edge.to);
            let from_ty = node_by_ctx.get(&from).map(|node| &node.ty_name);
            let to_ty = node_by_ctx.get(&to).map(|node| &node.ty_name);
            let edge_type_match =
                subset_type.contains(&edge.ty_name) || subset_edge_type.contains(&edge.ty_name);
            let connected_node_type_match = from_ty.is_some_and(|ty| subset_node_type.contains(ty))
                || to_ty.is_some_and(|ty| subset_node_type.contains(ty));
            let edge_node_match = subset_edge_type.contains(&edge.ty_name);

            if edge_type_match || connected_node_type_match || edge_node_match {
                selected_edges.insert(*edge_ctx);
                selected_nodes.insert(from);
                selected_nodes.insert(to);
            }
        }
    }

    let base_selected_nodes = selected_nodes.clone();
    match config.family {
    GraphVizConfigParentMode::AllFamilyIfAnyMember => {
        expand_matching_families(lc, &base_selected_nodes, &mut selected_nodes, |_| true);
    }
    GraphVizConfigParentMode::AllFamilyIfAnyFresh => {
        expand_matching_families(lc, &base_selected_nodes, &mut selected_nodes, |member| {
            !lc.is_stale(member)
        });
    }
    GraphVizConfigParentMode::AllFamilyIfAnyParent => {
        expand_matching_families(lc, &base_selected_nodes, &mut selected_nodes, |member| {
            lc.is_stale(member)
        });
    }
    GraphVizConfigParentMode::OnlyFresh => {
        selected_nodes.retain(|node_ctx| !lc.is_stale(*node_ctx));
    }
    GraphVizConfigParentMode::OnlyParents => {
        selected_nodes.retain(|node_ctx| lc.is_stale(*node_ctx));
    }
    GraphVizConfigParentMode::NoFamily => {}
    }

    selected_edges.retain(|edge_ctx| {
        let Some(edge) = edge_by_ctx.get(edge_ctx) else {
            return false;
        };
        let from = ctx.look_at(edge.from);
        let to = ctx.look_at(edge.to);
        selected_nodes.contains(&from) && selected_nodes.contains(&to)
    });

    Selection {
        nodes: selected_nodes,
        edges: selected_edges,
    }
}

fn expand_matching_families<LC: LifeCore>(
    lc: &LC,
    base_selected_nodes: &BTreeSet<EventContext>,
    selected_nodes: &mut BTreeSet<EventContext>,
    member_predicate: impl Fn(EventContext) -> bool,
) {
    let ctx = EventContext::universe();
    for family in lc.non_solo_families(ctx) {
        let family = family
            .into_iter()
            .map(|member| ctx.look_at(member))
            .collect::<BTreeSet<_>>();
        if family
            .iter()
            .any(|member| base_selected_nodes.contains(member) && member_predicate(*member))
        {
            selected_nodes.extend(family);
        }
    }
}

fn selected_families<LC: LifeCore>(
    lc: &LC,
    selected_nodes: &BTreeSet<EventContext>,
) -> Vec<BTreeSet<EventContext>> {
    let ctx = EventContext::universe();
    lc.non_solo_families(ctx)
        .into_iter()
        .map(|family| {
            family
                .into_iter()
                .map(|member| ctx.look_at(member))
                .filter(|member| selected_nodes.contains(member))
                .collect::<BTreeSet<_>>()
        })
        .filter(|family| family.len() > 1)
        .collect_vec()
}

fn node_label(node: &NodeExt) -> String {
    let mut label = format!(
        "{{ {{ {} | {} }}",
        record_escape(&node.ty_name),
        record_escape(&node.e.to_string())
    );
    for (key, value) in node
        .field
        .0
        .iter()
        .sorted_by(|left, right| left.0.cmp(right.0))
    {
        label.push_str(&format!(
            " | {} = {}",
            record_escape(key),
            record_escape(&value.to_string())
        ));
    }
    label.push_str(" }");
    dot_escape(&label)
}

fn node_id(ctx: EventContext) -> String {
    format!("\"{}\"", dot_escape(&ctx.to_string()))
}

fn cluster_id(ctx: EventContext) -> String {
    let mut hasher = DefaultHasher::new();
    ctx.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn color_for_type(ty_name: &str) -> String {
    let mut hasher = DefaultHasher::new();
    ty_name.hash(&mut hasher);
    let hash = hasher.finish();
    let red = 128 + ((hash & 0x7f) as u8);
    let green = 128 + (((hash >> 8) & 0x7f) as u8);
    let blue = 128 + (((hash >> 16) & 0x7f) as u8);
    format!("#{red:02x}{green:02x}{blue:02x}")
}

fn record_escape(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        match c {
        '\\' => out.push_str("\\\\"),
        '"' => out.push_str("\\\""),
        '{' => out.push_str("\\{"),
        '}' => out.push_str("\\}"),
        '|' => out.push_str("\\|"),
        '<' => out.push_str("\\<"),
        '>' => out.push_str("\\>"),
        '\n' => out.push_str("\\n"),
        '\r' => {}
        _ => out.push(c),
        }
    }
    out
}

fn dot_escape(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        match c {
        '\\' => out.push_str("\\\\"),
        '"' => out.push_str("\\\""),
        '\n' => out.push_str("\\n"),
        '\r' => {}
        _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lifecore::{LifeCoreList, NodeBodyExt, PayloadExt};

    fn config() -> GraphVizConfig {
        GraphVizConfig {
            destination_file: PathBuf::from("unused.svg"),
            family: GraphVizConfigParentMode::NoFamily,
            subset_type: vec![],
            subset_node_type: vec![],
            subset_edge_type: vec![],
            colorize: true,
        }
    }

    #[test]
    fn dot_uses_event_context_and_sorted_record_rows() {
        let mut lc = LifeCoreList::new();
        lc.add(
            "Task",
            NodeBodyExt::from(&[
                ("z", PayloadExt::Number(2)),
                ("a", PayloadExt::Text("first".to_string())),
            ]),
            None,
        )
        .unwrap();

        let dot = dot_for_graph(&lc, &config()).unwrap();

        assert!(dot.contains("node [shape=mrecord];"));
        assert!(dot.contains("Task | EventContext[0, \\\\<0\\\\>, INF)"));
        let a_pos = dot.find("a =").unwrap();
        let z_pos = dot.find("z = 2").unwrap();
        assert!(a_pos < z_pos);
    }

    #[test]
    fn dot_escapes_record_control_characters() {
        let mut lc = LifeCoreList::new();
        lc.add(
            "Ty|pe",
            NodeBodyExt::from(&[("a{b}", PayloadExt::Text("x|y\\z".to_string()))]),
            None,
        )
        .unwrap();

        let dot = dot_for_graph(&lc, &config()).unwrap();

        assert!(dot.contains("Ty\\\\|pe"));
        assert!(dot.contains("a\\\\{b\\\\}"));
        assert!(dot.contains("x\\\\|y"));
        assert!(dot.contains("y\\\\\\\\z"));
    }

    #[test]
    fn graph_edges_target_whole_nodes() {
        let mut lc = LifeCoreList::new();
        let left = lc.add("Left", NodeBodyExt::from(&[]), None).unwrap();
        let right = lc.add("Right", NodeBodyExt::from(&[]), None).unwrap();
        lc.connect(left.e, "Knows", right.e).unwrap();

        let dot = dot_for_graph(&lc, &config()).unwrap();

        assert!(dot.contains(
            "\"EventContext[0, <0>, INF)\" -> \"EventContext[0, <1>, INF)\" [label=\"Knows\"];"
        ));
        assert!(!dot.contains(':'));
    }

    #[test]
    fn no_family_mode_omits_parent_subgraphs() {
        let mut lc = LifeCoreList::new();
        let old = lc.add("Task", NodeBodyExt::from(&[]), None).unwrap();
        lc.add("Task", NodeBodyExt::from(&[]), Some(old.e)).unwrap();

        let dot = dot_for_graph(&lc, &config()).unwrap();

        assert!(!dot.contains("subgraph cluster_"));
        assert!(!dot.contains("label=\"parent\""));
    }

    #[test]
    fn colorization_is_deterministic_and_can_be_omitted() {
        let mut lc = LifeCoreList::new();
        lc.add("Task", NodeBodyExt::from(&[]), None).unwrap();
        let colorized = dot_for_graph(&lc, &config()).unwrap();
        let mut no_color = config();
        no_color.colorize = false;
        let plain = dot_for_graph(&lc, &no_color).unwrap();

        assert!(colorized.contains(&format!("fillcolor=\"{}\"", color_for_type("Task"))));
        assert!(!plain.contains("fillcolor="));
        assert!(!plain.contains("style=filled"));
    }

    #[test]
    fn filters_include_expected_related_events() {
        let mut lc = LifeCoreList::new();
        let left = lc.add("Left", NodeBodyExt::from(&[]), None).unwrap();
        let right = lc.add("Right", NodeBodyExt::from(&[]), None).unwrap();
        let other = lc.add("Other", NodeBodyExt::from(&[]), None).unwrap();
        lc.connect(left.e, "Knows", right.e).unwrap();
        lc.connect(right.e, "Ignores", other.e).unwrap();
        let mut cfg = config();
        cfg.subset_node_type.push("Left".to_string());

        let dot = dot_for_graph(&lc, &cfg).unwrap();

        assert!(dot.contains("Left | EventContext[0, \\\\<0\\\\>, INF)"));
        assert!(dot.contains("Right | EventContext[0, \\\\<1\\\\>, INF)"));
        assert!(dot.contains("[label=\"Knows\"]"));
        assert!(!dot.contains("Other | EventContext[0, \\\\<2\\\\>, INF)"));
        assert!(!dot.contains("[label=\"Ignores\"]"));
    }

    #[test]
    fn only_fresh_family_mode_uses_fresh_not_survivor_semantics() {
        let mut lc = LifeCoreList::new();
        let old = lc.add("Task", NodeBodyExt::from(&[]), None).unwrap();
        lc.add("Task", NodeBodyExt::from(&[]), Some(old.e)).unwrap();
        let mut cfg = config();
        cfg.family = GraphVizConfigParentMode::OnlyFresh;

        let dot = dot_for_graph(&lc, &cfg).unwrap();

        assert!(!dot.contains("Task | EventContext[0, \\\\<0\\\\>, INF)"));
        assert!(dot.contains("Task | EventContext[0, \\\\<1\\\\>, INF)"));
    }
}
