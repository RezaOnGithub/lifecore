use crate::TypeCheck;
use lifecore::{
    LifeCore, LifeCoreList, NodeBodyExt, NodeType, NodeTypeClass, NodeTypeFieldStatus, PayloadExt,
    PayloadType, TypedBody, analysis, analysis::Analyzer,
};

// TODO rewrite tests against the new interfaces
// fn run_typecheck(lc: &LifeCoreList) -> analysis::Output {
//     TypeCheck {
//         config: "test".to_string(),
//         _lc: std::marker::PhantomData,
//     }
//     .analyze(lc)
// }

// fn messages(output: &AnalysisOutput) -> Vec<&str> {
//     output
//         .0
//         .iter()
//         .filter_map(|report| match report {
//         AnalysisReport::Inconsistency(message) |
// AnalysisReport::Warning(message) => {             
// Some(message.message.as_str())         }
//         _ => None,
//         })
//         .collect()
// }

// fn inconsistency_messages(output: &AnalysisOutput) -> Vec<&str> {
//     output
//         .0
//         .iter()
//         .filter_map(|report| match report {
//         AnalysisReport::Inconsistency(message) =>
// Some(message.message.as_str()),         _ => None,
//         })
//         .collect()
// }

// fn warning_messages(output: &AnalysisOutput) -> Vec<&str> {
//     output
//         .0
//         .iter()
//         .filter_map(|report| match report {
//         AnalysisReport::Warning(message) => Some(message.message.as_str()),
//         _ => None,
//         })
//         .collect()
// }

// fn bootstrap_node_type(lc: &mut LifeCoreList) {
//     lc.add(NodeType::name(), NodeType::describe_self().into(), None)
//         .unwrap();
// }

// #[test]
// fn initialize_suggests_bootstrap_node_type_when_missing() {
//     let lc = LifeCoreList::new();
//     let mut typecheck = TypeCheck {
//         config: "test".to_string(),
//     };

//     let output = typecheck.initialize(&lc);

//     assert!(output.0.iter().any(|report| {
//         matches!(
//             report,
//             AnalysisReport::Suggestion(_, lifecore::SuggestedAction::AddNode
// { ty_name, .. })                 if ty_name == NodeType::name()
//         )
//     }));
// }

// #[test]
// fn node_deserialization01() {
//     let nt_describe_json =
//         serde_json::to_string(&
// Into::<NodeBodyExt>::into(NodeType::describe_self())).unwrap();
//     let nt_describe_hashmap =
//         serde_json::from_str::<NodeBodyExt>(nt_describe_json.as_str()).
// unwrap();

//     let _ = NodeType::try_from(&nt_describe_hashmap).unwrap();
// }

// #[test]
// fn bootstrap_reports_missing_override_type_as_warning() {
//     let mut lc = LifeCoreList::new();
//     bootstrap_node_type(&mut lc);

//     let output = run_typecheck(&lc);

//     assert!(output.is_success());
//     assert!(
//         warning_messages(&output)
//             .iter()
//             .any(|message| message.contains("Override Tombstone on type
// Node_Type"))     );
// }

// #[test]
// fn missing_type_declaration_is_an_inconsistency() {
//     let mut lc = LifeCoreList::new();
//     lc.add(
//         "Missing_Type",
//         NodeBodyExt::from(&[("date",
// PayloadExt::Text("yellow".to_string()))]),         None,
//     )
//     .unwrap();

//     let output = run_typecheck(&lc);

//     assert!(
//         inconsistency_messages(&output)
//             .iter()
//             .any(|message| message.contains("Type Missing_Type does not
// exist"))     );
// }

// #[test]
// fn field_policy_checks_report_expected_warnings_and_errors() {
//     let mut lc = LifeCoreList::new();
//     bootstrap_node_type(&mut lc);
//     lc.add(
//         NodeType::name(),
//         type_decl(
//             "Task",
//             &[
//                 ("must", (PayloadType::Text,
// NodeTypeFieldStatus::Mandatory)),                 (
//                     "should",
//                     (PayloadType::Text, NodeTypeFieldStatus::Encouraged),
//                 ),
//                 ("old", (PayloadType::Text,
// NodeTypeFieldStatus::Deprecated)),                 (
//                     "maybe",
//                     (PayloadType::Number, NodeTypeFieldStatus::Optional),
//                 ),
//             ],
//             &[],
//             &[],
//             &[],
//         )
//         .into(),
//         None,
//     )
//     .unwrap();
//     lc.add(
//         "Task",
//         NodeBodyExt::from(&[
//             ("must", PayloadExt::Number(5)),
//             ("old", PayloadExt::Text("legacy".to_string())),
//             ("extra", PayloadExt::Text("nope".to_string())),
//         ]),
//         None,
//     )
//     .unwrap();

//     let output = run_typecheck(&lc);
//     let inconsistencies = inconsistency_messages(&output);
//     let warnings = warning_messages(&output);

//     assert!(
//         inconsistencies
//             .iter()
//             .any(|message| message.contains("Field must has the wrong type"))
//     );
//     assert!(
//         inconsistencies
//             .iter()
//             .any(|message| message.contains("Extraneous field extra not
// allowed"))     );
//     assert!(
//         warnings
//             .iter()
//             .any(|message| message.contains("encouraged to fill the field
// should"))     );
//     assert!(
//         warnings
//             .iter()
//             .any(|message| message.contains("Field old is deprecated"))
//     );
// }

