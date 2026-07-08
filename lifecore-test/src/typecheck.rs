use lifecore::typecheck::{NodeType, TypeCheck, TypeCheckFactory, TypedBody};
use lifecore::{
    LifeCore, NodeBodyExt, PayloadExt,
    analysis::{AnalysisRunner, Analyzer, AnalyzerFactory},
};

#[test]
fn node_deserialization01() {
    let nt_describe_json =
        serde_json::to_string(&Into::<NodeBodyExt>::into(NodeType::describe_self())).unwrap();
    eprintln!("nt_describe_json\t{}", nt_describe_json.as_str());

    let nt_describe_hashmap =
        serde_json::from_str::<NodeBodyExt>(nt_describe_json.as_str()).unwrap();
    eprintln!("nt_describe_hashmap\t{:#?}", &nt_describe_hashmap);

    let _ = NodeType::try_from(&nt_describe_hashmap).unwrap();
}

#[test]
fn no_bootstrap_typecheck() {
    let mut lc = LifeCore::new();
    let _ = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let mut runner = AnalysisRunner::new();
    lc.add_analyzer(TypeCheckFactory {}.analyzer_name(), "");
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory {}))
        .unwrap();

    let output = runner.run(&lc).unwrap();
    assert!(!output.0.is_empty())
}

#[test]
// Node_Type declares Tombstone as an additional override, which not defined
// Proper bootstrap is through the Suggestion system.
fn bad_bootstrap() {
    let mut lc = LifeCore::new();
    let _ = lc
        .add("Node_Type", NodeType::describe_self().into(), None)
        .unwrap();
    let mut typecheck = TypeCheck {
        config: String::new(),
    };
    assert!(!typecheck.analyze(&lc).0.is_empty());
    assert!(!typecheck.initialize(&lc).0.is_empty());
}

// #[test]
// fn tombstone01() {
//     let mut lc = LifeCore::new();
//     let nt_node_type = lc.add("Node_Type", NodeType::describe_self().into(), None).unwrap();
//     let nt_tombstone = lc
//         .add(
//             "Node_Type",
//             NodeType {
//                 name: "Tombstone".to_string(),
//                 class: lifecore::typecheck::NodeTypeClass::Regular,
//                 outgoing: vec![],
//                 incoming: vec![],
//                 additional_override: vec![],
//                 field: vec![],
//             }
//             .into(),
//             None,
//         )
//         .unwrap();
//     let nt_node_type_2 = lc.add("Node_Type", payload, parent)
//     let mut runner = AnalysisRunner::new();
//     lc.add_analyzer(TypeCheckFactory {}.analyzer_name(), "");
//     runner
//         .register_analyzer_kind(Box::new(TypeCheckFactory {}))
//         .unwrap();

//     let output = runner.run(&lc).unwrap();
//     assert!(output.0.is_empty())
// }
