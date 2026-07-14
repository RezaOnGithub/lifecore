//! Integration tests for Lifecore.

#![cfg(test)]

use lifecore::{
    LifeCore, LifeCoreError, LifeCoreList, NodeBodyExt, NodeType, NodeTypeClass,
    NodeTypeFieldStatus, PayloadExt, PayloadType, TypedBody,
    analysis::{self, AnalyzerFactory},
};
use lifecore_typecheck::TypeCheckFactory;

fn runner_with_typecheck<LC: LifeCore + 'static>() -> analysis::AnalysisRunner<LC> {
    let mut runner = analysis::AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory::<LC>::default()))
        .unwrap();
    runner
}

fn configure_typecheck<LC: LifeCore + 'static>(lc: &mut LC) {
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

    assert!(output.0.iter().any(|report| {
        match report {
        analysis::Message::Inconsistency(message) => message
            .message
            .contains("Missing_Type couldn't be resolved"),
        _ => false,
        }
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
        NodeType::new(
            "Task",
            &[("title", (PayloadType::Text, NodeTypeFieldStatus::Mandatory))],
            &[],
            &[],
            &[],
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
        analysis::Message::Warning(message) => message.message.contains("Override"),
        _ => false,
        }
    }));
    assert!(output.0.iter().any(|report| match report {
    analysis::Message::Inconsistency(message) => message.message.contains("Mandatory"),
    _ => false,
    }));
}

#[test]
fn runner_reports_missing_registered_analyzer_kind() {
    let mut lc = LifeCoreList::new();
    lc.add(NodeType::name(), NodeType::describe_self().into(), None)
        .unwrap();
    lc.add_analyzer("No_Such_Analyzer", "");
    let mut runner = analysis::AnalysisRunner::<LifeCoreList>::default();

    let error = runner.run(&lc).unwrap_err();

    assert!(matches!(
        error,
        LifeCoreError::AnalyzerNotFound { name } if name == "No_Such_Analyzer"
    ));
}

#[test]
fn runner_rejects_duplicate_analyzer_factory() {
    let mut runner = analysis::AnalysisRunner::<LifeCoreList>::default();
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
