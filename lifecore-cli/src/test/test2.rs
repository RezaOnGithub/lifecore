use std::{collections::BTreeMap, ffi::OsString, fs, io, path::PathBuf};

use lifecore::{
    Body, DiagnosticLocation, EntryIndex, LifeCore, LifeCoreList, OrderedSet, Payload,
    analysis::{AnalyzerFactory, Inconsistency, Message, SuggestedAction, Suggestion, Warning},
    serialize::NodeBody,
    typing::Type,
};
use lifecore_typecheck::TypeCheckFactory;
use uuid::{Uuid, Version};

use crate::{
    InvocationContext, Io,
    analysis::{
        AnalysisReport, AnalyzerRegistry, configured_analyzers, initial_graph, run_analysis,
    },
    app::{CircumstanceRecord, CommandOutcome},
    effective::{EffectiveState, prepare_effective, reject_attached_mutation},
    entry::entry_record,
    error::AppError,
    graphviz::{
        GraphvizExporter, GraphvizInput, GraphvizKind, GraphvizOptions, GraphvizTarget, generate,
    },
    interactive_analyzer::InteractiveFactory,
    locator::{
        EntryExpectation, EntryFingerprint, EntryLocator, entry_fingerprint_identity_bytes,
        resolve_entry, resolve_index,
    },
    messages::{
        MessageFilter, MessageLocation, MessageUniverse, Severity, encode_lower_hex,
        message_identity_bytes,
    },
    output::{render_error, render_outcome},
    payload_input::{parse_field_assignment, parse_field_assignments},
    query::{execute_query, parse_predicate, parse_query},
    reference::{AliasStatus, NodeReference, resolve_node_reference},
    run_cli, run_from_in,
    sidecar::{build_sidecar_plan, parse_sidecar},
    state::{PersistedState, StatePath, StateStore, StoredEntryLocatorV1},
    transaction::{
        ExpectedHead, PlannedChange, TransactionRequest, TransactionService, TransactionState,
        verify_expected_head,
    },
    view::{GraphView, ViewSpec},
};

fn test_io<'a>(stdout: &'a mut Vec<u8>, stderr: &'a mut Vec<u8>) -> Io<'a> {
    Io::new(stdout, stderr, false)
}

fn run(args: &[&str]) -> Result<CommandOutcome, AppError> {
    let directory = tempfile::tempdir().expect("temporary invocation directory");
    run_in(args, directory.path(), None)
}

fn run_in(
    args: &[&str],
    directory: &std::path::Path,
    state_path_environment: Option<OsString>,
) -> Result<CommandOutcome, AppError> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let io = test_io(&mut stdout, &mut stderr);
    let invocation = InvocationContext::new(directory, state_path_environment);

    run_from_in(args, &io, &invocation)
}

fn assert_usage_error(args: &[&str], expected: &str) {
    match run(args) {
    Err(AppError::Usage { message, .. }) => {
        assert!(
            message.contains(expected),
            "expected {expected:?} in {message:?}"
        );
    }
    result => panic!("expected usage error, got {result:?}"),
    }
}

#[test]
fn help_and_version_are_successful_at_every_command_level() {
    let help_paths: &[&[&str]] = &[
        &["lifecore", "--help"],
        &["lifecore", "init", "--help"],
        &["lifecore", "entry", "--help"],
        &["lifecore", "entry", "show", "--help"],
        &["lifecore", "add", "--help"],
        &["lifecore", "connect", "--help"],
        &["lifecore", "query", "--help"],
        &["lifecore", "query", "nodes", "--help"],
        &["lifecore", "alias", "--help"],
        &["lifecore", "alias", "list", "--help"],
        &["lifecore", "alias", "set", "--help"],
        &["lifecore", "alias", "remove", "--help"],
        &["lifecore", "type", "--help"],
        &["lifecore", "type", "list", "--help"],
        &["lifecore", "type", "show", "--help"],
        &["lifecore", "type", "create", "--help"],
        &["lifecore", "type", "override", "--help"],
        &["lifecore", "sidecar", "--help"],
        &["lifecore", "sidecar", "attach", "--help"],
        &["lifecore", "sidecar", "detach", "--help"],
        &["lifecore", "sidecar", "dump", "--help"],
        &["lifecore", "sidecar", "commit", "--help"],
        &["lifecore", "analyzer", "--help"],
        &["lifecore", "analyzer", "list", "--help"],
        &["lifecore", "analyzer", "add", "--help"],
        &["lifecore", "graphviz", "--help"],
        &["lifecore", "check", "--help"],
        &["lifecore", "message", "--help"],
        &["lifecore", "message", "show", "--help"],
        &["lifecore", "message", "dismiss", "--help"],
        &["lifecore", "message", "restore", "--help"],
        &["lifecore", "message", "apply", "--help"],
    ];

    for args in help_paths {
        match run(args) {
        Ok(CommandOutcome::Text(text)) => {
            assert!(text.contains("Usage: lifecore"), "args: {args:?}");
        }
        result => panic!("help failed for {args:?}: {result:?}"),
        }
    }

    match run(&["lifecore", "--version"]) {
    Ok(CommandOutcome::Text(text)) => assert_eq!(text, "lifecore 0.1.0\n"),
    result => panic!("version failed: {result:?}"),
    }

    match run(&["lifecore", "--help"]) {
    Ok(CommandOutcome::Text(text)) => {
        assert!(!text.contains("interactive"));
        assert!(!text.contains("-i,"));
        assert!(!text.contains("--interactive"));
    }
    result => panic!("root help failed: {result:?}"),
    }
}

#[test]
fn removed_interactive_forms_are_usage_errors() {
    let cases: &[&[&str]] = &[
        &["lifecore", "interactive"],
        &["lifecore", "interactive", "--format", "table"],
        &["lifecore", "interactive", "--format", "json"],
    ];

    for args in cases {
        match run(args) {
        Err(AppError::Usage { message, exit_code }) => {
            assert_eq!(exit_code, 2, "args: {args:?}");
            assert!(
                message.contains("unrecognized subcommand 'interactive'"),
                "args: {args:?}, message: {message:?}"
            );
        }
        result => panic!("removed interactive form was accepted for {args:?}: {result:?}"),
        }
    }
}

#[test]
fn query_selection_actions_have_typed_nested_grammar() {
    assert!(matches!(
        run(&[
            "lifecore",
            "query",
            "nodes",
            "all",
            "--select",
            "4:abcde",
            "--",
            "type",
            "override",
            "--as",
            "FocusedTask",
            "--field",
            "priority = Number : Optional",
            "--commit",
        ],),
        Err(AppError::StateIo { .. })
    ));

    assert_usage_error(
        &[
            "lifecore", "query", "nodes", "all", "--select", "4:abcde", "--", "alias", "remove",
            "chosen",
        ],
        "unrecognized subcommand",
    );
}

#[test]
fn structural_conflicts_fail_before_dispatch() {
    let cases: &[(&[&str], &str)] = &[
        (&["lifecore"], "a command is required"),
        (&["lifecore", "-i"], "unexpected argument '-i'"),
        (
            &["lifecore", "add", "Note", "--expect-head", "EMPTY"],
            "--commit",
        ),
        (
            &[
                "lifecore",
                "query",
                "nodes",
                "all",
                "--select",
                "4:abcde",
                "--explain",
                "5:abcde",
            ],
            "cannot be used with",
        ),
        (
            &[
                "lifecore", "query", "nodes", "all", "--", "entry", "inspect",
            ],
            "--select",
        ),
        (
            &[
                "lifecore",
                "check",
                "--include-dismissed",
                "--only-dismissed",
            ],
            "cannot be used with",
        ),
        (
            &["lifecore", "check", "--dismissal-ids", "--include-resolved"],
            "cannot be used with",
        ),
    ];

    for (args, expected) in cases {
        assert_usage_error(args, expected);
    }

    assert!(matches!(
        run(&[
            "lifecore",
            "message",
            "apply",
            "m:abcde",
            "--commit",
            "--expect-head",
            "HEAD",
        ]),
        Err(AppError::InvalidExpectedHead { .. })
    ));
}

#[test]
fn format_is_available_only_on_output_commands() {
    assert_usage_error(
        &["lifecore", "--format", "json", "query", "nodes", "all"],
        "unexpected argument '--format'",
    );
    assert_usage_error(
        &["lifecore", "graphviz", "graph.svg", "--format", "json"],
        "unexpected argument '--format'",
    );
    assert_usage_error(
        &[
            "lifecore",
            "sidecar",
            "attach",
            "entries.toml",
            "--format",
            "json",
        ],
        "unexpected argument '--format'",
    );
}

#[test]
fn removed_interactive_forms_exit_two_on_stderr() {
    let cases: &[&[&str]] = &[
        &["lifecore", "interactive"],
        &["lifecore", "interactive", "--format", "table"],
        &["lifecore", "interactive", "--format", "json"],
    ];

    for args in cases {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = test_io(&mut stdout, &mut stderr);
        let exit = run_cli(args.iter().copied(), &mut io);
        let stderr = String::from_utf8(stderr).expect("stderr is UTF-8");

        assert_eq!(exit, std::process::ExitCode::from(2), "args: {args:?}");
        assert!(stdout.is_empty(), "args: {args:?}");
        assert!(
            stderr.contains("unrecognized subcommand 'interactive'"),
            "args: {args:?}, stderr: {stderr:?}"
        );
    }
}

#[test]
fn rejected_interactive_forms_do_not_create_or_modify_files() -> io::Result<()> {
    let directory = tempfile::tempdir()?;
    let sentinel = directory.path().join("sentinel");
    fs::write(&sentinel, b"unchanged")?;
    let cases: &[&[&str]] = &[
        &["lifecore", "interactive"],
        &["lifecore", "interactive", "--format", "table"],
        &["lifecore", "interactive", "--format", "json"],
    ];
    let before = directory_snapshot(directory.path())?;

    for args in cases {
        assert!(matches!(
            run_in(args, directory.path(), None),
            Err(AppError::Usage { exit_code: 2, .. })
        ));
    }

    let after = directory_snapshot(directory.path())?;
    assert_eq!(after, before);
    let state_path = directory.path().join("LifeCoreState.json");
    assert!(!state_path.exists());

    fs::write(&state_path, b"existing state bytes")?;
    let before = directory_snapshot(directory.path())?;
    for args in cases {
        assert!(matches!(
            run_in(args, directory.path(), None),
            Err(AppError::Usage { exit_code: 2, .. })
        ));
    }
    let after = directory_snapshot(directory.path())?;

    assert_eq!(after, before);
    Ok(())
}

#[test]
fn init_creates_canonical_state_and_read_only_diagnostics_preserve_bytes() {
    let directory = tempfile::tempdir().expect("temporary state directory");

    let initialized =
        run_in(&["lifecore", "init"], directory.path(), None).expect("initialization succeeds");
    let (message_id, uuid) = match initialized {
    CommandOutcome::Initialized {
        view,
        messages,
        uuid,
        ..
    } => {
        assert_eq!(view.end, 0);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].severity, Severity::Suggestion);
        assert_eq!(messages[0].analyzer, "Type_Check");
        assert_eq!(messages[0].actions.len(), 1);

        (messages[0].display_id.clone(), uuid)
    }
    outcome => panic!("expected initialized outcome, got {outcome:?}"),
    };

    let state_path = directory.path().join("LifeCoreState.json");
    let before = fs::read(&state_path).expect("initialized state is readable");
    assert_eq!(before.last(), Some(&b'\n'));

    let envelope: serde_json::Value = serde_json::from_slice(&before).expect("state is valid JSON");
    let top_level = envelope
        .as_object()
        .expect("state envelope is an object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(top_level, vec!["cli", "lifecore"]);
    assert_eq!(envelope["cli"]["version"], 1);
    assert_eq!(envelope["cli"]["entry_fingerprint_version"], 1);
    assert_eq!(envelope["cli"]["message_id_version"], 1);
    assert_eq!(
        uuid::Uuid::parse_str(&uuid)
            .expect("state UUID parses")
            .get_version(),
        Some(Version::SortRand)
    );

    let store = StateStore::new(
        StatePath::resolve(None, directory.path()).expect("default state path resolves"),
    );
    let state = store.load().expect("canonical state reloads");
    let configured = state
        .lifecore
        .current_analysis_suite()
        .0
        .into_iter()
        .map(|config| (config.name, config.config))
        .collect::<Vec<_>>();
    assert_eq!(
        configured,
        vec![
            ("Type_Check".to_string(), String::new()),
            ("Interactive".to_string(), String::new())
        ]
    );

    match run_in(&["lifecore", "init"], directory.path(), None) {
    Err(AppError::StateIo { source, .. }) => {
        assert_eq!(source.kind(), io::ErrorKind::AlreadyExists);
    }
    result => panic!("second init must not clobber state, got {result:?}"),
    }

    let checked = run_in(&["lifecore", "check"], directory.path(), None).expect("check succeeds");
    match checked {
    CommandOutcome::Messages { items, detail, .. } => {
        assert!(!detail);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].display_id, message_id);
    }
    outcome => panic!("expected check messages, got {outcome:?}"),
    }

    let shown = run_in(
        &["lifecore", "message", "show", &message_id],
        directory.path(),
        None,
    )
    .expect("message show resolves a display ID");
    match shown {
    CommandOutcome::Messages { items, detail, .. } => {
        assert!(detail);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].display_id, message_id);
    }
    outcome => panic!("expected message detail, got {outcome:?}"),
    }

    match run_in(
        &["lifecore", "message", "show", "m:00000"],
        directory.path(),
        None,
    ) {
    Err(AppError::UnknownMessageId { .. }) => {}
    result => panic!("unknown message prefix should fail, got {result:?}"),
    }

    let after = fs::read(state_path).expect("state remains readable");
    assert_eq!(after, before);
}

#[test]
fn state_path_override_is_authoritative_and_never_falls_back() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    let override_path = directory.path().join("authoritative.json");

    run_in(
        &["lifecore", "init"],
        directory.path(),
        Some(override_path.clone().into_os_string()),
    )
    .expect("override initialization succeeds");

    assert!(override_path.exists());
    assert!(!directory.path().join("LifeCoreState.json").exists());

    let missing_parent = directory.path().join("missing").join("state.json");
    match run_in(
        &["lifecore", "check"],
        directory.path(),
        Some(missing_parent.into_os_string()),
    ) {
    Err(AppError::StateIo { source, .. }) => {
        assert_eq!(source.kind(), io::ErrorKind::NotFound);
    }
    result => panic!("authoritative missing path must fail, got {result:?}"),
    }

    assert!(!directory.path().join("LifeCoreState.json").exists());
}

#[test]
fn canonical_store_rejects_mock_state_and_missing_analyzer_factories() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    let store = StateStore::new(
        StatePath::resolve(None, directory.path()).expect("default state path resolves"),
    );
    let mut graph = LifeCoreList::default();
    graph
        .add_mock("Provisional", Body::default(), None)
        .expect("mock append is basically valid");

    match store.initialize(&PersistedState::new(graph)) {
    Err(AppError::InvalidState { message, .. }) => {
        assert!(message.contains("Mock"));
    }
    result => panic!("persisted Mock must be rejected, got {result:?}"),
    }
    assert!(!store.path().exists());

    match run_analysis(&initial_graph(), &AnalyzerRegistry::empty()) {
    Err(AppError::Analysis { message, .. }) => {
        assert!(message.contains("not found"));
    }
    _ => panic!("missing analyzer factory must fail"),
    }
}

#[test]
fn canonical_load_rejects_unsupported_versions_and_unknown_envelope_fields() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    run_in(&["lifecore", "init"], directory.path(), None).expect("initialization succeeds");
    let path = directory.path().join("LifeCoreState.json");
    let original = fs::read(&path).expect("state bytes are readable");
    let mut value: serde_json::Value =
        serde_json::from_slice(&original).expect("state JSON parses");

    value["cli"]["version"] = serde_json::json!(2);
    fs::write(
        &path,
        serde_json::to_vec_pretty(&value).expect("changed state serializes"),
    )
    .expect("changed state is written");
    match run_in(&["lifecore", "check"], directory.path(), None) {
    Err(AppError::InvalidState { message, .. }) => {
        assert!(message.contains("unsupported CLI state version"));
    }
    result => panic!("unsupported state version must fail, got {result:?}"),
    }

    let mut value: serde_json::Value =
        serde_json::from_slice(&original).expect("original state JSON parses");
    value["unexpected"] = serde_json::json!(true);
    fs::write(
        &path,
        serde_json::to_vec_pretty(&value).expect("changed state serializes"),
    )
    .expect("changed state is written");
    match run_in(&["lifecore", "check"], directory.path(), None) {
    Err(AppError::StateDecode { source, .. }) => {
        assert!(source.to_string().contains("unknown field"));
    }
    result => panic!("unknown envelope field must fail, got {result:?}"),
    }
}