// #[test]
// fn missing_mandatory_field_is_an_inconsistency() {
//     let mut lc = LifeCoreList::new();
//     bootstrap_node_type(&mut lc);
//     lc.add(
//         NodeType::name(),
//         type_decl(
//             "Task",
//             &[("must", (PayloadType::Text, NodeTypeFieldStatus::Mandatory))],
//             &[],
//             &[],
//             &[],
//         )
//         .into(),
//         None,
//     )
//     .unwrap();
//     lc.add("Task", NodeBodyExt::from(&[]), None).unwrap();

//     let output = run_typecheck(&lc);

//     assert!(
//         inconsistency_messages(&output)
//             .iter()
//             .any(|message| message.contains("mandatory field must"))
//     );
// }

// #[test]
// fn redeclaring_active_type_without_parent_override_is_an_inconsistency() {
//     let mut lc = LifeCoreList::new();
//     bootstrap_node_type(&mut lc);
//     lc.add(
//         NodeType::name(),
//         type_decl("Task", &[], &[], &[], &[]).into(),
//         None,
//     )
//     .unwrap();
//     lc.add(
//         NodeType::name(),
//         type_decl(
//             "Task",
//             &[("note", (PayloadType::Text, NodeTypeFieldStatus::Optional))],
//             &[],
//             &[],
//             &[],
//         )
//         .into(),
//         None,
//     )
//     .unwrap();

//     let output = run_typecheck(&lc);

//     assert!(
//         inconsistency_messages(&output)
//             .iter()
//             .any(|message| message.contains("already declared"))
//     );
// }

// #[test]
// fn redeclaring_type_with_parent_override_is_not_a_duplicate_type_error() {
//     let mut lc = LifeCoreList::new();
//     bootstrap_node_type(&mut lc);
//     let old_type = lc
//         .add(
//             NodeType::name(),
//             type_decl(
//                 "Task",
//                 &[("old", (PayloadType::Text,
// NodeTypeFieldStatus::Optional))],                 &[],
//                 &[],
//                 &[],
//             )
//             .into(),
//             None,
//         )
//         .unwrap();
//     lc.add(
//         NodeType::name(),
//         type_decl(
//             "Task",
//             &[("new", (PayloadType::Number, NodeTypeFieldStatus::Optional))],
//             &[],
//             &[],
//             &[],
//         )
//         .into(),
//         Some(old_type.e),
//     )
//     .unwrap();

//     let output = run_typecheck(&lc);

//     assert!(
//         !messages(&output)
//             .iter()
//             .any(|message| message.contains("Task which was already
// declared"))     );
// }

// #[test]
// fn incoming_edge_permissions_use_incoming_list() {
//     let mut lc = LifeCoreList::new();
//     bootstrap_node_type(&mut lc);
//     lc.add(
//         NodeType::name(),
//         type_decl("Source", &[], &["Link"], &[], &[]).into(),
//         None,
//     )
//     .unwrap();
//     lc.add(
//         NodeType::name(),
//         type_decl("Target", &[], &[], &["Link"], &[]).into(),
//         None,
//     )
//     .unwrap();
//     let source = lc.add("Source", NodeBodyExt::from(&[]), None).unwrap();
//     let target = lc.add("Target", NodeBodyExt::from(&[]), None).unwrap();
//     lc.connect(source.e, "Link", target.e).unwrap();

//     let output = run_typecheck(&lc);

//     assert!(
//         !inconsistency_messages(&output)
//             .iter()
//             .any(|message| message.contains("does not allow outgoing edge of
// type Link")),         "TypeCheck should accept Link because Target lists it
// as incoming: {:#?}",         output
//     );
// }

// #[test]
// fn malformed_node_type_is_reported_without_panicking() {
//     let output = std::panic::catch_unwind(|| {
//         let mut lc = LifeCoreList::new();
//         bootstrap_node_type(&mut lc);
//         lc.add(NodeType::name(), NodeBodyExt::from(&[]), None)
//             .unwrap();
//         run_typecheck(&lc)
//     });

//     assert!(
//         output.is_ok(),
//         "TypeCheck must diagnose an invalid Node_Type body instead of
// unwrapping it"     );
//     assert!(output.unwrap().has_error());
// }

// #[test]
// fn disallowed_incoming_edge_diagnostic_identifies_the_incoming_end() {
//     let mut lc = LifeCoreList::new();
//     bootstrap_node_type(&mut lc);
//     lc.add(
//         NodeType::name(),
//         type_decl("Source", &[], &["Link"], &[], &[]).into(),
//         None,
//     )
//     .unwrap();
//     lc.add(
//         NodeType::name(),
//         type_decl("Target", &[], &[], &[], &[]).into(),
//         None,
//     )
//     .unwrap();
//     let source = lc.add("Source", NodeBodyExt::from(&[]), None).unwrap();
//     let target = lc.add("Target", NodeBodyExt::from(&[]), None).unwrap();
//     lc.connect(source.e, "Link", target.e).unwrap();

//     let output = run_typecheck(&lc);

//     assert!(
//         inconsistency_messages(&output).iter().any(|message| {
//             message
//                 .contains("Edge tail node of type Target does not allow
// incoming edge of type Link")         }),
//         "the diagnostic must describe the Target/incoming check: {output:#?}"
//     );
// }
