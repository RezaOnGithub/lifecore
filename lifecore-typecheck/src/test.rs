use crate::{TypeCheck, TypeCheckFactory};
use env_logger;
use lifecore::{AnalysisRunner, AnalyzerFactory, LifeCore, LifeCoreList, NodeBodyExt, PayloadExt};
use lifecore::{NodeType, TypedBody};
use std::sync::Once;

fn init_test_logging() {
    static LOG_INIT: Once = Once::new();
    LOG_INIT.call_once(|| {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace"))
            .format_timestamp_millis()
            .is_test(false) // comment out when debugging crashes
            .target(env_logger::Target::Stderr)
            .try_init()
            .expect("Could not initialize logger");
    });
}

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
    let mut lc = LifeCoreList::new();
    let _ = lc
        .add(
            "xyz",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let mut runner = AnalysisRunner::new();
    let factory = TypeCheckFactory::<LifeCoreList>::default();
    lc.add_analyzer(factory.analyzer_name(), "");
    runner.register_analyzer_kind(Box::new(factory)).unwrap();

    assert!(runner.run(&lc).unwrap().is_success());
}

#[test]
// Under current typing rules, this should only give a warning
fn bootstrap() {
    init_test_logging();

    let mut lc = LifeCoreList::new();
    let _ = lc
        .add(NodeType::name(), NodeType::describe_self().into(), None)
        .unwrap();
    let mut runner = AnalysisRunner::new();
    let factory = TypeCheckFactory::<LifeCoreList>::default();
    lc.add_analyzer(factory.analyzer_name(), "");
    runner.register_analyzer_kind(Box::new(factory)).unwrap();

    let r = runner.run(&lc).unwrap();
    assert!(r.is_success());
    assert!(r.has_warning())
}

#[test]
fn typing01() {
    let mut lc = LifeCoreList::new();
    let mut runner = AnalysisRunner::new();
    let factory = TypeCheckFactory::<LifeCoreList>::default();
    lc.add_analyzer(factory.analyzer_name(), "");
    runner.register_analyzer_kind(Box::new(factory)).unwrap();
    let _ = lc.add(NodeType::name(), NodeType::describe_self().into(), None);
}

// #[test]
// fn tombstone01() {
//     let mut lc = LifeCoreList::new();
//     let nt_node_type = lc.add("Node_Type", NodeType::describe_self().into(), None).unwrap();
//     let nt_tombstone = lc
//         .add(
//             "Node_Type",
//             NodeType {
//                 name: "Tombstone".to_string(),
//                 class: LifeCoreList::typecheck::NodeTypeClass::Regular,
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