#[test]
fn stage2_outcomes_render_deterministically_in_json_table_lines_and_auto() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    let initialized = run_in(
        &["lifecore", "init", "--format", "json"],
        directory.path(),
        None,
    )
    .expect("JSON initialization succeeds");
    let mut json_output = Vec::new();
    render_outcome(&initialized, &mut json_output, false).expect("JSON rendering succeeds");
    let json: serde_json::Value =
        serde_json::from_slice(&json_output).expect("initialization output is JSON");
    assert_eq!(json["transaction"]["state"], "saved");
    assert_eq!(json["transaction"]["entries"], serde_json::json!([]));
    assert_eq!(json["transaction"]["messages"][0]["severity"], "suggestion");
    assert_eq!(json["view"]["end"], 0);

    let table = run_in(
        &["lifecore", "check", "--format", "table"],
        directory.path(),
        None,
    )
    .expect("table check succeeds");
    let mut table_output = Vec::new();
    render_outcome(&table, &mut table_output, false).expect("table rendering succeeds");
    let table_output = String::from_utf8(table_output).expect("table output is UTF-8");
    assert!(
        table_output
            .starts_with("ID\tSEVERITY\tANALYZER\tLOCATION\tSTATE\tRELEVANCE\tMESSAGE\tACTION\n")
    );

    let lines = run_in(
        &["lifecore", "check", "--format", "lines"],
        directory.path(),
        None,
    )
    .expect("lines check succeeds");
    let mut lines_output = Vec::new();
    render_outcome(&lines, &mut lines_output, true).expect("lines rendering succeeds");
    let lines_output = String::from_utf8(lines_output).expect("lines output is UTF-8");
    assert!(lines_output.starts_with("m:"));
    assert!(!lines_output.starts_with("ID\t"));

    let automatic =
        run_in(&["lifecore", "check"], directory.path(), None).expect("automatic check succeeds");
    let mut tty_output = Vec::new();
    render_outcome(&automatic, &mut tty_output, true).expect("TTY rendering succeeds");
    assert!(
        String::from_utf8(tty_output)
            .expect("TTY output is UTF-8")
            .starts_with("ID\t")
    );

    let mut non_tty_output = Vec::new();
    render_outcome(&automatic, &mut non_tty_output, false).expect("line rendering succeeds");
    assert!(
        String::from_utf8(non_tty_output)
            .expect("line output is UTF-8")
            .starts_with("m:")
    );
}

#[test]
fn message_identity_deduplicates_and_preserves_action_order() {
    let graph = LifeCoreList::default();
    let warning = |relevance| {
        Message::Warning(Warning {
            offending_event: DiagnosticLocation::Context(EntryIndex(3).before()),
            system_name: "Test".to_string(),
            system_config: "{}".to_string(),
            relevance,
            message: "same".to_string(),
        })
    };
    let duplicate_report = AnalysisReport {
        suite: Default::default(),
        messages: vec![
            warning(vec![EntryIndex(2), EntryIndex(1)]),
            warning(vec![EntryIndex(1), EntryIndex(2)]),
        ],
    };
    let duplicate_universe =
        MessageUniverse::from_analysis(&duplicate_report, &graph, &[]).expect("messages encode");
    assert_eq!(duplicate_universe.records().len(), 1);

    let action_a = SuggestedAction::AddNode {
        ty_name: "A".to_string(),
        payload: Body::default(),
        parent: None,
    };
    let action_b = SuggestedAction::AddNode {
        ty_name: "B".to_string(),
        payload: Body::default(),
        parent: None,
    };
    let suggestion = |actions| {
        Message::Suggestion(Suggestion {
            offending_event: DiagnosticLocation::Context(EntryIndex(0).before()),
            system_name: "Test".to_string(),
            system_config: "{}".to_string(),
            relevance: Vec::new(),
            message: "ordered".to_string(),
            action: actions,
        })
    };
    let ordered_report = AnalysisReport {
        suite: Default::default(),
        messages: vec![
            suggestion(vec![action_a.clone(), action_b.clone()]),
            suggestion(vec![action_b, action_a]),
        ],
    };
    let ordered_universe =
        MessageUniverse::from_analysis(&ordered_report, &graph, &[]).expect("messages encode");
    assert_eq!(ordered_universe.records().len(), 2);
    assert_ne!(
        ordered_universe.records()[0].id,
        ordered_universe.records()[1].id
    );
}

#[test]
fn message_identity_v1_has_golden_wire_bytes_and_digest() {
    let graph = LifeCoreList::default();
    let warning = Message::Warning(Warning {
        offending_event: DiagnosticLocation::Context(EntryIndex(0).before()),
        system_name: "Test".to_string(),
        system_config: "{}".to_string(),
        relevance: Vec::new(),
        message: "same".to_string(),
    });
    let bytes = message_identity_bytes(&warning).expect("message identity serializes");
    let report = AnalysisReport {
        suite: Default::default(),
        messages: vec![warning],
    };
    let universe =
        MessageUniverse::from_analysis(&report, &graph, &[]).expect("message identity hashes");

    assert_eq!(
        bytes,
        vec![
            19, 108, 105, 102, 101, 99, 111, 114, 101, 45, 109, 101, 115, 115, 97, 103, 101, 45,
            105, 100, 1, 1, 4, 84, 101, 115, 116, 2, 123, 125, 0, 0, 4, 115, 97, 109, 101, 0, 0,
        ]
    );
    assert_eq!(
        universe.records()[0].id,
        "f11a41decf7a856c48341abe8473fdd70a279ded1dd5145dc20876fbebf2d85a532fa1a7355d65666a60afe56d58b00eecc9439fd29409540982958417987c3c"
    );
}

#[test]
fn fixed_digest_hex_is_exact_lowercase_with_leading_zeroes() {
    let mut digest = [0_u8; 64];
    digest[0] = 0x01;
    digest[1] = 0xaf;
    digest[63] = 0xf0;

    let encoded = encode_lower_hex(&digest);

    assert_eq!(encoded, format!("01af{}f0", "00".repeat(61)));
}

#[test]
fn analyzer_configuration_uses_registered_factory_names() {
    let graph = initial_graph();
    let report =
        run_analysis(&graph, &AnalyzerRegistry::built_in()).expect("built-in analyzers run");
    let configured_names = configured_analyzers(&report)
        .iter()
        .map(|config| config.name.as_str())
        .collect::<Vec<_>>();
    let type_check = TypeCheckFactory::<LifeCoreList>::default();
    let interactive = InteractiveFactory;

    assert_eq!(
        configured_names,
        vec![type_check.analyzer_name(), interactive.analyzer_name()]
    );
}

#[test]
fn message_resolution_uses_the_diagnostic_window() {
    let mut graph = LifeCoreList::default();
    let parent = graph
        .add_mock("Test", Body::default(), None)
        .expect("parent is added");
    let child = graph
        .add_mock("Test", Body::default(), Some(parent.e))
        .expect("child is added");
    let warning_at = |offending_event| {
        Message::Warning(Warning {
            offending_event: DiagnosticLocation::Entry(offending_event),
            system_name: "Test".to_string(),
            system_config: String::new(),
            relevance: Vec::new(),
            message: "bounded".to_string(),
        })
    };
    let historical = AnalysisReport {
        suite: Default::default(),
        messages: vec![warning_at(parent.e)],
    };
    let current = AnalysisReport {
        suite: Default::default(),
        messages: vec![warning_at(child.e.window().look_at(parent.e))],
    };

    let historical = MessageUniverse::from_analysis(&historical, &graph, &[])
        .expect("historical message normalizes");
    let current =
        MessageUniverse::from_analysis(&current, &graph, &[]).expect("current message normalizes");

    assert!(!historical.records()[0].resolved);
    assert!(current.records()[0].resolved);
}

#[test]
fn message_prefix_resolution_detects_syntax_unknown_and_ambiguity() {
    let graph = LifeCoreList::default();
    let mut messages = Vec::new();

    for index in 0..8_000 {
        messages.push(Message::Warning(Warning {
            offending_event: DiagnosticLocation::Context(EntryIndex(0).before()),
            system_name: "Collision_Test".to_string(),
            system_config: String::new(),
            relevance: Vec::new(),
            message: format!("message {index}"),
        }));
    }

    let report = AnalysisReport {
        suite: Default::default(),
        messages,
    };
    let universe = MessageUniverse::from_analysis(&report, &graph, &[]).expect("messages encode");
    let mut prefixes = std::collections::BTreeMap::<String, usize>::new();
    let ambiguous = universe.records().iter().find_map(|record| {
        let prefix = record.id[..5].to_string();
        let count = prefixes.entry(prefix.clone()).or_default();
        *count += 1;
        (*count == 2).then_some(format!("m:{prefix}"))
    });
    let ambiguous = ambiguous.expect("8,000 deterministic IDs contain a five-digit collision");

    match universe.resolve(&ambiguous) {
    Err(AppError::AmbiguousMessageId { .. }) => {}
    result => panic!("ambiguous prefix should fail, got {result:?}"),
    }
    match universe.resolve("bad") {
    Err(AppError::InvalidMessageId { .. }) => {}
    result => panic!("malformed prefix should fail, got {result:?}"),
    }
    match universe.resolve("m:fffff") {
    Err(AppError::UnknownMessageId { .. }) | Err(AppError::AmbiguousMessageId { .. }) => {}
    result => panic!("valid prefix should resolve or report absence, got {result:?}"),
    }

    let record = &universe.records()[0];
    let prefix = &record.id[..5];
    let alternate = format!(
        "{}{}",
        prefix,
        if record.id.as_bytes()[5] == b'a' {
            "b".repeat(123)
        } else {
            "a".repeat(123)
        }
    );
    let dismissal = crate::state::DismissalRecord {
        version: 1,
        digest: alternate,
    };
    assert!(matches!(
        universe.resolve_with_dismissals(&format!("m:{prefix}"), &[dismissal]),
        Err(AppError::AmbiguousMessageId { .. })
    ));
}

#[test]
fn inconsistencies_remain_visible_even_when_their_id_is_dismissed() {
    let graph = LifeCoreList::default();
    let report = AnalysisReport {
        suite: Default::default(),
        messages: vec![Message::Inconsistency(Inconsistency {
            offending_event: DiagnosticLocation::Context(EntryIndex(0).before()),
            system_name: "Test".to_string(),
            system_config: String::new(),
            message: "broken".to_string(),
        })],
    };
    let first =
        MessageUniverse::from_analysis(&report, &graph, &[]).expect("message identity is computed");
    let dismissal = crate::state::DismissalRecord {
        version: 1,
        digest: first.records()[0].id.clone(),
    };
    let dismissed = MessageUniverse::from_analysis(&report, &graph, &[dismissal])
        .expect("dismissal comparison succeeds");

    assert_eq!(dismissed.visible().len(), 1);
    assert!(!dismissed.records()[0].dismissed);
}

fn stage3_state() -> PersistedState {
    let mut graph = initial_graph();
    let original = graph
        .add_mock(
            "Note",
            Body::from([
                ("title", Payload::Text("Draft note".to_string())),
                ("score", Payload::Number(1)),
                (
                    "history",
                    Payload::Sequence(vec![Payload::Text("draft".to_string()), Payload::Number(1)]),
                ),
                (
                    "tags",
                    Payload::OrderedSet(OrderedSet::from([
                        Payload::Text("archived".to_string()),
                        Payload::Text("review".to_string()),
                    ])),
                ),
            ]),
            None,
        )
        .expect("original node is valid");
    let task = graph
        .add_mock(
            "Task",
            Body::from([
                ("title", Payload::Text("Pay invoice".to_string())),
                ("score", Payload::Number(2)),
                (
                    "history",
                    Payload::Sequence(vec![Payload::Text("open".to_string())]),
                ),
                (
                    "tags",
                    Payload::OrderedSet(OrderedSet::from([Payload::Text("urgent".to_string())])),
                ),
            ]),
            None,
        )
        .expect("task node is valid");
    let revised = graph
        .add_mock(
            "Note",
            Body::from([
                ("title", Payload::Text("Published note".to_string())),
                ("score", Payload::Number(3)),
                (
                    "history",
                    Payload::Sequence(vec![
                        Payload::Text("draft".to_string()),
                        Payload::Text("published".to_string()),
                    ]),
                ),
                (
                    "tags",
                    Payload::OrderedSet(OrderedSet::from([Payload::Text("review".to_string())])),
                ),
            ]),
            Some(original.e),
        )
        .expect("successor node is valid");
    graph
        .connect_mock(revised.e, "Relates_To", task.e)
        .expect("final edge is valid");
    graph.apply().expect("fixture suffix applies");

    let mut state = PersistedState::new(graph);
    state.cli.uuid =
        Uuid::parse_str("019f9b41-fa40-7f43-8f99-c5a480f9027c").expect("fixed UUID is valid");
    state
}

fn initialize_stage3_state(directory: &std::path::Path) -> PersistedState {
    let state = stage3_state();
    let store = StateStore::new(
        StatePath::resolve(None, directory).expect("default Stage 3 state path resolves"),
    );
    store.initialize(&state).expect("Stage 3 state initializes");
    state
}

fn stage7_state() -> PersistedState {
    let mut graph = LifeCoreList::default();
    let original = graph
        .add_mock("Note", Body::default(), None)
        .expect("original Stage 7 node is valid");
    let task = graph
        .add_mock("Task", Body::default(), None)
        .expect("Stage 7 task is valid");
    let revised = graph
        .add_mock("Note", Body::default(), Some(original.e))
        .expect("revised Stage 7 node is valid");
    graph
        .connect_mock(revised.e, "Relates_To", task.e)
        .expect("Stage 7 edge is valid");
    graph.apply().expect("Stage 7 fixture applies");

    PersistedState::new(graph)
}

fn initialize_stage7_state(directory: &std::path::Path) -> PersistedState {
    let state = stage7_state();
    let store = StateStore::new(
        StatePath::resolve(None, directory).expect("default Stage 7 state path resolves"),
    );
    store.initialize(&state).expect("Stage 7 state initializes");
    state
}

fn initialize_stage8_state(directory: &std::path::Path) -> PersistedState {
    let initialized =
        run_in(&["lifecore", "init"], directory, None).expect("Stage 8 state initializes");
    let suggestion = match initialized {
    CommandOutcome::Initialized { messages, .. } => messages
        .into_iter()
        .next()
        .expect("bootstrap suggestion is emitted"),
    outcome => panic!("expected initialized Stage 8 state, got {outcome:?}"),
    };
    run_in(
        &[
            "lifecore",
            "message",
            "apply",
            &suggestion.display_id,
            "--commit",
            "--expect-head",
            "EMPTY",
        ],
        directory,
        None,
    )
    .expect("bootstrap declaration commits");

    StateStore::new(StatePath::resolve(None, directory).expect("Stage 8 state path resolves"))
        .load()
        .expect("bootstrapped Stage 8 state loads")
}

fn initialize_stage9_ready_state(directory: &std::path::Path) -> (PersistedState, String, String) {
    initialize_stage8_state(directory);
    run_in(
        &[
            "lifecore",
            "type",
            "create",
            "Note",
            "--field",
            "title = Text : Optional",
            "--field",
            "tags = OrderedSet(Text) : Optional",
            "--outgoing",
            "Rel",
            "--incoming",
            "Rel",
            "--commit",
        ],
        directory,
        None,
    )
    .expect("Stage 9 Note declaration commits");
    run_in(
        &[
            "lifecore",
            "add",
            "Note",
            "--field",
            "title = \"base\"",
            "--commit",
        ],
        directory,
        None,
    )
    .expect("Stage 9 base node commits");
    run_in(
        &[
            "lifecore",
            "add",
            "Note",
            "--field",
            "title = \"other\"",
            "--commit",
        ],
        directory,
        None,
    )
    .expect("Stage 9 other node commits");

    let store =
        StateStore::new(StatePath::resolve(None, directory).expect("Stage 9 state path resolves"));
    let state = store.load().expect("Stage 9 nodes load");
    let view = GraphView::latest(&state.lifecore, &state.cli).expect("Stage 9 view constructs");
    let base = resolve_index(
        &state.lifecore,
        &state.cli,
        2,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )
    .expect("Stage 9 base resolves");
    let other = resolve_index(
        &state.lifecore,
        &state.cli,
        3,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )
    .expect("Stage 9 other resolves");
    let base_locator = base.display_locator();
    let other_locator = other.display_locator();

    run_in(
        &[
            "lifecore",
            "connect",
            "--from",
            &base_locator,
            "--type",
            "Rel",
            "--to",
            &other_locator,
            "--commit",
        ],
        directory,
        None,
    )
    .expect("Stage 9 canonical edge commits");
    run_in(
        &["lifecore", "alias", "set", "Base", &base_locator],
        directory,
        None,
    )
    .expect("Stage 9 Base alias saves");
    run_in(
        &["lifecore", "alias", "set", "Other", &other_locator],
        directory,
        None,
    )
    .expect("Stage 9 Other alias saves");

    (
        store.load().expect("Stage 9 ready state reloads"),
        base_locator,
        other_locator,
    )
}

fn stage9_ready_sidecar() -> &'static str {
    r#"
[[entry]]
alias = "SideTypeAlias"
ty = "Type"

[entry.body]
name = "SideType"
body = { ordered_set = [] }
outgoing = { ordered_set = [] }
incoming = { ordered_set = [] }
additional_override = { ordered_set = [] }

[[entry]]
alias = "TempFirst"
ty = "Note"

[entry.body]
title = "provisional first"
tags = { ordered_set = ["home", "errand"] }

[entry.connect]
connect_to = [{ target = "Other", ty = "Rel" }]
connect_back_from = [{ target = "Other", ty = "Rel" }]

[[entry]]
alias = "TempChild"
ty = "Note"
parent = "Base"

