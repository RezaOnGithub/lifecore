//! TypeCheck analyzer regression tests maintained by agents.

use lifecore::{
    Body, LifeCore, LifeCoreList, Payload,
    analysis::{AnalysisRunner, AnalyzerFactory},
    serialize::{LifeCoreSerializeExt, NodeBody},
    typing::Type,
};

use crate::TypeCheckFactory;

fn runner() -> AnalysisRunner<LifeCoreList> {
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory::<LifeCoreList>::default()))
        .unwrap();
    runner
}

fn configure(lifecore: &mut LifeCoreList) {
    lifecore.add_analyzer(TypeCheckFactory::<LifeCoreList>::name(), "test2");
}

fn add_bootstrap(lifecore: &mut LifeCoreList) {
    lifecore
        .add_typed_mock(Type::describe_self(), None)
        .unwrap();
}

#[test]
fn runner_accepts_a_valid_typed_edge_across_all_historical_prefixes() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    lifecore
        .add_typed_mock(Type::new("From", &[], &["link"], &[], &[]), None)
        .unwrap();
    lifecore
        .add_typed_mock(Type::new("To", &[], &[], &["link"], &[]), None)
        .unwrap();
    let from = lifecore.add_mock("From", Body::default(), None).unwrap();
    let to = lifecore.add_mock("To", Body::default(), None).unwrap();
    lifecore.connect_mock(from.e, "link", to.e).unwrap();
    configure(&mut lifecore);

    let output = runner().run(&lifecore).unwrap();

    assert!(!output.has_error(), "unexpected output: {output:?}");
}

#[test]
fn malformed_type_body_remains_an_inconsistency() {
    let mut lifecore = LifeCoreList::default();
    lifecore
        .add_mock(
            Type::TYPE_NAME,
            Body::from([("name", Payload::Number(1))]),
            None,
        )
        .unwrap();
    configure(&mut lifecore);

    let output = runner().run(&lifecore).unwrap();

    assert!(
        output
            .errors()
            .iter()
            .any(|message| { message.message == "Unable to parse NodeType body" })
    );
}

#[test]
fn factory_reports_its_stable_registered_name() {
    let factory = TypeCheckFactory::<LifeCoreList>::default();

    assert_eq!(factory.analyzer_name(), "Type_Check");
}
