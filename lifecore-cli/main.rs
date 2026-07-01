use std::{collections::HashSet, path::PathBuf};

use lifecore::*;
fn main() {
    let mut lc = LifeCore::new();
    let n0 = lc
        .add(
            "",
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let n1 = lc
        .add(
            "",
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
            Some(n0),
        )
        .unwrap();

    graphviz(
        &lc,
        GraphVizConfig {
            destination_file: PathBuf::from("SomeThing.svg").as_path(),
            family: GraphVizConfigParentMode::AllFamilyIfAnyMember,
            subset_type: HashSet::new(),
            subset_node_type: HashSet::new(),
            subset_edge_type: HashSet::new(),
        },
    ).unwrap();
    println!("Hello, world!");
}
