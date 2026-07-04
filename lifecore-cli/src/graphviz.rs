use std::{
    collections::{HashMap, HashSet},
    io::pipe,
    path::Path,
};

use itertools::Itertools;

use crate::{EventIndex, LifeCore, NodeBody, PayloadExt, SmallTable};

// GraphViz Visualization Config. Can be configured to draw a subset of the graph in multiple passes.
// If an event E passes any of these tests, it is drawn.
//      - `subset_type`: For X in this set, (type name of E is X (either node or an edge))
//      - `subset_node_type`: For X in this set, (E is a node of type X) OR (E is an edge connected to a node of type X)
//      - `subset_edge_type`: For X in this set, (E is an edge of type X) OR (E is a node connected by an edge of type X)
// Along with the check above, if `self.family != GraphVizConfigParentMode::NoFamily`, the entire bloodline is drawn as a
// `graphviz` "subgraph" according to the parameter. (self-descriptive and not fully implemented)

pub struct GraphVizConfig<'a> {
    pub destination_file: &'a Path,
    // Draw all node families as GraphViz "subgraphs"
    pub family: GraphVizConfigParentMode,
    pub subset_type: HashSet<&'a str>,
    pub subset_node_type: HashSet<&'a str>,
    pub subset_edge_type: HashSet<&'a str>,
}

pub enum GraphVizConfigParentMode {
    AllFamilyIfAnyMember,
    AllFamilyIfAnySurvivor,
    AllFamilyIfAnyParent,
    OnlySurvivors,
    OnlyParents,
    NoFamily,
}

pub fn graphviz<'a, 'b>(lc: &'a LifeCore, config: GraphVizConfig<'b>) -> anyhow::Result<()>
where
    'b: 'a,
{
    fn node_label(e: EventIndex, ty: &str, payload: NodeBody) -> String {
        let mut pl = "".to_string();
        for (k, v) in payload
            .0
            .iter()
            .sorted_by(|a, b| Ord::cmp(a.0.as_str(), b.0.as_str()))
        {
            pl.push_str(format!(" | {} = {}", k.as_str(), v).as_str());
        }
        let inner = format!("{{ {{ {} | {} }} {} }}", ty, e.0, pl.as_str())
            .escape_default()
            .to_string();
        format!("\"{}\"", inner.as_str())
    }

    fn node_name(e: EventIndex) -> String {
        format!("E{}", e.0)
    }

    // TODO for now we draw everything
    // Spawn process, take care to close its stdin when we are done.
    let final_output_file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(config.destination_file)?;
    let (graph_dot_feed, mut www) = pipe()?;
    let mut gv = std::process::Command::new("dot")
        .args(["-Tsvg"])
        .stdout(final_output_file)
        .stdin(graph_dot_feed.try_clone()?)
        .spawn()?;
    drop(graph_dot_feed);

    let mut w = String::new();
    {
        use std::fmt::Write;
        writeln!(&mut w, "digraph LifeCore {{")?;
        writeln!(&mut w, "node [shape=Mrecord];")?;
        for e in lc.nodes() {
            writeln!(
                &mut w,
                "E{} [label={}]",
                e.0.0,
                node_label(e.0, &lc[e.1.ty], lc.resolve(&e.1.payload))
            )?;
        }

        for family in lc.non_solo_families() {
            writeln!(&mut w, "subgraph cluster_E{} {{", family[0].0)?;
            for e in family.iter().skip(1) {
                writeln!(
                    &mut w,
                    "{} -> {};",
                    node_name(lc.node(*e).parent.unwrap()),
                    node_name(*e)
                )?;
            }
            writeln!(&mut w, "}}")?;
        }
        writeln!(&mut w, "}}")?;
    }

    eprintln!("Input to GraphViz\n{}", w.as_str());

    use std::io::Write;
    writeln!(&mut www, "{}", w.as_str())?;
    drop(www);
    let _ = gv.wait()?;
    Ok(())
}