[entry.body]
title = "provisional child"

[[entry]]
alias = "TempLast"
ty = "Note"
parent = "HEAD"

[entry.body]
title = "provisional last"
"#
}

#[test]
fn entry_fingerprint_wire_and_digest_are_stable() {
    let state = stage3_state();
    let entry = state
        .lifecore
        .get(EntryIndex(0).before_and_including())
        .expect("first fixture entry exists");
    let bytes = entry_fingerprint_identity_bytes(&state.cli, EntryIndex(0), &entry)
        .expect("fingerprint input encodes");
    let fingerprint =
        EntryFingerprint::for_entry(&state.cli, EntryIndex(0), &entry).expect("fingerprint hashes");

    let wire_hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(
        wire_hex,
        "1e6c696665636f72652d636c692d656e7472792d66696e6765727072696e7401019f9b41fa407f438f99c5a480f9027c0000044e6f746504"
    );
    assert_eq!(
        fingerprint.as_str(),
        "e0f12e6ec9c62fa65e6bc99566ee2cea4a6599a05b2211bb5a4ee30dae1977438cabbf95796ed6e5de086e0113a483e81640ffe5ed8ec0f8e796954427cdbb9c"
    );
}

#[test]
fn locator_resolution_is_strict_and_checks_in_the_required_order() {
    let state = stage3_state();
    let view = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let node = resolve_index(
        &state.lifecore,
        &state.cli,
        2,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )
    .expect("node resolves");
    let edge = resolve_index(
        &state.lifecore,
        &state.cli,
        3,
        EntryExpectation::Any,
        Some(view.end_exclusive()),
    )
    .expect("edge resolves");

    for invalid in [
        "",
        "2",
        " 2:abcde",
        "+2:abcde",
        "2:abcd",
        "2:ABCDE",
        "2:abcde:",
        "184467440737095516161:abcde",
    ] {
        assert!(
            matches!(
                EntryLocator::parse(invalid),
                Err(AppError::InvalidEntryLocator { .. })
            ),
            "{invalid:?}"
        );
    }

    let unknown = EntryLocator::parse("99:abcde").expect("locator grammar is valid");
    assert!(matches!(
        resolve_entry(
            &state.lifecore,
            &state.cli,
            &unknown,
            EntryExpectation::Node,
            Some(0)
        ),
        Err(AppError::UnknownEntryIndex { .. })
    ));
    let wrong_digest =
        EntryLocator::parse(&format!("3:{}", "0".repeat(16))).expect("locator grammar is valid");
    assert!(matches!(
        resolve_entry(
            &state.lifecore,
            &state.cli,
            &wrong_digest,
            EntryExpectation::Node,
            Some(3)
        ),
        Err(AppError::EntryFingerprintMismatch { .. })
    ));
    assert!(matches!(
        resolve_entry(
            &state.lifecore,
            &state.cli,
            &EntryLocator::parse(&edge.display_locator()).expect("edge locator parses"),
            EntryExpectation::Node,
            Some(view.end_exclusive())
        ),
        Err(AppError::EntryNotNode { .. })
    ));
    assert!(matches!(
        resolve_entry(
            &state.lifecore,
            &state.cli,
            &EntryLocator::parse(&node.display_locator()).expect("node locator parses"),
            EntryExpectation::Node,
            Some(2)
        ),
        Err(AppError::EntryOutsideView { .. })
    ));
}

#[test]
fn node_references_are_lexically_disjoint_and_head_skips_the_latest_edge() {
    let mut state = stage3_state();
    let view = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let expected = resolve_index(
        &state.lifecore,
        &state.cli,
        2,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )
    .expect("latest node resolves");
    state.cli.aliases.insert(
        "chosen".to_string(),
        StoredEntryLocatorV1 {
            version: 1,
            index: 2,
            digest: expected.fingerprint.as_str().to_string(),
        },
    );

    let head = resolve_node_reference(&state.lifecore, &state.cli, &NodeReference::Head, &view)
        .expect("HEAD resolves");
    let alias = resolve_node_reference(
        &state.lifecore,
        &state.cli,
        &NodeReference::parse("chosen").expect("alias parses"),
        &view,
    )
    .expect("alias resolves");
    assert_eq!(head.index(), 2);
    assert_eq!(alias.index(), 2);
    assert!(matches!(
        NodeReference::parse("foo:bar"),
        Err(AppError::InvalidEntryLocator { .. })
    ));
    assert!(matches!(
        NodeReference::parse("42"),
        Err(AppError::InvalidAlias { .. })
    ));
    assert_eq!(
        NodeReference::parse("head").expect("lowercase head is an alias"),
        NodeReference::Alias(crate::reference::AliasName::parse("head").expect("alias parses"))
    );

    let durable = state.cli.aliases["chosen"].clone();
    state.cli.aliases.insert("bad-name".to_string(), durable);
    assert!(matches!(
        state.validate(),
        Err(AppError::InvalidState { .. })
    ));

    let mut reserved = stage3_state();
    reserved
        .cli
        .aliases
        .insert("HEAD".to_string(), expected.durable_locator());
    assert!(matches!(
        reserved.validate(),
        Err(AppError::InvalidState { .. })
    ));
}

#[test]
fn bounded_views_change_freshness_without_changing_entry_identity() {
    let state = stage3_state();
    let latest = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let boundary = resolve_index(
        &state.lifecore,
        &state.cli,
        1,
        EntryExpectation::Any,
        Some(latest.end_exclusive()),
    )
    .expect("historical boundary resolves");
    let historical = GraphView::construct(
        &state.lifecore,
        &state.cli,
        ViewSpec::Through(
            EntryLocator::parse(&boundary.display_locator()).expect("boundary locator parses"),
        ),
    )
    .expect("historical view constructs");
    let original = resolve_index(
        &state.lifecore,
        &state.cli,
        0,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("original node resolves");
    let historical_record = entry_record(&state.lifecore, &state.cli, &historical, &original)
        .expect("historical record renders");
    let latest_record = entry_record(&state.lifecore, &state.cli, &latest, &original)
        .expect("latest record renders");

    assert!(historical_record.fresh);
    assert!(!latest_record.fresh);
    assert_eq!(historical_record.locator, latest_record.locator);
}

#[test]
fn complete_predicate_grammar_adapts_to_core_queries() {
    for predicate in [
        "fresh",
        "type Note",
        "field title exists",
        "field title kind text",
        "field title equals-text Draft note",
        "field score equals-number -1",
        "field title matches (?i)note",
        "field history sequence-contains-text draft",
        "field history sequence-contains-number 1",
        "field history sequence-count 2",
        "field tags set-contains-text review",
        "field tags set-contains-number 1",
        "field tags set-count 2",
        "field tags[*] count 2",
        "outgoing Relates_To",
        "incoming Relates_To",
        "fresh-outgoing Relates_To",
        "fresh-incoming Relates_To",
        "not (field tags[*] equals-text archived)",
    ] {
        parse_predicate(predicate).unwrap_or_else(|error| panic!("{predicate:?}: {error}"));
    }
    for invalid in [
        "field Title exists",
        "field title equals-number nope",
        "field title sequence-count -1",
        "field title matches (",
        "not fresh",
    ] {
        assert!(matches!(
            parse_predicate(invalid),
            Err(AppError::InvalidQueryPredicate { .. })
        ));
    }

    let state = stage3_state();
    let latest = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let query = parse_query(
        &latest,
        crate::cli::QueryCombinator::All,
        &[
            "fresh".to_string(),
            "type Note".to_string(),
            "field tags set-contains-text review".to_string(),
            "incoming Relates_To".to_string(),
        ],
    )
    .expect("combined query parses");
    let results = execute_query(&state.lifecore, &query);

    assert!(results.is_empty());
    assert_eq!(
        execute_query(
            &state.lifecore,
            &parse_query(&latest, crate::cli::QueryCombinator::All, &[])
                .expect("empty all query parses")
        )
        .len(),
        3
    );
    assert!(
        execute_query(
            &state.lifecore,
            &parse_query(&latest, crate::cli::QueryCombinator::Any, &[])
                .expect("empty any query parses")
        )
        .is_empty()
    );
    assert_eq!(
        execute_query(
            &state.lifecore,
            &parse_query(&latest, crate::cli::QueryCombinator::None, &[])
                .expect("empty none query parses")
        )
        .len(),
        3
    );
}

#[test]
fn stage3_cli_lists_selects_inspects_and_explains_without_writing_state() {
    let directory = tempfile::tempdir().expect("temporary Stage 3 directory");
    let state = initialize_stage3_state(directory.path());
    let path = directory.path().join("LifeCoreState.json");
    let before = fs::read(&path).expect("state bytes are readable");
    let latest = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let original = resolve_index(
        &state.lifecore,
        &state.cli,
        0,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("original node resolves");
    let revised = resolve_index(
        &state.lifecore,
        &state.cli,
        2,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("revised node resolves");

    let shown = run_in(
        &["lifecore", "entry", "show", &original.display_locator()],
        directory.path(),
        None,
    )
    .expect("entry show succeeds");
    match shown {
    CommandOutcome::Entries { items, detail, .. } => {
        assert!(detail);
        assert_eq!(items[0].index, 0);
        assert!(!items[0].fresh);
    }
    outcome => panic!("expected entry detail, got {outcome:?}"),
    }

    let listed = run_in(
        &[
            "lifecore", "query", "nodes", "all", "--where", "fresh", "--format", "json",
        ],
        directory.path(),
        None,
    )
    .expect("query listing succeeds");
    let mut rendered = Vec::new();
    render_outcome(&listed, &mut rendered, false).expect("query JSON renders");
    let value: serde_json::Value = serde_json::from_slice(&rendered).expect("query output is JSON");
    assert_eq!(
        value["items"].as_array().expect("items is an array").len(),
        2
    );
    assert_eq!(value["view"]["end"], 4);

    let selected = run_in(
        &[
            "lifecore",
            "query",
            "nodes",
            "all",
            "--where",
            "fresh",
            "--select",
            &revised.display_locator(),
            "--",
            "entry",
            "inspect",
        ],
        directory.path(),
        None,
    )
    .expect("query member selection succeeds");
    assert!(matches!(
        selected,
        CommandOutcome::Entries { detail: true, .. }
    ));

    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "query",
                "nodes",
                "all",
                "--where",
                "fresh",
                "--select",
                &original.display_locator(),
            ],
            directory.path(),
            None,
        ),
        Err(AppError::QuerySelection { .. })
    ));

    let explained = run_in(
        &[
            "lifecore",
            "query",
            "nodes",
            "all",
            "--where",
            "fresh",
            "--explain",
            &original.display_locator(),
        ],
        directory.path(),
        None,
    )
    .expect("non-member explanation succeeds");
    match explained {
    CommandOutcome::QueryExplanation { explanation, .. } => {
        assert_eq!(explanation.predicates.len(), 1);
        assert!(!explanation.predicates[0].matches);
        assert!(!explanation.matches);
    }
    outcome => panic!("expected query explanation, got {outcome:?}"),
    }

    assert_eq!(
        fs::read(path).expect("state remains readable"),
        before,
        "Stage 3 read operations must preserve exact state bytes"
    );
}

#[test]
fn stage7_aliases_are_exact_rebindable_view_aware_and_byte_safe() {
    let directory = tempfile::tempdir().expect("temporary Stage 7 alias directory");
    let state = initialize_stage7_state(directory.path());
    let path = directory.path().join("LifeCoreState.json");
    let latest = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let original = resolve_index(
        &state.lifecore,
        &state.cli,
        0,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("original node resolves");
    let boundary = resolve_index(
        &state.lifecore,
        &state.cli,
        1,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("historical boundary resolves");
    let revised = resolve_index(
        &state.lifecore,
        &state.cli,
        2,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("revised node resolves");
    let edge = resolve_index(
        &state.lifecore,
        &state.cli,
        3,
        EntryExpectation::Any,
        Some(latest.end_exclusive()),
    )
    .expect("edge resolves");
    let original_locator = original.display_locator();
    let revised_locator = revised.display_locator();
    let revised_full = revised.full_locator();
    let before = fs::read(&path).expect("initial state bytes are readable");

    assert!(matches!(
        run_in(
            &["lifecore", "alias", "set", "HEAD", &revised_locator],
            directory.path(),
            None,
        ),
        Err(AppError::Usage { .. })
    ));
    assert!(matches!(
        run_in(
            &["lifecore", "alias", "set", "chosen", "another_alias"],
            directory.path(),
            None,
        ),
        Err(AppError::Usage { .. })
    ));
    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "alias",
                "set",
                "chosen",
                &edge.display_locator()
            ],
            directory.path(),
            None,
        ),
        Err(AppError::EntryNotNode { .. })
    ));
    assert!(matches!(
        run_in(
            &["lifecore", "alias", "set", "chosen", "2:00000"],
            directory.path(),
            None,
        ),
        Err(AppError::EntryFingerprintMismatch { .. })
    ));
    assert_eq!(
        fs::read(&path).expect("failed aliases preserve state"),
        before
    );

    for (name, locator) in [
        ("zeta", revised_locator.as_str()),
        ("alpha", original_locator.as_str()),
        ("second", revised_locator.as_str()),
    ] {
        let outcome = run_in(
            &["lifecore", "alias", "set", name, locator],
            directory.path(),
            None,
        )
        .expect("alias binding succeeds");
        assert!(matches!(outcome, CommandOutcome::AliasChanged { .. }));
    }

    let listed = run_in(&["lifecore", "alias", "list"], directory.path(), None)
        .expect("latest alias listing succeeds");
    let mut table = Vec::new();
    render_outcome(&listed, &mut table, true).expect("alias table renders");
    let table = String::from_utf8(table).expect("alias table is UTF-8");
    assert!(table.contains("NAME\tLOCATOR\tSTATUS"));
    assert!(table.contains("alpha\t0:"));

    let mut lines = Vec::new();
    render_outcome(&listed, &mut lines, false).expect("alias lines render");
    let lines = String::from_utf8(lines).expect("alias lines are UTF-8");
    assert!(lines.contains(&original.full_locator()));
    assert!(lines.contains("status=stale"));

    match listed {
    CommandOutcome::Aliases { items, .. } => {
        assert_eq!(
            items
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "second", "zeta"]
        );
        assert_eq!(items[0].status, AliasStatus::Stale);
        assert_eq!(items[1].status, AliasStatus::Current);
        assert_eq!(items[2].status, AliasStatus::Current);
        assert!(
            items
                .iter()
                .all(|item| item.locator.len() > item.display_locator.len())
        );
    }
    outcome => panic!("expected alias listing, got {outcome:?}"),
    }

    let historical = run_in(
        &[
            "lifecore",
            "alias",
            "list",
            "--at",
            &boundary.display_locator(),
        ],
        directory.path(),
        None,
    )
    .expect("historical alias listing succeeds");
    match historical {
    CommandOutcome::Aliases { items, .. } => {
        assert_eq!(items[0].status, AliasStatus::Current);
        assert_eq!(items[1].status, AliasStatus::OutsideView);
        assert_eq!(items[2].status, AliasStatus::OutsideView);
    }
    outcome => panic!("expected historical alias listing, got {outcome:?}"),
    }

    let child = run_in(
        &["lifecore", "add", "Note", "--parent", "zeta"],
        directory.path(),
        None,
    )
    .expect("ordinary node reference accepts an alias");
    match child {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(
            result.entries[0].parent.as_deref(),
            Some(revised_full.as_str())
        );
    }
    outcome => panic!("expected alias-parent transaction, got {outcome:?}"),
    }

    run_in(
        &["lifecore", "alias", "set", "alpha", &revised_locator],
        directory.path(),
        None,
    )
    .expect("explicit alias rebind succeeds");
    let rebound =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"))
            .load()
            .expect("rebound state loads");
    assert_eq!(
        rebound.cli.aliases["alpha"].digest,
        revised.fingerprint.as_str()
    );

    let dot_path = directory.path().join("aliases.dot");
    run_in(
        &[
            "lifecore",
            "graphviz",
            dot_path.to_str().expect("DOT path is UTF-8"),
            "--dot",
        ],
        directory.path(),
        None,
    )
    .expect("GraphViz with aliases succeeds");
    let dot = fs::read_to_string(dot_path).expect("alias DOT is readable");
    assert!(dot.contains("aliases: alpha, second, zeta"));

    run_in(
        &["lifecore", "alias", "remove", "zeta"],
        directory.path(),
        None,
    )
    .expect("alias removal succeeds");
    let stable = fs::read(&path).expect("post-removal state bytes are readable");
    assert!(matches!(
        run_in(
            &["lifecore", "alias", "remove", "zeta"],
            directory.path(),
            None,
        ),
        Err(AppError::InvalidNodeReference { .. })
    ));
    assert_eq!(
        fs::read(&path).expect("unknown removal preserves state"),
        stable
    );
}

