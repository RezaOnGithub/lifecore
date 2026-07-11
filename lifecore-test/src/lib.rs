//! Integration tests for Lifecore.

#![cfg(test)]

use lifecore::{
    AnalysisReport, AnalysisRunner, AnalyzerFactory, LifeCore, LifeCoreError, LifeCoreList,
    NodeBodyExt, NodeType, NodeTypeClass, NodeTypeFieldStatus, PayloadExt, PayloadType, TypedBody,
};
use lifecore_typecheck::TypeCheckFactory;

fn type_decl(name: &str, fields: &[(&str, (PayloadType, NodeTypeFieldStatus))]) -> NodeType {
    NodeType {
        name: name.to_string(),
        class: NodeTypeClass::Regular,
        outgoing: vec![],
        incoming: vec![],
        additional_override: vec![],
        field: fields
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect(),
    }
}

fn runner_with_typecheck<LC: LifeCore + 'static>() -> AnalysisRunner<LC> {
    let mut runner = AnalysisRunner::new();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory::<LC>::default()))
        .unwrap();
    runner
}

fn configure_typecheck<LC: LifeCore>(lc: &mut LC) {
    lc.add_analyzer(
        TypeCheckFactory::<LC>::default().analyzer_name(),
        "integration",
    );
}

fn generic_runner_reports_missing_type<LC: LifeCore + 'static>(mut lc: LC) {
    lc.add(
        "Missing_Type",
        NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
        None,
    )
    .unwrap();
    configure_typecheck(&mut lc);
    let mut runner = runner_with_typecheck::<LC>();

    let output = runner.run(&lc).unwrap();

    assert!(output.0.iter().any(|report| match report {
    AnalysisReport::Inconsistency(message) =>
        message.message.contains("Type Missing_Type does not exist"),
    _ => false,
    }));
}

#[test]
fn runner_reports_typecheck_analysis_for_generic_lifecore_path() {
    generic_runner_reports_missing_type(LifeCoreList::new());
}

#[test]
fn runner_combines_typecheck_warnings_and_inconsistencies() {
    let mut lc = LifeCoreList::new();
    lc.add(NodeType::name(), NodeType::describe_self().into(), None)
        .unwrap();
    lc.add(
        NodeType::name(),
        type_decl(
            "Task",
            &[("title", (PayloadType::Text, NodeTypeFieldStatus::Mandatory))],
        )
        .into(),
        None,
    )
    .unwrap();
    lc.add("Task", NodeBodyExt::from(&[]), None).unwrap();
    configure_typecheck(&mut lc);
    let mut runner = runner_with_typecheck::<LifeCoreList>();

    let output = runner.run(&lc).unwrap();

    assert!(output.0.iter().any(|report| {
        match report {
        AnalysisReport::Warning(message) => message
            .message
            .contains("Override Tombstone on type Node_Type"),
        _ => false,
        }
    }));
    assert!(output.0.iter().any(|report| match report {
    AnalysisReport::Inconsistency(message) => message.message.contains("mandatory field title"),
    _ => false,
    }));
}

#[test]
fn runner_reports_missing_registered_analyzer_kind() {
    let mut lc = LifeCoreList::new();
    lc.add(NodeType::name(), NodeType::describe_self().into(), None)
        .unwrap();
    lc.add_analyzer("No_Such_Analyzer", "");
    let mut runner = AnalysisRunner::<LifeCoreList>::new();

    let error = runner.run(&lc).unwrap_err();

    assert!(matches!(
        error,
        LifeCoreError::AnalyzerNotFound { name } if name == "No_Such_Analyzer"
    ));
}

#[test]
fn runner_rejects_duplicate_analyzer_factory() {
    let mut runner = AnalysisRunner::<LifeCoreList>::new();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory::<LifeCoreList>::default()))
        .unwrap();

    let error = runner
        .register_analyzer_kind(Box::new(TypeCheckFactory::<LifeCoreList>::default()))
        .unwrap_err();

    assert!(matches!(
        error,
        LifeCoreError::DuplicateAnalyzer { dupe_name } if dupe_name == "Type_Check"
    ));
}
