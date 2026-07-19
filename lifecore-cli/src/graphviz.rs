use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, anyhow, bail};
use lifecore::{LifeCore, LifeCoreList, NodeExt, PointedEventContext};

use crate::util::{latest_context, pointed_index, same_event};

pub fn render_dot(
    lifecore: &LifeCoreList,
    aliases: &BTreeMap<String, PointedEventContext>,
    full: bool,
    include_fields: bool,
) -> String {
    let context = latest_context(lifecore);
    let nodes = if full {
        lifecore.nodes(context).collect::<Vec<_>>()
    } else {
        lifecore.fresh_nodes(context).collect::<Vec<_>>()
    };
    let edges = if full {
        lifecore.edges(context).collect::<Vec<_>>()
    } else {
        lifecore.fresh_edges(context).collect::<Vec<_>>()
    };

    let mut output =
        String::from("digraph LifeCore {\n  graph [compound=true];\n  node [shape=mrecord];\n");
    if full {
        let included = nodes
            .iter()
            .map(|node| pointed_index(node.e))
            .collect::<BTreeSet<_>>();
        for (cluster, family) in lifecore.families(context).iter().enumerate() {
            let members = family
                .iter()
                .filter(|context| included.contains(&pointed_index(*context)))
                .collect::<Vec<_>>();
            if members.is_empty() {
                continue;
            }
            output.push_str(&format!(
                "  subgraph cluster_{cluster} {{\n    label=\"family {cluster}\";\n"
            ));
            for member in members {
                output.push_str(&format!("    n{};\n", pointed_index(member)));
            }
            output.push_str("  }\n");
        }
    }

    for node in &nodes {
        output.push_str(&format!(
            "  n{} [label=\"{}\"];\n",
            pointed_index(node.e),
            node_label(node, aliases, include_fields)
        ));
    }
    for edge in edges {
        output.push_str(&format!(
            "  n{} -> n{} [label=\"{}\\n{}\"];\n",
            pointed_index(edge.from),
            pointed_index(edge.to),
            escape_record(&edge.ty_name),
            escape_record(&edge.e.to_string())
        ));
    }
    if full {
        for node in &nodes {
            if let Some(parent) = node.parent {
                output.push_str(&format!(
                    "  n{} -> n{} [style=dashed, arrowhead=empty, constraint=false, label=\"parent\"];\n",
                    pointed_index(parent),
                    pointed_index(node.e)
                ));
            }
        }
    }
    output.push_str("}\n");
    output
}

pub fn write_dot(path: &Path, dot: &str) -> Result<()> {
    fs::write(path, dot)
        .with_context(|| format!("failed to write GraphViz definition to {}", path.display()))
}

pub fn write_svg(path: &Path, dot: &str) -> Result<()> {
    log::debug!("rendering GraphViz SVG to {}", path.display());
    let mut child = Command::new("dot")
        .arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to launch GraphViz executable `dot`")?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("failed to open GraphViz stdin"))?
        .write_all(dot.as_bytes())
        .context("failed to send graph definition to GraphViz")?;
    let output = child
        .wait_with_output()
        .context("failed while waiting for GraphViz")?;
    if !output.status.success() {
        bail!(
            "GraphViz failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    fs::write(path, output.stdout)
        .with_context(|| format!("failed to write GraphViz SVG to {}", path.display()))
}

fn node_label(
    node: &NodeExt,
    aliases: &BTreeMap<String, PointedEventContext>,
    include_fields: bool,
) -> String {
    let alias = aliases
        .iter()
        .filter(|(_, context)| same_event(**context, node.e))
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut parts = vec![
        escape_record(&node.e.to_string()),
        escape_record(&node.ty_name),
        escape_record(if alias.is_empty() { "-" } else { &alias }),
    ];
    if include_fields {
        parts.extend(
            node.field
                .0
                .iter()
                .map(|(name, value)| escape_record(&format!("{name}={value}"))),
        );
    }
    format!("{{{}}}", parts.join("|"))
}

fn escape_record(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('|', "\\|")
        .replace('<', "\\<")
        .replace('>', "\\>")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use lifecore::{LifeCore, Body, Payload};

    use super::*;

    #[test]
    fn full_graph_contains_clusters_parent_edges_aliases_and_fields() {
        let mut lifecore = LifeCoreList::default();
        let root = lifecore
            .add(
                "Note",
                Body::from(&[("text", Payload::Text("a|b".into()))]),
                None,
            )
            .unwrap();
        let child = lifecore
            .add("Note", Body::default(), Some(root.e))
            .unwrap();
        let aliases = BTreeMap::from([("child".into(), child.e)]);

        let dot = render_dot(&lifecore, &aliases, true, true);

        assert!(dot.contains("subgraph cluster_0"));
        assert!(dot.contains("style=dashed"));
        assert!(dot.contains("child"));
        assert!(dot.contains("text=\\\"a\\|b\\\""));
    }

    #[test]
    fn fresh_graph_omits_stale_parent() {
        let mut lifecore = LifeCoreList::default();
        let root = lifecore.add("Note", Body::default(), None).unwrap();
        let child = lifecore
            .add("Note", Body::default(), Some(root.e))
            .unwrap();
        let dot = render_dot(&lifecore, &BTreeMap::new(), false, false);
        assert!(!dot.contains(&format!("n{} [", pointed_index(root.e))));
        assert!(dot.contains(&format!("n{} [", pointed_index(child.e))));
    }

    #[test]
    fn graphviz_process_accepts_generated_dot_when_available() {
        if Command::new("dot").arg("-V").output().is_err() {
            return;
        }
        let mut lifecore = LifeCoreList::default();
        lifecore.add("Note", Body::default(), None).unwrap();
        let dot = render_dot(&lifecore, &BTreeMap::new(), false, false);
        let path = std::env::temp_dir().join(format!(
            "lifecore-cli-graphviz-test-{}.svg",
            std::process::id()
        ));

        write_svg(&path, &dot).unwrap();

        let svg = fs::read_to_string(&path).unwrap();
        assert!(svg.contains("<svg"));
        fs::remove_file(path).unwrap();
    }
}