#[test]
fn stage7_query_actions_attest_membership_and_fix_the_selected_parent() {
    let directory = tempfile::tempdir().expect("temporary Stage 7 selection directory");
    let state = initialize_stage7_state(directory.path());
    let path = directory.path().join("LifeCoreState.json");
    let latest = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let original = resolve_index(
        &state.lifecore,
        &state.cli,
        0,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("original node resolves");
    let task = resolve_index(
        &state.lifecore,
        &state.cli,
        1,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("task node resolves");
    let revised = resolve_index(
        &state.lifecore,
        &state.cli,
        2,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("revised node resolves");
    let edge = resolve_index(
        &state.lifecore,
        &state.cli,
        3,
        EntryExpectation::Any,
        Some(latest.end_exclusive()),
    )
    .expect("head edge resolves");

    let selected_alias = run_in(
        &[
            "lifecore",
            "query",
            "nodes",
            "all",
            "--where",
            "fresh",
            "--select",
            &revised.display_locator(),
            "--",
            "alias",
            "set",
            "guarded",
        ],
        directory.path(),
        None,
    )
    .expect("query-attested alias binding succeeds");
    assert!(matches!(
        selected_alias,
        CommandOutcome::AliasChanged { .. }
    ));

    let stable = fs::read(&path).expect("aliased state bytes are readable");
    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "query",
                "nodes",
                "all",
                "--where",
                "fresh",
                "--select",
                &original.display_locator(),
                "--",
                "alias",
                "set",
                "rejected",
            ],
            directory.path(),
            None,
        ),
        Err(AppError::QuerySelection { .. })
    ));
    assert_eq!(
        fs::read(&path).expect("non-member action preserves state"),
        stable
    );

    let dry_run = run_in(
        &[
            "lifecore",
            "query",
            "nodes",
            "all",
            "--where",
            "fresh",
            "--select",
            &task.display_locator(),
            "--",
            "add-child",
            "Task",
            "--field",
            "title = \"child\"",
        ],
        directory.path(),
        None,
    )
    .expect("selected child dry run succeeds");
    match dry_run {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(result.state, TransactionState::DryRun);
        assert_eq!(
            result.entries[0].parent.as_deref(),
            Some(task.full_locator().as_str())
        );
    }
    outcome => panic!("expected selected-child dry run, got {outcome:?}"),
    }
    assert_eq!(fs::read(&path).expect("dry run preserves state"), stable);

    assert_usage_error(
        &[
            "lifecore",
            "query",
            "nodes",
            "all",
            "--select",
            &task.display_locator(),
            "--",
            "add-child",
            "Task",
            "--parent",
            &revised.display_locator(),
        ],
        "unexpected argument '--parent'",
    );

    let committed = run_in(
        &[
            "lifecore",
            "query",
            "nodes",
            "all",
            "--where",
            "fresh",
            "--select",
            &task.display_locator(),
            "--",
            "add-child",
            "Task",
            "--field",
            "title = \"child\"",
            "--commit",
            "--expect-head",
            &edge.display_locator(),
        ],
        directory.path(),
        None,
    )
    .expect("selected child commit succeeds");
    match committed {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(result.state, TransactionState::Committed);
        assert_eq!(
            result.entries[0].parent.as_deref(),
            Some(task.full_locator().as_str())
        );
    }
    outcome => panic!("expected selected-child commit, got {outcome:?}"),
    }

    let after_commit = fs::read(&path).expect("committed state bytes are readable");
    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "query",
                "nodes",
                "all",
                "--where",
                "fresh",
                "--select",
                &task.display_locator(),
                "--",
                "alias",
                "set",
                "too_late",
            ],
            directory.path(),
            None,
        ),
        Err(AppError::QuerySelection { .. })
    ));
    assert_eq!(
        fs::read(&path).expect("changed membership preserves state"),
        after_commit
    );
}

#[test]
fn stage8_type_create_list_show_and_rendering_are_deterministic() {
    let directory = tempfile::tempdir().expect("temporary Stage 8 type directory");
    let state = initialize_stage8_state(directory.path());
    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    let path = store.path();
    let view = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let bootstrap = resolve_index(
        &state.lifecore,
        &state.cli,
        0,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )
    .expect("bootstrap declaration resolves");
    let before = fs::read(path).expect("bootstrapped bytes are readable");
    let declaration_args = [
        "lifecore",
        "type",
        "create",
        "Rich",
        "--field",
        "literal = \"fixed\" : Optional",
        "--field",
        "history = Sequence(Text) : Optional",
        "--field",
        "pair = Tuple(Text, Number) : Optional",
        "--field",
        "tags = OrderedSet(Text) : Optional",
        "--field",
        "status = OneOf(\"Owner's\", \"Done\") : Optional",
        "--outgoing",
        "Blocks",
        "--incoming",
        "Blocked_By",
        "--allows-override",
        "ArchivedRich",
    ];

    let dry_run = run_in(&declaration_args, directory.path(), None)
        .expect("rich declaration dry run succeeds");
    match dry_run {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(result.state, TransactionState::DryRun);
        assert_eq!(result.entries.len(), 1);
        let fields = result.entries[0]
            .fields
            .as_ref()
            .expect("staged Type node has fields");
        let declaration = Type::try_from_body(fields).expect("staged Type body decodes");
        assert_eq!(declaration.name, "Rich");
        assert_eq!(declaration.body.len(), 5);
    }
    outcome => panic!("expected type-create transaction, got {outcome:?}"),
    }
    assert_eq!(fs::read(path).expect("dry-run state is readable"), before);

    let mut commit_args = declaration_args.to_vec();
    let bootstrap_locator = bootstrap.display_locator();
    commit_args.extend(["--commit", "--expect-head", bootstrap_locator.as_str()]);
    let committed =
        run_in(&commit_args, directory.path(), None).expect("rich declaration commit succeeds");
    assert!(matches!(
        committed,
        CommandOutcome::Transaction { result, .. }
            if result.state == TransactionState::Committed
    ));

    let loaded = store.load().expect("committed rich declaration loads");
    let latest =
        GraphView::latest(&loaded.lifecore, &loaded.cli).expect("latest rich view constructs");
    let rich = resolve_index(
        &loaded.lifecore,
        &loaded.cli,
        1,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("rich declaration resolves");

    let listed = run_in(
        &["lifecore", "type", "list", "--format", "json"],
        directory.path(),
        None,
    )
    .expect("type listing succeeds");
    match &listed {
    CommandOutcome::Types {
        items,
        detail: false,
        ..
    } => {
        assert_eq!(
            items
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Rich", "Type"]
        );
        assert_eq!(items[0].fields.len(), 5);
        assert_eq!(items[0].outgoing, ["Blocks"]);
        assert_eq!(items[0].incoming, ["Blocked_By"]);
        assert_eq!(items[0].allows_override, ["ArchivedRich"]);
    }
    outcome => panic!("expected type listing, got {outcome:?}"),
    }
    let mut json = Vec::new();
    render_outcome(&listed, &mut json, false).expect("type JSON renders");
    let json: serde_json::Value = serde_json::from_slice(&json).expect("type output is JSON");
    assert_eq!(json["items"][0]["name"], "Rich");
    assert_eq!(json["items"][0]["locator"], rich.full_locator());
    assert_eq!(json["view"]["end"], 2);

    let table = run_in(&["lifecore", "type", "list"], directory.path(), None)
        .expect("type table source succeeds");
    let mut rendered = Vec::new();
    render_outcome(&table, &mut rendered, true).expect("type table renders");
    let rendered = String::from_utf8(rendered).expect("type table is UTF-8");
    assert!(rendered.contains("NAME\tLOCATOR"));
    assert!(rendered.contains(&format!("Rich\t{}", rich.display_locator())));

    let shown = run_in(
        &[
            "lifecore",
            "type",
            "show",
            &rich.display_locator(),
            "--format",
            "lines",
        ],
        directory.path(),
        None,
    )
    .expect("type show succeeds");
    let mut rendered = Vec::new();
    render_outcome(&shown, &mut rendered, false).expect("type detail renders");
    let rendered = String::from_utf8(rendered).expect("type detail is UTF-8");
    assert!(rendered.contains(&format!("Locator: {}", rich.full_locator())));
    assert!(rendered.contains("lifecore type create 'Rich'"));
    assert!(rendered.contains("--field 'pair = Tuple(Text, Number) : Optional'"));
    assert!(rendered.contains("--field 'status = OneOf(\"Owner'\"'\"'s\", \"Done\") : Optional'"));
    assert!(!rendered.contains("--commit"));
}

#[test]
fn stage8_overrides_preserve_order_parent_history_and_query_attestation() {
    let directory = tempfile::tempdir().expect("temporary Stage 8 override directory");
    let state = initialize_stage8_state(directory.path());
    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    let bootstrap_view =
        GraphView::latest(&state.lifecore, &state.cli).expect("bootstrap view constructs");
    let bootstrap = resolve_index(
        &state.lifecore,
        &state.cli,
        0,
        EntryExpectation::Node,
        Some(bootstrap_view.end_exclusive()),
    )
    .expect("bootstrap resolves");
    run_in(
        &[
            "lifecore",
            "type",
            "create",
            "Task",
            "--field",
            "first = Text : Optional",
            "--field",
            "second = Number : Optional",
            "--outgoing",
            "Blocks",
            "--outgoing",
            "Keeps",
            "--incoming",
            "Blocked_By",
            "--incoming",
            "Keeps_In",
            "--allows-override",
            "OldTask",
            "--commit",
            "--expect-head",
            &bootstrap.display_locator(),
        ],
        directory.path(),
        None,
    )
    .expect("Task declaration commits");

    let loaded = store.load().expect("Task declaration loads");
    let view = GraphView::latest(&loaded.lifecore, &loaded.cli).expect("Task view constructs");
    let source = resolve_index(
        &loaded.lifecore,
        &loaded.cli,
        1,
        EntryExpectation::Node,
        Some(view.end_exclusive()),
    )
    .expect("source Task declaration resolves");
    run_in(
        &[
            "lifecore",
            "add",
            "Task",
            "--field",
            "first = \"present\"",
            "--commit",
            "--expect-head",
            &source.display_locator(),
        ],
        directory.path(),
        None,
    )
    .expect("existing Task node commits");

    let patched = run_in(
        &[
            "lifecore",
            "type",
            "override",
            "Task",
            "--field",
            "first = Text : Encouraged",
            "--drop-field",
            "second",
            "--field",
            "third = Number : Optional",
            "--disallow-outgoing",
            "Blocks",
            "--allow-outgoing",
            "Relates_To",
            "--disallow-incoming",
            "Blocked_By",
            "--allow-incoming",
            "Mentions",
            "--disallow-override",
            "OldTask",
            "--allows-override",
            "FocusedTask",
            "--commit",
        ],
        directory.path(),
        None,
    )
    .expect("Task override commits");
    let patched_entry = match patched {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(result.state, TransactionState::Committed);
        assert_eq!(
            result.entries[0].parent.as_deref(),
            Some(source.full_locator().as_str())
        );
        result.entries[0].clone()
    }
    outcome => panic!("expected type-override transaction, got {outcome:?}"),
    };
    let patched_type = Type::try_from_body(
        patched_entry
            .fields
            .as_ref()
            .expect("patched Type node has fields"),
    )
    .expect("patched Type body decodes");
    assert_eq!(patched_type.name, "Task");
    assert_eq!(
        patched_type
            .body
            .iter()
            .map(|field| field.name())
            .collect::<Vec<_>>(),
        vec!["first", "third"]
    );
    assert_eq!(
        patched_type
            .outgoing
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["Keeps", "Relates_To"]
    );
    assert_eq!(
        patched_type
            .incoming
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["Keeps_In", "Mentions"]
    );
    assert_eq!(
        patched_type
            .additional_override
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["FocusedTask"]
    );

    assert!(matches!(
        run_in(
            &["lifecore", "type", "show", &source.display_locator()],
            directory.path(),
            None,
        ),
        Ok(CommandOutcome::Types { detail: true, .. })
    ));
    let historical_list = run_in(
        &[
            "lifecore",
            "type",
            "list",
            "--at",
            &source.display_locator(),
        ],
        directory.path(),
        None,
    )
    .expect("historical type listing succeeds");
    assert!(matches!(
        historical_list,
        CommandOutcome::Types { items, .. }
            if items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>()
                == ["Task", "Type"]
    ));

    let stable = fs::read(store.path()).expect("patched bytes are readable");
    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "type",
                "override",
                "Task",
                "--allow-outgoing",
                "Keeps",
            ],
            directory.path(),
            None,
        ),
        Err(AppError::TypePatchNoOp { .. })
    ));
    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "type",
                "override",
                "Task",
                "--allow-outgoing",
                "Conflict",
                "--disallow-outgoing",
                "Conflict",
            ],
            directory.path(),
            None,
        ),
        Err(AppError::InvalidTypePatch { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("failed patch bytes load"),
        stable
    );

    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "type",
                "override",
                "Task",
                "--field",
                "required = Text : Mandatory",
                "--commit",
            ],
            directory.path(),
            None,
        ),
        Err(AppError::TransactionAborted { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("TypeCheck rejection bytes load"),
        stable
    );

    let historical = run_in(
        &[
            "lifecore",
            "type",
            "override",
            "Task",
            "--at",
            &source.display_locator(),
            "--as",
            "FocusedTask",
            "--field",
            "branch = OneOf(\"Focused\", \"Paused\") : Optional",
            "--commit",
        ],
        directory.path(),
        None,
    )
    .expect("historical branch commits");
    match historical {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(
            result.entries[0].parent.as_deref(),
            Some(source.full_locator().as_str())
        );
        let declaration = Type::try_from_body(
            result.entries[0]
                .fields
                .as_ref()
                .expect("historical branch has fields"),
        )
        .expect("historical branch decodes");
        assert_eq!(declaration.name, "FocusedTask");
    }
    outcome => panic!("expected historical type transaction, got {outcome:?}"),
    }

    let loaded = store.load().expect("historical branch state loads");
    let latest =
        GraphView::latest(&loaded.lifecore, &loaded.cli).expect("branch latest view constructs");
    let current_task = resolve_index(
        &loaded.lifecore,
        &loaded.cli,
        patched_entry.index,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("current Task declaration resolves");
    let ordinary_task = resolve_index(
        &loaded.lifecore,
        &loaded.cli,
        2,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("ordinary Task node resolves");

    let selected = run_in(
        &[
            "lifecore",
            "query",
            "nodes",
            "all",
            "--where",
            "fresh",
            "--where",
            "type Type",
            "--where",
            "field name equals-text Task",
            "--select",
            &current_task.display_locator(),
            "--",
            "type",
            "override",
            "--allow-incoming",
            "Guarded",
        ],
        directory.path(),
        None,
    )
    .expect("query-attested type override dry run succeeds");
    match selected {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(result.state, TransactionState::DryRun);
        assert_eq!(
            result.entries[0].parent.as_deref(),
            Some(current_task.full_locator().as_str())
        );
    }
    outcome => panic!("expected selected type transaction, got {outcome:?}"),
    }

    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "query",
                "nodes",
                "all",
                "--where",
                "fresh",
                "--select",
                &source.display_locator(),
                "--",
                "type",
                "override",
                "--as",
                "Rejected",
            ],
            directory.path(),
            None,
        ),
        Err(AppError::QuerySelection { .. })
    ));
    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "query",
                "nodes",
                "all",
                "--select",
                &source.display_locator(),
                "--",
                "type",
                "override",
                "--as",
                "Rejected",
            ],
            directory.path(),
            None,
        ),
        Err(AppError::InvalidTypeDeclaration { .. })
    ));
    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "query",
                "nodes",
                "all",
                "--select",
                &ordinary_task.display_locator(),
                "--",
                "type",
                "override",
                "--as",
                "Rejected",
            ],
            directory.path(),
            None,
        ),
        Err(AppError::InvalidTypeDeclaration { .. })
    ));
}

