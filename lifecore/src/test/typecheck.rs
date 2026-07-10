use crate::typecheck::{NodeType, TypeCheck, TypeCheckFactory, TypedBody};
use crate::{
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
#[should_panic]
fn no_bootstrap_typecheck() {
    let mut lc = LifeCore::new();
    let _ = lc
        .add(
            "xyz",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let mut runner = AnalysisRunner::new();
    lc.add_analyzer(TypeCheckFactory {}.analyzer_name(), "");
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory {}))
        .unwrap();

    runner.run(&lc).unwrap().unwrap();
}

#[test]
// Under current typing rules, this should only give a warning
fn bootstrap() {
    let mut lc = LifeCore::new();
    let _ = lc
        .add(NodeType::name(), NodeType::describe_self().into(), None)
        .unwrap();
    let mut runner = AnalysisRunner::new();
    lc.add_analyzer(TypeCheckFactory {}.analyzer_name(), "");
    #[expect(clippy::let_underscore_must_use)]
    let _ = runner
        .register_analyzer_kind(Box::new(TypeCheckFactory {}))
        .unwrap();

    let r = runner.run(&lc).unwrap();
    let _ = r.clone().unwrap();
    assert!(r.has_warning())
}

#[test]
fn typing01() {
    let mut lc = LifeCore::new();
    let mut runner = AnalysisRunner::new();
    lc.add_analyzer(TypeCheckFactory {}.analyzer_name(), "");
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory {}))
        .unwrap();
    let _ = lc
        .add(NodeType::name(), NodeType::describe_self().into(), None)
        .unwrap();
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