#[test]
fn stage8_type_failures_are_early_view_aware_and_byte_safe() {
    let empty = tempfile::tempdir().expect("temporary empty type directory");
    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "type",
                "create",
                "Broken",
                "--field",
                "not a field declaration",
            ],
            empty.path(),
            None,
        ),
        Err(AppError::InvalidTypeDeclaration { .. })
    ));
    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "type",
                "override",
                "Task",
                "--field",
                "title = Text : Optional",
                "--drop-field",
                "title",
            ],
            empty.path(),
            None,
        ),
        Err(AppError::InvalidTypePatch { .. })
    ));
    assert!(!empty.path().join("LifeCoreState.json").exists());

    let directory = tempfile::tempdir().expect("temporary malformed type directory");
    let mut state = initialize_stage8_state(directory.path());
    let malformed = state
        .lifecore
        .add_mock(
            Type::TYPE_NAME,
            Body::from([("name", Payload::Number(1))]),
            None,
        )
        .expect("malformed declaration is basically valid");
    let duplicate_a = state
        .lifecore
        .add_mock(
            Type::TYPE_NAME,
            Type::new("Duplicate", &[], &[], &[], &[]).into_body(),
            None,
        )
        .expect("first duplicate declaration is basically valid");
    let duplicate_b = state
        .lifecore
        .add_mock(
            Type::TYPE_NAME,
            Type::new(
                "Duplicate",
                &[(
                    "distinguishing",
                    (
                        lifecore::typing::ValueType::Text,
                        lifecore::typing::FieldStatus::Optional,
                    ),
                )],
                &[],
                &[],
                &[],
            )
            .into_body(),
            None,
        )
        .expect("second duplicate declaration is basically valid");
    state
        .lifecore
        .apply()
        .expect("malformed fixture suffix applies");
    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    store.save(&state).expect("malformed fixture saves");
    let latest = GraphView::latest(&state.lifecore, &state.cli).expect("fixture view constructs");
    let malformed = resolve_index(
        &state.lifecore,
        &state.cli,
        malformed.e.entry().0,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("malformed declaration resolves");
    let duplicate_a = resolve_index(
        &state.lifecore,
        &state.cli,
        duplicate_a.e.entry().0,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("first duplicate resolves");
    let duplicate_b = resolve_index(
        &state.lifecore,
        &state.cli,
        duplicate_b.e.entry().0,
        EntryExpectation::Node,
        Some(latest.end_exclusive()),
    )
    .expect("second duplicate resolves");
    let before = fs::read(store.path()).expect("malformed fixture bytes are readable");

    let listed = run_in(&["lifecore", "type", "list"], directory.path(), None)
        .expect("malformed declarations are omitted from listing");
    match listed {
    CommandOutcome::Types { items, .. } => {
        assert_eq!(
            items
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Duplicate", "Duplicate", "Type"]
        );
        assert_eq!(items[0].index, duplicate_a.index());
        assert_eq!(items[1].index, duplicate_b.index());
    }
    outcome => panic!("expected malformed-safe type listing, got {outcome:?}"),
    }
    assert!(matches!(
        run_in(
            &["lifecore", "type", "show", &malformed.display_locator()],
            directory.path(),
            None,
        ),
        Err(AppError::InvalidTypeDeclaration { .. })
    ));
    assert!(matches!(
        run_in(
            &["lifecore", "type", "override", "Duplicate", "--as", "Other",],
            directory.path(),
            None,
        ),
        Err(AppError::AmbiguousTypeDeclaration { count: 2, .. })
    ));
    assert!(matches!(
        run_in(
            &["lifecore", "type", "override", "Missing", "--as", "Other",],
            directory.path(),
            None,
        ),
        Err(AppError::TypeDeclarationNotFound { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("failed type operation bytes load"),
        before
    );
}

fn graphviz_state() -> PersistedState {
    let mut state = stage3_state();
    state
        .lifecore
        .add_mock(
            "Special|Type",
            Body::from([
                (
                    "special",
                    Payload::Text(
                        "quote \" slash \\ braces {} pipe | angle <tag> line\nnext".to_string(),
                    ),
                ),
                (
                    "nested",
                    Payload::Sequence(vec![
                        Payload::Text("left|right".to_string()),
                        Payload::Number(7),
                    ]),
                ),
            ]),
            None,
        )
        .expect("special-character node is valid");
    state
        .lifecore
        .apply()
        .expect("special-character suffix applies");
    state
}

#[test]
fn graphviz_generation_is_deterministic_view_aware_and_safely_escaped() {
    let state = graphviz_state();
    let latest = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let default_options = GraphvizOptions {
        full: false,
        fields: false,
    };
    let generate_latest = |options| {
        generate(
            GraphvizInput {
                graph: &state.lifecore,
                state: &state.cli,
                view: &latest,
                aliases: &BTreeMap::new(),
                canonical_end: latest.end_exclusive(),
            },
            options,
        )
        .expect("DOT generation succeeds")
    };
    let fresh = generate_latest(default_options);
    let repeated = generate_latest(default_options);

    assert_eq!(fresh.as_bytes(), repeated.as_bytes());
    assert!(!fresh.as_str().contains("cluster_family"));
    assert!(!fresh.as_str().contains("style=dashed"));
    assert!(!fresh.as_str().contains("  n0 ["));
    assert!(fresh.as_str().contains("  n1 ["));
    assert!(fresh.as_str().contains("  n2 ["));
    assert!(fresh.as_str().contains("  n4 ["));
    assert!(fresh.as_str().contains("n2 -> n1"));

    let full = generate_latest(GraphvizOptions {
        full: true,
        fields: true,
    });
    assert!(full.as_str().contains("subgraph cluster_family_0"));
    assert!(full.as_str().contains("n0 -> n2"));
    assert!(full.as_str().contains("style=dashed"));
    assert!(full.as_str().contains("Special\\|Type"));
    assert!(full.as_str().contains("{nested|"));
    assert!(full.as_str().contains("{special|"));
    assert!(full.as_str().contains("\\<tag\\>"));
    assert!(full.as_str().contains("left\\|right"));
    assert!(full.as_str().contains("\\\\"));
    assert!(full.as_str().contains("label=\"{{{4:"));
    assert!(!full.as_str().contains("\\\"nested\\\":"));
    for index in 0..=4 {
        let entry = resolve_index(
            &state.lifecore,
            &state.cli,
            index,
            EntryExpectation::Any,
            Some(latest.end_exclusive()),
        )
        .expect("GraphViz entry resolves");
        let short = format!("{index}:{}", &entry.fingerprint.as_str()[..5]);

        assert!(full.as_str().contains(&short));
        assert!(!full.as_str().contains(&entry.display_locator()));
    }

    let boundary = resolve_index(
        &state.lifecore,
        &state.cli,
        1,
        EntryExpectation::Any,
        Some(latest.end_exclusive()),
    )
    .expect("historical boundary resolves");
    let historical = GraphView::through(
        &state.lifecore,
        &state.cli,
        &EntryLocator::parse(&boundary.display_locator()).expect("boundary locator parses"),
    )
    .expect("historical view constructs");
    let historical = generate(
        GraphvizInput {
            graph: &state.lifecore,
            state: &state.cli,
            view: &historical,
            aliases: &BTreeMap::new(),
            canonical_end: historical.end_exclusive(),
        },
        default_options,
    )
    .expect("historical DOT generation succeeds");

    assert!(historical.as_str().contains("  n0 ["));
    assert!(historical.as_str().contains("  n1 ["));
    assert!(!historical.as_str().contains("  n2 ["));
    assert!(!historical.as_str().contains("n2 -> n1"));
}

#[test]
fn graphviz_cli_writes_dot_and_preserves_canonical_state_on_every_path() {
    let directory = tempfile::tempdir().expect("temporary GraphViz directory");
    let state = initialize_stage3_state(directory.path());
    let state_path = directory.path().join("LifeCoreState.json");
    let before = fs::read(&state_path).expect("state bytes are readable");
    let latest = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let boundary = resolve_index(
        &state.lifecore,
        &state.cli,
        1,
        EntryExpectation::Any,
        Some(latest.end_exclusive()),
    )
    .expect("historical boundary resolves");

    let outcome = run_in(
        &[
            "lifecore",
            "graphviz",
            "latest.dot",
            "--dot",
            "--full",
            "--fields",
        ],
        directory.path(),
        None,
    )
    .expect("DOT export succeeds");
    match outcome {
    CommandOutcome::GraphvizArtifact {
        target,
        artifact_kind,
        view,
    } => {
        assert_eq!(target, directory.path().join("latest.dot"));
        assert_eq!(artifact_kind, "DOT");
        assert_eq!(view.end, 4);
    }
    outcome => panic!("expected GraphViz artifact, got {outcome:?}"),
    }
    let dot =
        fs::read_to_string(directory.path().join("latest.dot")).expect("DOT artifact is readable");
    assert!(dot.starts_with("digraph lifecore {"));

    run_in(
        &[
            "lifecore",
            "graphviz",
            "historical.dot",
            "--dot",
            "--at",
            &boundary.display_locator(),
        ],
        directory.path(),
        None,
    )
    .expect("historical DOT export succeeds");
    let historical = fs::read_to_string(directory.path().join("historical.dot"))
        .expect("historical DOT artifact is readable");
    assert!(historical.contains("  n0 ["));
    assert!(!historical.contains("  n2 ["));

    let blocked_target = directory.path().join("blocked");
    fs::create_dir(&blocked_target).expect("blocked target directory is created");
    assert!(matches!(
        run_in(
            &["lifecore", "graphviz", "blocked", "--dot"],
            directory.path(),
            None,
        ),
        Err(AppError::GraphvizTargetIo { .. })
    ));
    assert_eq!(
        fs::read(state_path).expect("state remains readable"),
        before,
        "GraphViz success and failure must preserve exact state bytes"
    );
}

#[test]
fn stage5_bootstrap_suggestion_dry_run_commit_and_orphan_restore_are_atomic() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    let path = directory.path().join("LifeCoreState.json");
    let initialized = run_in(
        &["lifecore", "init", "--format", "json"],
        directory.path(),
        None,
    )
    .expect("initialization succeeds");
    let suggestion = match initialized {
    CommandOutcome::Initialized { messages, .. } => {
        assert_eq!(messages.len(), 1);
        messages[0].clone()
    }
    outcome => panic!("expected initialization outcome, got {outcome:?}"),
    };
    let before = fs::read(&path).expect("initialized bytes are readable");

    let dry_run = run_in(
        &[
            "lifecore",
            "message",
            "apply",
            &suggestion.display_id,
            "--format",
            "json",
        ],
        directory.path(),
        None,
    )
    .expect("bootstrap dry run succeeds");
    let mut rendered = Vec::new();
    render_outcome(&dry_run, &mut rendered, false).expect("dry-run JSON renders");
    let rendered: serde_json::Value =
        serde_json::from_slice(&rendered).expect("dry-run output is JSON");
    assert_eq!(rendered["transaction"]["state"], "dry_run");
    match dry_run {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(result.state, TransactionState::DryRun);
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].circumstance, CircumstanceRecord::Mock);
    }
    outcome => panic!("expected transaction outcome, got {outcome:?}"),
    }
    assert_eq!(fs::read(&path).expect("state is readable"), before);

    let mismatch = run_in(
        &[
            "lifecore",
            "message",
            "apply",
            &suggestion.display_id,
            "--commit",
            "--expect-head",
            "0:abcde",
            "--format",
            "json",
        ],
        directory.path(),
        None,
    )
    .expect_err("mismatched expectation must abort");
    match &mismatch {
    AppError::TransactionAborted { message, .. } => {
        assert!(message.contains("expected canonical head"));
    }
    result => panic!("expected transaction abort, got {result:?}"),
    }
    let mut rendered = Vec::new();
    render_error(&mismatch, &mut rendered).expect("aborted JSON renders");
    let rendered: serde_json::Value =
        serde_json::from_slice(&rendered).expect("aborted output is JSON");
    assert_eq!(rendered["error"]["transaction"]["state"], "aborted");
    assert_eq!(fs::read(&path).expect("state is readable"), before);

    let committed = run_in(
        &[
            "lifecore",
            "message",
            "apply",
            &suggestion.display_id,
            "--commit",
            "--expect-head",
            "EMPTY",
        ],
        directory.path(),
        None,
    )
    .expect("guarded bootstrap commit succeeds");
    let mut rendered = Vec::new();
    render_outcome(&committed, &mut rendered, false).expect("commit output renders");
    assert!(
        String::from_utf8(rendered)
            .expect("commit output is UTF-8")
            .contains("Transaction: committed")
    );
    match committed {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(result.state, TransactionState::Committed);
        assert_eq!(result.entries.len(), 1);
        assert!(!matches!(
            result.entries[0].circumstance,
            CircumstanceRecord::Mock
        ));
    }
    outcome => panic!("expected committed transaction, got {outcome:?}"),
    }

    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    let committed = store.load().expect("committed state loads");
    assert!(!committed.lifecore.is_mock());
    assert_eq!(committed.cli.dismissals.len(), 1);
    assert_eq!(committed.cli.dismissals[0].digest, suggestion.id);

    let dismissal_ids = run_in(
        &["lifecore", "check", "--dismissal-ids"],
        directory.path(),
        None,
    )
    .expect("dismissal registry listing succeeds");
    match dismissal_ids {
    CommandOutcome::Dismissals { items, .. } => {
        assert_eq!(items.len(), 1);
        assert!(!items[0].emitted);
    }
    outcome => panic!("expected dismissal listing, got {outcome:?}"),
    }

    let full_id = format!("m:{}", suggestion.id);
    run_in(
        &["lifecore", "message", "restore", &full_id],
        directory.path(),
        None,
    )
    .expect("orphaned full dismissal ID restores");
    assert!(
        store
            .load()
            .expect("restored state loads")
            .cli
            .dismissals
            .is_empty()
    );
}

#[test]
fn stage5_dismissal_filters_are_independent_and_keep_message_skips_registration() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    let initialized =
        run_in(&["lifecore", "init"], directory.path(), None).expect("initialization succeeds");
    let suggestion = match initialized {
    CommandOutcome::Initialized { messages, .. } => messages[0].clone(),
    outcome => panic!("expected initialization outcome, got {outcome:?}"),
    };

    for _ in 0..2 {
        run_in(
            &["lifecore", "message", "dismiss", &suggestion.display_id],
            directory.path(),
            None,
        )
        .expect("dismissal is idempotent");
    }

    let visible = run_in(&["lifecore", "check"], directory.path(), None)
        .expect("default message listing succeeds");
    assert!(matches!(
        visible,
        CommandOutcome::Messages { items, .. } if items.is_empty()
    ));
    let dismissed = run_in(
        &["lifecore", "check", "--only-dismissed"],
        directory.path(),
        None,
    )
    .expect("dismissed listing succeeds");
    assert!(matches!(
        &dismissed,
        CommandOutcome::Messages { items, .. }
            if items.len() == 1 && items[0].dismissed
    ));
    let mut rendered = Vec::new();
    render_outcome(&dismissed, &mut rendered, false).expect("dismissed message renders");
    assert!(
        String::from_utf8(rendered)
            .expect("dismissed output is UTF-8")
            .contains("state=dismissed")
    );

    run_in(
        &["lifecore", "message", "restore", &suggestion.display_id],
        directory.path(),
        None,
    )
    .expect("emitted dismissal restores");
    run_in(
        &[
            "lifecore",
            "message",
            "apply",
            &suggestion.display_id,
            "--commit",
            "--expect-head",
            "EMPTY",
            "--keep-message",
        ],
        directory.path(),
        None,
    )
    .expect("keep-message commit succeeds");

    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    assert!(
        store
            .load()
            .expect("committed state loads")
            .cli
            .dismissals
            .is_empty()
    );
}

#[test]
fn stage5_inconsistencies_block_transactions_and_cannot_be_dismissed() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    let initialized =
        run_in(&["lifecore", "init"], directory.path(), None).expect("initialization succeeds");
    let suggestion = match initialized {
    CommandOutcome::Initialized { messages, .. } => messages[0].clone(),
    outcome => panic!("expected initialization outcome, got {outcome:?}"),
    };
    run_in(
        &[
            "lifecore",
            "message",
            "apply",
            &suggestion.display_id,
            "--commit",
            "--expect-head",
            "EMPTY",
        ],
        directory.path(),
        None,
    )
    .expect("bootstrap commit succeeds");

    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    let loaded = store.load().expect("bootstrapped state loads");
    let before = fs::read(store.path()).expect("bootstrapped bytes are readable");
    let registry = AnalyzerRegistry::built_in();
    let service = TransactionService::new(&store, &registry);
    let change = PlannedChange::AddNode {
        type_name: "No_Such_Type".to_string(),
        body: Body::default(),
        parent: None,
    };
    let aborted = service.execute(
        &loaded,
        TransactionRequest {
            change: change.clone(),
            commit: true,
            expected_head: None,
            dismiss_after_commit: None,
        },
    );
    match aborted {
    Err(AppError::TransactionAborted {
        detail, message, ..
    }) => {
        assert!(message.contains("Inconsistency"));
        assert!(
            detail
                .messages
                .iter()
                .any(|message| message.severity == Severity::Inconsistency)
        );
    }
    result => panic!("inconsistent candidate must abort, got {result:?}"),
    }
    assert_eq!(
        fs::read(store.path()).expect("state remains readable"),
        before
    );

    //A Persisting an intentionally inconsistent but basically valid fixture
    //A lets the command boundary prove that presentation never turns an
    //A Inconsistency into dismissible CLI state.
    let mut inconsistent = loaded;
    inconsistent
        .lifecore
        .add_mock("No_Such_Type", Body::default(), None)
        .expect("fixture node is basically valid");
    inconsistent
        .lifecore
        .apply()
        .expect("fixture suffix applies");
    store
        .save(&inconsistent)
        .expect("inconsistent fixture persists");
    let checked = run_in(&["lifecore", "check"], directory.path(), None).expect("check succeeds");
    let inconsistency = match checked {
    CommandOutcome::Messages { items, .. } => items
        .into_iter()
        .find(|message| message.severity == Severity::Inconsistency)
        .expect("inconsistency is visible"),
    outcome => panic!("expected message listing, got {outcome:?}"),
    };
    let before_dismiss = fs::read(store.path()).expect("fixture bytes are readable");
    assert!(matches!(
        run_in(
            &["lifecore", "message", "dismiss", &inconsistency.display_id,],
            directory.path(),
            None,
        ),
        Err(AppError::WrongMessageSeverity { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("fixture remains readable"),
        before_dismiss
    );
}

#[test]
fn stage5_message_filter_keeps_inconsistencies_and_separates_resolution() {
    let mut graph = LifeCoreList::default();
    let parent = graph
        .add_mock("Parent", Body::default(), None)
        .expect("parent appends");
    graph
        .add_mock("Child", Body::default(), Some(parent.e))
        .expect("child appends");
    let report = AnalysisReport {
        suite: Default::default(),
        messages: vec![
            Message::Inconsistency(Inconsistency {
                offending_event: DiagnosticLocation::Context(EntryIndex(2).before()),
                system_name: "Test".to_string(),
                system_config: String::new(),
                message: "broken".to_string(),
            }),
            Message::Warning(Warning {
                offending_event: DiagnosticLocation::Entry(
                    EntryIndex(1).before_and_including().look_at(parent.e),
                ),
                system_name: "Test".to_string(),
                system_config: String::new(),
                relevance: Vec::new(),
                message: "historical".to_string(),
            }),
            Message::Suggestion(Suggestion {
                offending_event: DiagnosticLocation::Context(EntryIndex(2).before()),
                system_name: "Test".to_string(),
                system_config: String::new(),
                relevance: Vec::new(),
                message: "active".to_string(),
                action: Vec::new(),
            }),
        ],
    };
    let first = MessageUniverse::from_analysis(&report, &graph, &[]).expect("universe constructs");
    let warning = first
        .records()
        .iter()
        .find(|message| message.severity == Severity::Warning)
        .expect("warning exists");
    assert!(warning.resolved);
    let dismissal = crate::state::DismissalRecord {
        version: 1,
        digest: warning.id.clone(),
    };
    let universe = MessageUniverse::from_analysis(&report, &graph, &[dismissal])
        .expect("dismissed universe constructs");

    assert_eq!(universe.visible().len(), 2);
    let only_dismissed = universe.filtered(MessageFilter {
        only_dismissed: true,
        include_resolved: true,
        ..MessageFilter::default()
    });
    assert_eq!(only_dismissed.len(), 2);
    assert!(
        only_dismissed
            .iter()
            .any(|message| message.severity == Severity::Inconsistency)
    );
    assert!(
        only_dismissed
            .iter()
            .any(|message| message.severity == Severity::Warning)
    );
}

#[test]
fn stage5_transaction_composites_preserve_order_and_roll_back_every_failure() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    let initial = PersistedState::new(LifeCoreList::default());
    store.initialize(&initial).expect("empty state initializes");
    let before = fs::read(store.path()).expect("initial bytes are readable");
    let registry = AnalyzerRegistry::empty();
    let service = TransactionService::new(&store, &registry);
    let composite = PlannedChange::Composite(vec![
        PlannedChange::AddNode {
            type_name: "A".to_string(),
            body: Body::default(),
            parent: None,
        },
        PlannedChange::AddNode {
            type_name: "B".to_string(),
            body: Body::default(),
            parent: None,
        },
        PlannedChange::AddEdge {
            from: EntryIndex(0),
            type_name: "Link".to_string(),
            to: EntryIndex(1),
        },
    ]);

    let dry_run = service
        .execute(
            &initial,
            TransactionRequest {
                change: composite.clone(),
                commit: false,
                expected_head: None,
                dismiss_after_commit: None,
            },
        )
        .expect("composite dry run succeeds");
    assert_eq!(dry_run.state, TransactionState::DryRun);
    assert_eq!(
        dry_run
            .entries
            .iter()
            .map(|entry| entry.kind.as_str())
            .collect::<Vec<_>>(),
        vec!["node", "node", "edge"]
    );
    assert_eq!(fs::read(store.path()).expect("state is readable"), before);

    let committed = service
        .execute(
            &initial,
            TransactionRequest {
                change: composite,
                commit: true,
                expected_head: Some(ExpectedHead::Empty),
                dismiss_after_commit: None,
            },
        )
        .expect("composite commit succeeds");
    assert_eq!(committed.state, TransactionState::Committed);
    let loaded = store.load().expect("committed state loads");
    assert!(!loaded.lifecore.is_mock());
    let committed_bytes = fs::read(store.path()).expect("committed bytes are readable");
    let edge_head = resolve_index(
        &loaded.lifecore,
        &loaded.cli,
        2,
        EntryExpectation::Any,
        None,
    )
    .expect("edge head resolves");
    verify_expected_head(
        &store,
        &ExpectedHead::Entry(
            EntryLocator::parse(&edge_head.full_locator()).expect("edge locator parses"),
        ),
    )
    .expect("latest edge is a valid expected head");

    let duplicate_edges = PlannedChange::Composite(vec![
        PlannedChange::AddEdge {
            from: EntryIndex(0),
            type_name: "Duplicate".to_string(),
            to: EntryIndex(1),
        },
        PlannedChange::AddEdge {
            from: EntryIndex(0),
            type_name: "Duplicate".to_string(),
            to: EntryIndex(1),
        },
    ]);
    assert!(matches!(
        service.execute(
            &loaded,
            TransactionRequest {
                change: duplicate_edges,
                commit: true,
                expected_head: None,
                dismiss_after_commit: None,
            }
        ),
        Err(AppError::TransactionAborted { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("state remains readable"),
        committed_bytes
    );

    assert!(matches!(
        service.execute(
            &loaded,
            TransactionRequest {
                change: PlannedChange::AddAnalyzer {
                    name: "Missing".to_string(),
                    config: String::new(),
                },
                commit: true,
                expected_head: None,
                dismiss_after_commit: None,
            }
        ),
        Err(AppError::TransactionAborted { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("state remains readable"),
        committed_bytes
    );

    let failing_apply_service = TransactionService::failing_apply_for_test(&store, &registry);
    assert!(matches!(
        failing_apply_service.execute(
            &loaded,
            TransactionRequest {
                change: PlannedChange::AddNode {
                    type_name: "Unapplied".to_string(),
                    body: Body::default(),
                    parent: None,
                },
                commit: true,
                expected_head: None,
                dismiss_after_commit: None,
            }
        ),
        Err(AppError::TransactionAborted { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("state remains readable"),
        committed_bytes
    );

    let failing_store = store.failing_save_for_test();
    let failing_service = TransactionService::new(&failing_store, &registry);
    assert!(matches!(
        failing_service.execute(
            &loaded,
            TransactionRequest {
                change: PlannedChange::AddNode {
                    type_name: "Unsaved".to_string(),
                    body: Body::default(),
                    parent: None,
                },
                commit: true,
                expected_head: None,
                dismiss_after_commit: None,
            }
        ),
        Err(AppError::TransactionAborted { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("state remains readable"),
        committed_bytes
    );
}

#[cfg(unix)]
#[test]
fn graphviz_svg_export_streams_input_and_reports_process_failures() {
    use std::os::unix::fs::PermissionsExt;

    fn executable_script(directory: &std::path::Path, name: &str, body: &str) -> PathBuf {
        let path = directory.join(name);
        fs::write(&path, body).expect("test executable is written");
        let mut permissions = fs::metadata(&path)
            .expect("test executable metadata is readable")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).expect("test executable is made executable");
        path
    }

    let directory = tempfile::tempdir().expect("temporary exporter directory");
    let state = stage3_state();
    let view = GraphView::latest(&state.lifecore, &state.cli).expect("latest view constructs");
    let document = generate(
        GraphvizInput {
            graph: &state.lifecore,
            state: &state.cli,
            view: &view,
            aliases: &BTreeMap::new(),
            canonical_end: view.end_exclusive(),
        },
        GraphvizOptions {
            full: true,
            fields: true,
        },
    )
    .expect("DOT generation succeeds");
    let success_program = executable_script(directory.path(), "fake-dot", "#!/bin/sh\ncat\n");
    let target = GraphvizTarget::resolve(
        PathBuf::from("graph.svg"),
        directory.path(),
        GraphvizKind::Svg,
    )
    .expect("SVG target resolves");

    GraphvizExporter::new(success_program)
        .write_svg(&target, &document)
        .expect("streamed SVG conversion succeeds");
    assert_eq!(
        fs::read(target.path()).expect("streamed artifact is readable"),
        document.as_bytes()
    );

    let failing_program = executable_script(
        directory.path(),
        "failing-dot",
        "#!/bin/sh\necho conversion-failed >&2\nexit 7\n",
    );
    fs::write(target.path(), b"previous artifact").expect("existing artifact is written");
    match GraphvizExporter::new(failing_program).write_svg(&target, &document) {
    Err(AppError::GraphvizProcessFailed { stderr, .. }) => {
        assert!(stderr.contains("conversion-failed"));
    }
    result => panic!("expected failed GraphViz process, got {result:?}"),
    }
    assert_eq!(
        fs::read(target.path()).expect("prior artifact remains readable"),
        b"previous artifact"
    );

    let missing = directory.path().join("missing-dot");
    let missing_exporter = GraphvizExporter::new(&missing);
    assert!(matches!(
        missing_exporter.write_svg(&target, &document),
        Err(AppError::GraphvizProcessSpawn { .. })
    ));
    assert_eq!(
        fs::read(target.path()).expect("prior artifact remains readable"),
        b"previous artifact"
    );

    let dot_target = GraphvizTarget::resolve(
        PathBuf::from("direct.dot"),
        directory.path(),
        GraphvizKind::Dot,
    )
    .expect("DOT target resolves");
    missing_exporter
        .write_dot(&dot_target, &document)
        .expect("DOT export never invokes the configured executable");
    assert_eq!(
        fs::read(dot_target.path()).expect("direct DOT artifact is readable"),
        document.as_bytes()
    );
}

#[test]
fn stage6_payload_input_converts_recursive_values_and_rejects_every_unsupported_shape() {
    let fields = vec![
        "title = \"Errands\"".to_string(),
        "score = -3".to_string(),
        "history = { sequence = [\"draft\", 2, { sequence = [\"done\"] }] }".to_string(),
        "tags = { ordered_set = [\"home\", \"urgent\"] }".to_string(),
    ];
    let body = parse_field_assignments(&fields).expect("recursive fields parse");

    assert_eq!(body.get("title"), Some(&Payload::Text("Errands".into())));
    assert_eq!(body.get("score"), Some(&Payload::Number(-3)));
    assert_eq!(
        body.get("history"),
        Some(&Payload::Sequence(vec![
            Payload::Text("draft".into()),
            Payload::Number(2),
            Payload::Sequence(vec![Payload::Text("done".into())]),
        ]))
    );
    assert_eq!(
        body.get("tags"),
        Some(&Payload::OrderedSet(OrderedSet::from([
            Payload::Text("home".into()),
            Payload::Text("urgent".into()),
        ])))
    );

    for (assignment, expected) in [
        ("value = [1, 2]", "bare TOML arrays"),
        ("value = true", "booleans"),
        ("value = 1.5", "floats"),
        ("value = 1979-05-27T07:32:00Z", "date-time"),
        ("value = { tuple = [1, 2] }", "unknown payload constructor"),
        (
            "value = { sequence = [1], ordered_set = [2] }",
            "exactly one key",
        ),
        ("value = { sequence = 1 }", "requires an array"),
        (
            "value = { ordered_set = [\"same\", \"same\"] }",
            "duplicate element",
        ),
    ] {
        match parse_field_assignment(assignment) {
        Err(AppError::InvalidPayload { message, .. }) => {
            assert!(
                message.contains(expected),
                "expected {expected:?} in {message:?}"
            );
        }
        result => panic!("expected invalid payload for {assignment:?}, got {result:?}"),
        }
    }

    assert!(matches!(
        parse_field_assignment("first = 1\nsecond = 2"),
        Err(AppError::InvalidFieldAssignment { .. })
    ));
    assert!(matches!(
        parse_field_assignments(&["same = 1".into(), "same = 2".into()]),
        Err(AppError::InvalidFieldAssignment { .. })
    ));
}

#[test]
fn stage6_add_connect_head_and_edge_expectations_use_atomic_transactions() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    store
        .initialize(&PersistedState::new(LifeCoreList::default()))
        .expect("empty canonical state initializes");
    let path = directory.path().join("LifeCoreState.json");

    let before = fs::read(&path).expect("empty state bytes are readable");
    let dry_run = run_in(
        &[
            "lifecore",
            "add",
            "Note",
            "--field",
            "meta = { sequence = [\"draft\", { ordered_set = [1, 2] }] }",
        ],
        directory.path(),
        None,
    )
    .expect("nested node dry run succeeds");
    match dry_run {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(result.state, TransactionState::DryRun);
        assert_eq!(result.entries.len(), 1);
        assert!(result.analyzers.is_empty());
        assert_eq!(result.entries[0].index, 0);
        assert_eq!(result.entries[0].kind, "node");
        assert!(result.entries[0].locator.starts_with("0:"));
    }
    outcome => panic!("expected transaction outcome, got {outcome:?}"),
    }
    assert_eq!(fs::read(&path).expect("state remains readable"), before);

    let first = run_in(
        &[
            "lifecore",
            "add",
            "Note",
            "--field",
            "title = \"first\"",
            "--commit",
            "--expect-head",
            "EMPTY",
        ],
        directory.path(),
        None,
    )
    .expect("first node commits");
    let first_locator = match first {
    CommandOutcome::Transaction { result, .. } => result.entries[0].locator.clone(),
    outcome => panic!("expected transaction outcome, got {outcome:?}"),
    };

    let second = run_in(
        &[
            "lifecore",
            "add",
            "Note",
            "--field",
            "title = \"second\"",
            "--commit",
            "--expect-head",
            &first_locator,
        ],
        directory.path(),
        None,
    )
    .expect("second node commits");
    let second_locator = match second {
    CommandOutcome::Transaction { result, .. } => result.entries[0].locator.clone(),
    outcome => panic!("expected transaction outcome, got {outcome:?}"),
    };

    let edge = run_in(
        &[
            "lifecore",
            "connect",
            "--from",
            &first_locator,
            "--type",
            "Related",
            "--to",
            &second_locator,
            "--commit",
            "--expect-head",
            &second_locator,
        ],
        directory.path(),
        None,
    )
    .expect("edge commits");
    let edge_locator = match edge {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].kind, "edge");
        assert_eq!(
            result.entries[0].from.as_deref(),
            Some(first_locator.as_str())
        );
        assert_eq!(
            result.entries[0].to.as_deref(),
            Some(second_locator.as_str())
        );
        result.entries[0].locator.clone()
    }
    outcome => panic!("expected transaction outcome, got {outcome:?}"),
    };

    let child = run_in(
        &[
            "lifecore",
            "add",
            "Note",
            "--parent",
            "HEAD",
            "--commit",
            "--expect-head",
            &edge_locator,
        ],
        directory.path(),
        None,
    )
    .expect("HEAD skips the edge while the edge remains the expected canonical head");
    match child {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(
            result.entries[0].parent.as_deref(),
            Some(second_locator.as_str())
        );
        assert_eq!(result.entries[0].index, 3);
    }
    outcome => panic!("expected transaction outcome, got {outcome:?}"),
    }

    let stable = fs::read(&path).expect("committed bytes are readable");
    assert!(matches!(
        run_in(
            &[
                "lifecore", "add", "Note", "--field", "same = 1", "--field", "same = 2",
                "--commit",
            ],
            directory.path(),
            None,
        ),
        Err(AppError::InvalidFieldAssignment { .. })
    ));
    assert!(matches!(
        run_in(
            &[
                "lifecore",
                "connect",
                "--from",
                &edge_locator,
                "--type",
                "Invalid",
                "--to",
                &first_locator,
                "--commit",
            ],
            directory.path(),
            None,
        ),
        Err(AppError::EntryNotNode { .. })
    ));
    assert_eq!(fs::read(&path).expect("state remains readable"), stable);
}

#[test]
fn stage6_analyzer_list_and_add_preserve_order_and_reject_missing_factories() {
    let directory = tempfile::tempdir().expect("temporary state directory");
    run_in(&["lifecore", "init"], directory.path(), None).expect("initialization succeeds");
    let path = directory.path().join("LifeCoreState.json");

    let listed = run_in(
        &["lifecore", "analyzer", "list", "--format", "json"],
        directory.path(),
        None,
    )
    .expect("analyzers list");
    let mut rendered = Vec::new();
    render_outcome(&listed, &mut rendered, false).expect("analyzer listing renders");
    let rendered: serde_json::Value =
        serde_json::from_slice(&rendered).expect("analyzer listing is JSON");
    assert_eq!(rendered["items"][0]["name"], "Type_Check");
    assert_eq!(rendered["items"][1]["name"], "Interactive");
    match listed {
    CommandOutcome::Analyzers { items, .. } => {
        assert_eq!(
            items
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Type_Check", "Interactive"]
        );
    }
    outcome => panic!("expected analyzer listing, got {outcome:?}"),
    }

    let before = fs::read(&path).expect("initialized bytes are readable");
    let dry_run = run_in(
        &[
            "lifecore",
            "analyzer",
            "add",
            "Interactive",
            "review-config",
            "--format",
            "json",
        ],
        directory.path(),
        None,
    )
    .expect("registered analyzer dry run succeeds");
    let mut rendered = Vec::new();
    render_outcome(&dry_run, &mut rendered, false).expect("analyzer transaction renders");
    let rendered: serde_json::Value =
        serde_json::from_slice(&rendered).expect("analyzer transaction is JSON");
    assert_eq!(
        rendered["transaction"]["analyzers"][0]["name"],
        "Interactive"
    );
    assert!(
        rendered["transaction"]["entries"]
            .as_array()
            .expect("entries is an array")
            .is_empty()
    );
    match dry_run {
    CommandOutcome::Transaction { result, .. } => {
        assert_eq!(result.state, TransactionState::DryRun);
        assert!(result.entries.is_empty());
        assert_eq!(result.analyzers.len(), 1);
        assert_eq!(result.analyzers[0].name, "Interactive");
        assert_eq!(result.analyzers[0].config, "review-config");
    }
    outcome => panic!("expected transaction outcome, got {outcome:?}"),
    }
    assert_eq!(fs::read(&path).expect("state remains readable"), before);

    run_in(
        &[
            "lifecore",
            "analyzer",
            "add",
            "Interactive",
            "review-config",
            "--commit",
            "--expect-head",
            "EMPTY",
        ],
        directory.path(),
        None,
    )
    .expect("registered analyzer commits");
    let listed = run_in(
        &["lifecore", "analyzer", "list", "--format", "json"],
        directory.path(),
        None,
    )
    .expect("updated analyzers list");
    match listed {
    CommandOutcome::Analyzers { items, .. } => {
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].name, "Interactive");
        assert_eq!(items[2].config, "review-config");
    }
    outcome => panic!("expected analyzer listing, got {outcome:?}"),
    }

    let before_unknown = fs::read(&path).expect("committed bytes are readable");
    match run_in(
        &[
            "lifecore",
            "analyzer",
            "add",
            "MissingFactory",
            "opaque",
            "--commit",
            "--expect-head",
            "EMPTY",
        ],
        directory.path(),
        None,
    ) {
    Err(AppError::TransactionAborted { detail, .. }) => {
        assert_eq!(detail.analyzers.len(), 1);
        assert_eq!(detail.analyzers[0].name, "MissingFactory");
    }
    result => panic!("expected missing factory transaction abort, got {result:?}"),
    }
    assert_eq!(
        fs::read(&path).expect("state remains readable"),
        before_unknown
    );
}

#[test]
fn stage6_interactive_analyzer_preserves_direction_and_suppresses_ineligible_connections() {
    let suggestions_at = |graph: &LifeCoreList, index: usize| {
        run_analysis(graph, &AnalyzerRegistry::built_in())
            .expect("Interactive analysis succeeds")
            .messages
            .into_iter()
            .filter_map(|message| match message {
            Message::Suggestion(suggestion)
                if matches!(
                    suggestion.offending_event,
                    DiagnosticLocation::Entry(entry) if entry.entry().0 == index
                ) =>
            {
                Some(suggestion)
            }
            _ => None,
            })
            .collect::<Vec<_>>()
    };

    let mut graph = LifeCoreList::default();
    graph.add_analyzer("Interactive", "configured");
    let parent = graph
        .add_mock("Parent", Body::default(), None)
        .expect("parent appends");
    let outgoing_other = graph
        .add_mock("OutgoingEndpoint", Body::default(), None)
        .expect("outgoing endpoint appends");
    let incoming_other = graph
        .add_mock("IncomingEndpoint", Body::default(), None)
        .expect("incoming endpoint appends");
    let outgoing = graph
        .connect_mock(parent.e, "Outgoing", outgoing_other.e)
        .expect("outgoing edge appends");
    let incoming = graph
        .connect_mock(incoming_other.e, "Incoming", parent.e)
        .expect("incoming edge appends");
    let child = graph
        .add_mock("Child", Body::default(), Some(parent.e))
        .expect("child appends");
    let suggestions = suggestions_at(&graph, child.e.entry().0);

    assert_eq!(suggestions.len(), 2);
    assert!(suggestions.iter().all(|suggestion| {
        suggestion.system_name == "Interactive"
            && suggestion.system_config == "configured"
            && suggestion.relevance.contains(&parent.e.entry())
            && suggestion.relevance.contains(&child.e.entry())
    }));
    assert!(suggestions.iter().any(|suggestion| {
        suggestion.relevance.contains(&outgoing.e.entry())
            && suggestion.action
                == vec![SuggestedAction::AddEdge {
                    from: child.e.entry(),
                    ty_name: "Outgoing".into(),
                    to: outgoing_other.e.entry(),
                }]
    }));
    assert!(suggestions.iter().any(|suggestion| {
        suggestion.relevance.contains(&incoming.e.entry())
            && suggestion.action
                == vec![SuggestedAction::AddEdge {
                    from: incoming_other.e.entry(),
                    ty_name: "Incoming".into(),
                    to: child.e.entry(),
                }]
    }));

    let mut stale_graph = LifeCoreList::default();
    stale_graph.add_analyzer("Interactive", "");
    let stale_parent = stale_graph
        .add_mock("Parent", Body::default(), None)
        .expect("parent appends");
    let stale_other = stale_graph
        .add_mock("Opposite", Body::default(), None)
        .expect("opposite appends");
    stale_graph
        .connect_mock(stale_parent.e, "Related", stale_other.e)
        .expect("edge appends");
    stale_graph
        .add_mock("OppositeChild", Body::default(), Some(stale_other.e))
        .expect("opposite successor appends");
    let stale_child = stale_graph
        .add_mock("ParentChild", Body::default(), Some(stale_parent.e))
        .expect("child appends");
    assert!(suggestions_at(&stale_graph, stale_child.e.entry().0).is_empty());

    let mut recreated_graph = LifeCoreList::default();
    recreated_graph.add_analyzer("Interactive", "");
    let recreated_parent = recreated_graph
        .add_mock("Parent", Body::default(), None)
        .expect("parent appends");
    let recreated_other = recreated_graph
        .add_mock("Opposite", Body::default(), None)
        .expect("opposite appends");
    recreated_graph
        .connect_mock(recreated_parent.e, "Related", recreated_other.e)
        .expect("parent edge appends");
    let first_successor = recreated_graph
        .add_mock("FirstChild", Body::default(), Some(recreated_parent.e))
        .expect("first successor appends");
    recreated_graph
        .connect_mock(first_successor.e, "Related", recreated_other.e)
        .expect("successor edge appends");
    let second_successor = recreated_graph
        .add_mock("SecondChild", Body::default(), Some(recreated_parent.e))
        .expect("second successor appends");
    assert!(suggestions_at(&recreated_graph, second_successor.e.entry().0).is_empty());
}

#[test]
fn stage9_sidecar_schema_and_recursive_payloads_are_exact() {
    let document = parse_sidecar(
        br#"
[[entry]]
alias = "Nested"
ty = "Note"

[entry.body]
text = "hello"
number = -7
nested = { sequence = ["outer", { ordered_set = [1, 2] }] }
"#,
    )
    .expect("exact sidecar schema parses");
    assert_eq!(document.entry.len(), 1);
    let built = build_sidecar_plan(&PersistedState::new(LifeCoreList::default()), document)
        .expect("recursive payload sidecar stages");
    let node = built
        .staged
        .lifecore
        .node(EntryIndex(0).before_and_including())
        .expect("sidecar node exists");
    assert_eq!(node.field["text"], Payload::Text("hello".to_string()));
    assert_eq!(node.field["number"], Payload::Number(-7));
    assert_eq!(
        node.field["nested"],
        Payload::Sequence(vec![
            Payload::Text("outer".to_string()),
            Payload::OrderedSet(OrderedSet::from([Payload::Number(1), Payload::Number(2)])),
        ])
    );

    assert!(
        parse_sidecar(b"")
            .expect("empty document is valid")
            .entry
            .is_empty()
    );
    for invalid in [
        "[[entry]\nty = \"Note\"",
        "unknown = 1",
        "[[entry]]\nty = \"Note\"\nunknown = 1",
        "[[entry]]\nty = \"Note\"\n[entry.connect]\nunknown = []",
        "[[entry]]\nty = \"Note\"\n[entry.connect]\nconnect_to = [{ target = \"HEAD\", ty = \"Rel\", unknown = 1 }]",
    ] {
        assert!(
            matches!(
                parse_sidecar(invalid.as_bytes()),
                Err(AppError::InvalidSidecar { .. })
            ),
            "{invalid:?}"
        );
    }

    for unsupported in [
        "[[entry]]\nty = \"Note\"\n[entry.body]\nvalue = true",
        "[[entry]]\nty = \"Note\"\n[entry.body]\nvalue = 1.5",
        "[[entry]]\nty = \"Note\"\n[entry.body]\nvalue = [1, 2]",
        "[[entry]]\nty = \"Note\"\n[entry.body]\nvalue = 1979-05-27T07:32:00Z",
        "[[entry]]\nty = \"Note\"\n[entry.body]\nvalue = { arbitrary = [] }",
        "[[entry]]\nty = \"Note\"\n[entry.body]\nvalue = { ordered_set = [1, 1] }",
    ] {
        let document = parse_sidecar(unsupported.as_bytes()).expect("TOML shape decodes first");
        assert!(
            matches!(
                build_sidecar_plan(&PersistedState::new(LifeCoreList::default()), document),
                Err(AppError::InvalidSidecar { .. })
            ),
            "{unsupported:?}"
        );
    }
}

#[test]
fn stage9_planner_preserves_order_head_timing_aliases_and_directions() {
    let mut graph = LifeCoreList::default();
    graph
        .add_mock(
            "Note",
            Body::from([("id", Payload::Text("base".to_string()))]),
            None,
        )
        .expect("base node stages");
    graph
        .add_mock(
            "Note",
            Body::from([("id", Payload::Text("head".to_string()))]),
            None,
        )
        .expect("head node stages");
    graph.apply().expect("canonical nodes apply");
    let mut canonical = PersistedState::new(graph);
    let base = resolve_index(
        &canonical.lifecore,
        &canonical.cli,
        0,
        EntryExpectation::Node,
        None,
    )
    .expect("base resolves");
    canonical
        .cli
        .aliases
        .insert("Base".to_string(), base.durable_locator());
    let before = serde_json::to_vec(&canonical).expect("canonical state encodes");
    let document = parse_sidecar(
        br#"
[[entry]]
alias = "First"
ty = "Note"

[entry.connect]
connect_to = [
    { target = "HEAD", ty = "Captured" },
    { target = "First", ty = "Self" },
]
connect_back_from = [{ target = "Base", ty = "Back" }]

[[entry]]
alias = "Second"
ty = "Note"
parent = "First"

[[entry]]
ty = "Note"
parent = "HEAD"
"#,
    )
    .expect("planner fixture parses");
    let built = build_sidecar_plan(&canonical, document).expect("planner fixture stages");

    assert_eq!(
        serde_json::to_vec(&canonical).expect("canonical state still encodes"),
        before
    );
    let first = built
        .staged
        .lifecore
        .node(EntryIndex(2).before_and_including())
        .expect("first node occupies first suffix index");
    let captured = built
        .staged
        .lifecore
        .edge(EntryIndex(3).before_and_including())
        .expect("captured-head edge follows node");
    let self_edge = built
        .staged
        .lifecore
        .edge(EntryIndex(4).before_and_including())
        .expect("self edge retains source order");
    let back = built
        .staged
        .lifecore
        .edge(EntryIndex(5).before_and_including())
        .expect("back edge follows outgoing edges");
    let second = built
        .staged
        .lifecore
        .node(EntryIndex(6).before_and_including())
        .expect("second node follows first entry");
    let third = built
        .staged
        .lifecore
        .node(EntryIndex(7).before_and_including())
        .expect("third node follows second entry");

    assert_eq!(first.parent, None);
    assert_eq!((captured.from.0, captured.to.0), (2, 1));
    assert_eq!((self_edge.from.0, self_edge.to.0), (2, 2));
    assert_eq!((back.from.0, back.to.0), (0, 2));
    assert_eq!(second.parent, Some(EntryIndex(2)));
    assert_eq!(third.parent, Some(EntryIndex(6)));
    assert_eq!(
        built
            .plan
            .temporary_aliases
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["First", "Second"]
    );

    for invalid in [
        "[[entry]]\nalias = \"Base\"\nty = \"Note\"",
        "[[entry]]\nalias = \"HEAD\"\nty = \"Note\"",
        "[[entry]]\nalias = \"Dupe\"\nty = \"Note\"\n[[entry]]\nalias = \"Dupe\"\nty = \"Note\"",
        "[[entry]]\nty = \"Note\"\nparent = \"Later\"\n[[entry]]\nalias = \"Later\"\nty = \"Note\"",
    ] {
        let document = parse_sidecar(invalid.as_bytes()).expect("invalid plan still parses");
        assert!(
            matches!(
                build_sidecar_plan(&canonical, document),
                Err(AppError::InvalidSidecar { .. })
            ),
            "{invalid:?}"
        );
        assert_eq!(
            serde_json::to_vec(&canonical).expect("rollback state encodes"),
            before
        );
    }
}

#[test]
fn stage9_attach_and_detach_are_metadata_only_and_byte_reversible() {
    let directory = tempfile::tempdir().expect("temporary attachment directory");
    initialize_stage8_state(directory.path());
    let state_path = directory.path().join("LifeCoreState.json");
    let sidecar_path = directory.path().join("broken.toml");
    fs::write(&sidecar_path, b"not valid = [").expect("malformed sidecar writes");
    let source_before = fs::read(&sidecar_path).expect("source bytes read");
    let state_before = fs::read(&state_path).expect("state bytes read");

    let attached = run_in(
        &["lifecore", "sidecar", "attach", "broken.toml"],
        directory.path(),
        None,
    )
    .expect("attach does not parse source");
    match attached {
    CommandOutcome::SidecarAttachment { action, path } => {
        assert_eq!(action, "attached");
        assert!(path.is_absolute());
        assert_eq!(
            path,
            fs::canonicalize(&sidecar_path).expect("source canonicalizes")
        );
    }
    outcome => panic!("expected attachment outcome, got {outcome:?}"),
    }
    assert_eq!(
        fs::read(&sidecar_path).expect("attached source remains readable"),
        source_before
    );
    assert!(matches!(
        run_in(
            &["lifecore", "sidecar", "attach", "broken.toml"],
            directory.path(),
            None,
        ),
        Err(AppError::SidecarAlreadyAttached { .. })
    ));
    assert!(matches!(
        run_in(
            &["lifecore", "query", "nodes", "all"],
            directory.path(),
            None,
        ),
        Err(AppError::SidecarUnavailable { .. })
    ));

    fs::remove_file(&sidecar_path).expect("attached source is deleted externally");
    let detached = run_in(&["lifecore", "sidecar", "detach"], directory.path(), None)
        .expect("detach never reads deleted source");
    assert!(matches!(
        detached,
        CommandOutcome::SidecarAttachment { action, .. } if action == "detached"
    ));
    assert_eq!(
        fs::read(&state_path).expect("detached state bytes read"),
        state_before
    );
    assert!(matches!(
        run_in(&["lifecore", "sidecar", "detach"], directory.path(), None,),
        Err(AppError::SidecarNotAttached { .. })
    ));
}

#[test]
fn stage9_effective_states_distinguish_ready_diagnosable_and_unavailable() {
    let directory = tempfile::tempdir().expect("temporary effective-state directory");
    let canonical = initialize_stage8_state(directory.path());
    let registry = AnalyzerRegistry::built_in();
    assert!(matches!(
        prepare_effective(&canonical, &registry),
        EffectiveState::EffectiveReady(graph) if !graph.is_attached()
    ));

    let empty_path = directory.path().join("empty.toml");
    fs::write(&empty_path, b"").expect("empty sidecar writes");
    let mut attached_empty = canonical.clone();
    attached_empty.cli.sidecar_path =
        Some(fs::canonicalize(&empty_path).expect("empty sidecar canonicalizes"));
    assert!(matches!(
        prepare_effective(&attached_empty, &registry),
        EffectiveState::EffectiveReady(graph) if graph.is_attached()
    ));

    let inconsistent_path = directory.path().join("inconsistent.toml");
    fs::write(
        &inconsistent_path,
        b"[[entry]]\nty = \"No_Such_Type\"\n[[entry]]\nty = \"Still_Missing\"\n",
    )
    .expect("inconsistent sidecar writes");
    let mut inconsistent = canonical.clone();
    inconsistent.cli.sidecar_path =
        Some(fs::canonicalize(&inconsistent_path).expect("inconsistent sidecar canonicalizes"));
    assert!(matches!(
        prepare_effective(&inconsistent, &registry),
        EffectiveState::SidecarDiagnosable(_)
    ));

    let malformed_path = directory.path().join("malformed.toml");
    fs::write(&malformed_path, b"[[entry]").expect("malformed sidecar writes");
    let mut malformed = canonical.clone();
    malformed.cli.sidecar_path =
        Some(fs::canonicalize(&malformed_path).expect("malformed sidecar canonicalizes"));
    assert!(matches!(
        prepare_effective(&malformed, &registry),
        EffectiveState::SidecarUnavailable { .. }
    ));

    let mut missing_factory = attached_empty;
    missing_factory
        .lifecore
        .add_analyzer("Stage9_Missing_Factory", "");
    assert!(matches!(
        prepare_effective(&missing_factory, &registry),
        EffectiveState::SidecarUnavailable { reason, .. }
            if reason.contains("Stage9_Missing_Factory")
    ));
}

#[test]
fn stage9_empty_commit_and_unavailable_recovery_preserve_canonical_graph() {
    let directory = tempfile::tempdir().expect("temporary recovery directory");
    initialize_stage8_state(directory.path());
    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    let state_before = fs::read(store.path()).expect("canonical state bytes read");
    let empty = directory.path().join("empty.toml");
    fs::write(&empty, b"").expect("empty sidecar writes");
    run_in(
        &["lifecore", "sidecar", "attach", "empty.toml"],
        directory.path(),
        None,
    )
    .expect("empty sidecar attaches");
    let committed = run_in(&["lifecore", "sidecar", "commit"], directory.path(), None)
        .expect("empty sidecar commits");
    assert!(matches!(
        committed,
        CommandOutcome::SidecarCommitted { result, .. } if result.entries.is_empty()
    ));
    assert_eq!(
        fs::read(store.path()).expect("empty commit state reads"),
        state_before
    );

    let invalid = directory.path().join("invalid.toml");
    fs::write(
        &invalid,
        b"[[entry]]\nty = \"Type\"\nparent = \"HEAD\"\n[entry.connect]\nconnect_to = [{ target = \"HEAD\", ty = \"Rel\" }]\n",
    )
    .expect("basically invalid sidecar writes");
    run_in(
        &["lifecore", "sidecar", "attach", "invalid.toml"],
        directory.path(),
        None,
    )
    .expect("basically invalid sidecar attaches");
    assert!(matches!(
        run_in(
            &["lifecore", "entry", "show", "0:abcde"],
            directory.path(),
            None,
        ),
        Err(AppError::SidecarUnavailable { .. })
    ));
    run_in(&["lifecore", "sidecar", "detach"], directory.path(), None)
        .expect("basically invalid sidecar detaches");

    let mut missing_factory = store.load().expect("canonical state reloads");
    missing_factory
        .lifecore
        .add_analyzer("Stage9_Missing_Factory", "");
    store
        .save(&missing_factory)
        .expect("missing-factory recovery fixture saves");
    run_in(
        &["lifecore", "sidecar", "attach", "empty.toml"],
        directory.path(),
        None,
    )
    .expect("missing-factory sidecar attaches");
    assert!(matches!(
        run_in(
            &["lifecore", "alias", "list"],
            directory.path(),
            None,
        ),
        Err(AppError::SidecarUnavailable { reason, .. })
            if reason.contains("Stage9_Missing_Factory")
    ));
    run_in(&["lifecore", "sidecar", "detach"], directory.path(), None)
        .expect("missing-factory sidecar detaches");
}

#[test]
fn stage9_ready_sidecar_feeds_every_read_surface_and_commits_atomically() {
    let directory = tempfile::tempdir().expect("temporary ready-sidecar directory");
    let (canonical, _, _) = initialize_stage9_ready_state(directory.path());
    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    let sidecar_path = directory.path().join("ready.toml");
    fs::write(&sidecar_path, stage9_ready_sidecar()).expect("ready sidecar writes");
    let source_before = fs::read(&sidecar_path).expect("ready source bytes read");
    run_in(
        &["lifecore", "sidecar", "attach", "ready.toml"],
        directory.path(),
        None,
    )
    .expect("ready sidecar attaches");
    let attached_bytes = fs::read(store.path()).expect("attached state bytes read");

    let dumped = run_in(
        &["lifecore", "sidecar", "dump", "--format", "json"],
        directory.path(),
        None,
    )
    .expect("ready suffix dumps");
    let (predicted, effective_end) = match &dumped {
    CommandOutcome::SidecarDump { view, items, .. } => {
        assert_eq!(view.canonical_end, 5);
        assert_eq!(items.len(), 6);
        assert!(items.iter().all(|item| item.entry.provisional));
        assert_eq!(items[0].temporary_aliases, ["SideTypeAlias"]);
        assert_eq!(items[1].temporary_aliases, ["TempFirst"]);
        assert_eq!(
            items
                .iter()
                .map(|item| item.entry.index)
                .collect::<Vec<_>>(),
            vec![5, 6, 7, 8, 9, 10]
        );
        (
            items
                .iter()
                .map(|item| item.entry.locator.clone())
                .collect::<Vec<_>>(),
            view.end,
        )
    }
    outcome => panic!("expected sidecar dump, got {outcome:?}"),
    };
    let last_locator = predicted.last().expect("last predicted locator").clone();
    let mut dumped_json = Vec::new();
    render_outcome(&dumped, &mut dumped_json, false).expect("sidecar dump JSON renders");
    let dumped_json: serde_json::Value =
        serde_json::from_slice(&dumped_json).expect("sidecar dump output is JSON");
    assert_eq!(dumped_json["view"]["canonical_end"], 5);
    assert_eq!(dumped_json["items"][0]["provisional"], true);
    assert_eq!(
        dumped_json["items"][0]["temporary_aliases"][0],
        "SideTypeAlias"
    );
    let dumped_lines = run_in(
        &["lifecore", "sidecar", "dump", "--format", "lines"],
        directory.path(),
        None,
    )
    .expect("sidecar dump lines prepare");
    let mut dumped_lines_output = Vec::new();
    render_outcome(&dumped_lines, &mut dumped_lines_output, false)
        .expect("sidecar dump lines render");
    let dumped_lines_output =
        String::from_utf8(dumped_lines_output).expect("sidecar dump lines are UTF-8");
    assert!(dumped_lines_output.contains("provisional=true"));
    assert!(dumped_lines_output.contains("temporary_aliases=SideTypeAlias"));

    let queried = run_in(
        &[
            "lifecore",
            "query",
            "nodes",
            "all",
            "--where",
            "fresh-outgoing Rel",
        ],
        directory.path(),
        None,
    )
    .expect("query sees provisional edges");
    match queried {
    CommandOutcome::Entries { view, items, .. } => {
        assert_eq!(view.end, effective_end);
        assert!(items.iter().any(|item| item.provisional));
    }
    outcome => panic!("expected effective query entries, got {outcome:?}"),
    }

    let shown = run_in(
        &["lifecore", "entry", "show", &last_locator],
        directory.path(),
        None,
    )
    .expect("entry show sees final provisional node");
    assert!(matches!(
        shown,
        CommandOutcome::Entries { items, .. } if items[0].provisional
    ));
    let types = run_in(&["lifecore", "type", "list"], directory.path(), None)
        .expect("type list sees provisional declaration");
    assert!(matches!(
        types,
        CommandOutcome::Types { items, .. }
            if items.iter().any(|item| item.name == "SideType" && item.provisional)
    ));
    let aliases = run_in(&["lifecore", "alias", "list"], directory.path(), None)
        .expect("alias list sees temporary aliases");
    assert!(matches!(
        aliases,
        CommandOutcome::Aliases { items, .. }
            if items.iter().any(|item| item.name == "TempLast" && item.temporary)
    ));
    let analyzers = run_in(&["lifecore", "analyzer", "list"], directory.path(), None)
        .expect("analyzer list uses effective view");
    assert!(matches!(
        analyzers,
        CommandOutcome::Analyzers { view, .. } if view.end == effective_end
    ));
    let checked = run_in(
        &["lifecore", "check", "--include-resolved"],
        directory.path(),
        None,
    )
    .expect("check reuses prepared sidecar diagnostics");
    let message_id = match checked {
    CommandOutcome::Messages { view, items, .. } => {
        assert_eq!(view.end, effective_end);
        items
            .into_iter()
            .find(|item| item.analyzer == "Interactive")
            .map(|item| item.display_id)
    }
    outcome => panic!("expected effective messages, got {outcome:?}"),
    };
    if let Some(message_id) = message_id {
        assert!(matches!(
            run_in(
                &["lifecore", "message", "show", &message_id],
                directory.path(),
                None,
            ),
            Ok(CommandOutcome::Messages { view, .. }) if view.end == effective_end
        ));
    }

    let canonical_view =
        GraphView::latest(&canonical.lifecore, &canonical.cli).expect("canonical view constructs");
    let canonical_head = resolve_index(
        &canonical.lifecore,
        &canonical.cli,
        4,
        EntryExpectation::Any,
        Some(canonical_view.end_exclusive()),
    )
    .expect("canonical head resolves");
    let historical = run_in(
        &[
            "lifecore",
            "query",
            "nodes",
            "all",
            "--at",
            &canonical_head.display_locator(),
        ],
        directory.path(),
        None,
    )
    .expect("historical effective query succeeds");
    assert!(matches!(
        historical,
        CommandOutcome::Entries { view, items, .. }
            if view.end == 5 && view.canonical_end == 5
                && items.iter().all(|item| !item.provisional)
    ));

    let dot_path = directory.path().join("effective.dot");
    run_in(
        &[
            "lifecore",
            "graphviz",
            dot_path.to_str().expect("DOT path is UTF-8"),
            "--dot",
            "--full",
        ],
        directory.path(),
        None,
    )
    .expect("GraphViz sees effective suffix");
    let dot = fs::read_to_string(&dot_path).expect("effective DOT reads");
    assert!(dot.contains("provisional"));
    assert!(dot.contains("dodgerblue3"));
    assert_eq!(
        fs::read(store.path()).expect("reads preserve attached state"),
        attached_bytes
    );

    let committed = run_in(
        &["lifecore", "sidecar", "commit", "--format", "json"],
        directory.path(),
        None,
    )
    .expect("ready sidecar commits");
    match committed {
    CommandOutcome::SidecarCommitted { result, .. } => {
        assert_eq!(result.state, TransactionState::Committed);
        assert_eq!(
            result
                .entries
                .iter()
                .map(|item| item.locator.clone())
                .collect::<Vec<_>>(),
            predicted
        );
        assert!(result.entries.iter().all(|item| !item.provisional));
    }
    outcome => panic!("expected sidecar commit, got {outcome:?}"),
    }
    let committed = store.load().expect("committed sidecar state loads");
    assert_eq!(committed.cli.sidecar_path, None);
    assert!(!committed.lifecore.is_mock());
    for alias in ["SideTypeAlias", "TempFirst", "TempChild", "TempLast"] {
        assert!(committed.cli.aliases.contains_key(alias), "{alias}");
    }
    assert_eq!(
        fs::read(&sidecar_path).expect("committed source bytes read"),
        source_before
    );
    assert!(matches!(
        run_in(&["lifecore", "sidecar", "dump"], directory.path(), None,),
        Err(AppError::SidecarNotAttached { .. })
    ));
}

#[test]
fn stage9_attachment_gates_every_mutation_before_sidecar_read() {
    let directory = tempfile::tempdir().expect("temporary mutation-gate directory");
    let (_, base, other) = initialize_stage9_ready_state(directory.path());
    let sidecar_path = directory.path().join("ready.toml");
    fs::write(&sidecar_path, stage9_ready_sidecar()).expect("ready sidecar writes");
    run_in(
        &["lifecore", "sidecar", "attach", "ready.toml"],
        directory.path(),
        None,
    )
    .expect("sidecar attaches");
    fs::remove_file(&sidecar_path).expect("source is removed after attach");
    let state_path = directory.path().join("LifeCoreState.json");
    let before = fs::read(&state_path).expect("attached bytes read");
    let cases = vec![
        vec!["lifecore", "add", "Note"],
        vec![
            "lifecore", "connect", "--from", &base, "--type", "Rel", "--to", &other,
        ],
        vec!["lifecore", "analyzer", "add", "Missing", ""],
        vec!["lifecore", "alias", "set", "Blocked", &base],
        vec!["lifecore", "alias", "remove", "Base"],
        vec!["lifecore", "type", "create", "BlockedType"],
        vec![
            "lifecore",
            "type",
            "override",
            "Note",
            "--field",
            "extra = Text : Optional",
        ],
        vec!["lifecore", "message", "dismiss", "m:abcde"],
        vec!["lifecore", "message", "restore", "m:abcde"],
        vec!["lifecore", "message", "apply", "m:abcde"],
        vec![
            "lifecore", "query", "nodes", "all", "--select", &base, "--", "alias", "set", "Blocked",
        ],
        vec![
            "lifecore",
            "query",
            "nodes",
            "all",
            "--select",
            &base,
            "--",
            "add-child",
            "Note",
        ],
        vec![
            "lifecore",
            "query",
            "nodes",
            "all",
            "--select",
            &base,
            "--",
            "type",
            "override",
            "--field",
            "extra = Text : Optional",
        ],
    ];

    for args in cases {
        assert!(
            matches!(
                run_in(&args, directory.path(), None),
                Err(AppError::MutationWithSidecar { .. })
            ),
            "{args:?}"
        );
        assert_eq!(
            fs::read(&state_path).expect("forbidden mutation preserves state"),
            before
        );
    }
    assert!(matches!(
        reject_attached_mutation(
            &StateStore::new(
                StatePath::resolve(None, directory.path()).expect("state path resolves")
            )
            .load()
            .expect("attached state loads"),
            "direct test",
        ),
        Err(AppError::MutationWithSidecar { .. })
    ));
    run_in(&["lifecore", "sidecar", "detach"], directory.path(), None)
        .expect("deleted sidecar detaches");
}

#[test]
fn stage9_diagnosable_check_reaches_final_entry_and_detach_recovers() {
    let directory = tempfile::tempdir().expect("temporary diagnosable directory");
    let canonical = initialize_stage8_state(directory.path());
    let sidecar_path = directory.path().join("inconsistent.toml");
    fs::write(
        &sidecar_path,
        b"[[entry]]\nty = \"Missing_One\"\n[[entry]]\nty = \"Missing_Two\"\n",
    )
    .expect("inconsistent sidecar writes");
    run_in(
        &[
            "lifecore",
            "sidecar",
            "attach",
            sidecar_path.to_str().expect("sidecar path is UTF-8"),
        ],
        directory.path(),
        None,
    )
    .expect("inconsistent sidecar attaches");
    assert!(matches!(
        run_in(
            &["lifecore", "query", "nodes", "all"],
            directory.path(),
            None,
        ),
        Err(AppError::SidecarDiagnosable { .. })
    ));
    let checked =
        run_in(&["lifecore", "check"], directory.path(), None).expect("diagnosable check succeeds");
    let canonical_end = canonical
        .lifecore
        .last()
        .map_or(0, |entry| entry.entry().0 + 1);
    match checked {
    CommandOutcome::Messages { view, items, .. } => {
        assert_eq!(view.end, canonical_end + 2);
        assert!(items.iter().any(|item| {
            item.severity == Severity::Inconsistency
                && matches!(
                    item.location,
                    MessageLocation::Entry { entry_index, .. }
                        if entry_index == (canonical_end + 1) as u64
                )
        }));
    }
    outcome => panic!("expected diagnosable messages, got {outcome:?}"),
    }

    fs::remove_file(&sidecar_path).expect("inconsistent source deletes");
    run_in(&["lifecore", "sidecar", "detach"], directory.path(), None)
        .expect("diagnosable deleted source detaches");
}

#[test]
fn stage9_commit_rolls_back_stage_apply_expectation_and_save_failures() {
    let directory = tempfile::tempdir().expect("temporary sidecar rollback directory");
    let source = directory.path().join("one.toml");
    fs::write(&source, b"[[entry]]\nalias = \"One\"\nty = \"Note\"\n")
        .expect("rollback sidecar writes");
    let mut state = PersistedState::new(LifeCoreList::default());
    state.cli.sidecar_path =
        Some(fs::canonicalize(&source).expect("rollback sidecar canonicalizes"));
    let store =
        StateStore::new(StatePath::resolve(None, directory.path()).expect("state path resolves"));
    store
        .initialize(&state)
        .expect("attached rollback state initializes");
    let built = build_sidecar_plan(
        &state,
        parse_sidecar(&fs::read(&source).expect("rollback source reads"))
            .expect("rollback source parses"),
    )
    .expect("rollback plan builds");
    let before = fs::read(store.path()).expect("rollback state bytes read");
    let registry = AnalyzerRegistry::built_in();

    let staged_failure = TransactionService::new(&store, &registry).execute_sidecar(
        &state,
        PlannedChange::AddEdge {
            from: EntryIndex(99),
            type_name: "Rel".to_string(),
            to: EntryIndex(100),
        },
        BTreeMap::new(),
        None,
    );
    assert!(matches!(
        staged_failure,
        Err(AppError::TransactionAborted { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("stage failure preserves bytes"),
        before
    );

    let apply_failure = TransactionService::failing_apply_for_test(&store, &registry)
        .execute_sidecar(
            &state,
            built.plan.change.clone(),
            built.plan.temporary_aliases.clone(),
            None,
        );
    assert!(matches!(
        apply_failure,
        Err(AppError::TransactionAborted { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("apply failure preserves bytes"),
        before
    );

    let expectation_failure = TransactionService::new(&store, &registry).execute_sidecar(
        &state,
        built.plan.change.clone(),
        built.plan.temporary_aliases.clone(),
        Some(ExpectedHead::Entry(
            EntryLocator::parse("0:abcde").expect("wrong expectation parses"),
        )),
    );
    assert!(matches!(
        expectation_failure,
        Err(AppError::TransactionAborted { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("expectation failure preserves bytes"),
        before
    );

    let failing_store = store.failing_save_for_test();
    let save_failure = TransactionService::new(&failing_store, &registry).execute_sidecar(
        &state,
        built.plan.change,
        built.plan.temporary_aliases,
        None,
    );
    assert!(matches!(
        save_failure,
        Err(AppError::TransactionAborted { .. })
    ));
    assert_eq!(
        fs::read(store.path()).expect("save failure preserves bytes"),
        before
    );
    let reloaded = store.load().expect("rollback state reloads");
    assert_eq!(reloaded.cli.sidecar_path, state.cli.sidecar_path);
    assert!(reloaded.cli.aliases.get("One").is_none());
}

fn directory_snapshot(directory: &std::path::Path) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
    let mut snapshot = Vec::new();

    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let contents = fs::read(&path)?;
        snapshot.push((path, contents));
    }

    snapshot.sort();

    Ok(snapshot)
}
