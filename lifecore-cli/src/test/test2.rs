use std::{
    ffi::OsString,
    fs, io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use lifecore::{
    Body, DiagnosticLocation, EntryIndex, LifeCore, LifeCoreList, OrderedSet, Payload,
    analysis::{AnalyzerFactory, Inconsistency, Message, SuggestedAction, Suggestion, Warning},
};
use lifecore_typecheck::TypeCheckFactory;
use uuid::{Uuid, Version};

use crate::{
    InvocationContext, Io,
    analysis::{
        AnalysisReport, AnalyzerRegistry, configured_analyzers, initial_graph, run_analysis,
    },
    app::{CircumstanceRecord, CommandOutcome},
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
        MessageFilter, MessageUniverse, Severity, encode_lower_hex, message_identity_bytes,
    },
    output::{render_error, render_outcome},
    payload_input::{parse_field_assignment, parse_field_assignments},
    query::{execute_query, parse_predicate, parse_query},
    reference::{NodeReference, resolve_node_reference},
    run_cli, run_from_in,
    state::{PersistedState, StatePath, StateStore, StoredEntryLocatorV1},
    transaction::{
        ExpectedHead, PlannedChange, TransactionRequest, TransactionService, TransactionState,
        verify_expected_head,
    },
    view::{GraphView, ViewSpec},
};

fn test_io<'a>(stdout: &'a mut Vec<u8>, stderr: &'a mut Vec<u8>) -> Io<'a> {
    Io::new(stdout, stderr, false, false)
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

fn assert_unavailable(args: &[&str], expected_operation: &str) {
    match run(args) {
    Err(AppError::Unavailable { operation, .. }) => {
        assert_eq!(operation, expected_operation);
    }
    result => panic!("expected unavailable operation, got {result:?}"),
    }
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
        &["lifecore", "interactive", "--help"],
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
        assert!(text.contains("interactive  Start the menu-driven TTY interface"));
        assert!(!text.contains("-i,"));
        assert!(!text.contains("--interactive"));
    }
    result => panic!("root help failed: {result:?}"),
    }
}

#[test]
fn complete_command_tree_dispatches_to_named_unavailable_operations() {
    let cases: &[(&[&str], &str)] = &[
        (&["lifecore", "interactive"], "interactive"),
        (
            &["lifecore", "alias", "list", "--at", "4:abcde"],
            "alias list",
        ),
        (
            &["lifecore", "alias", "set", "chosen", "4:abcde"],
            "alias set",
        ),
        (&["lifecore", "alias", "remove", "chosen"], "alias remove"),
        (
            &["lifecore", "type", "list", "--at", "4:abcde"],
            "type list",
        ),
        (&["lifecore", "type", "show", "4:abcde"], "type show"),
        (
            &[
                "lifecore",
                "type",
                "create",
                "Task",
                "--field",
                "title = Text : Mandatory",
                "--outgoing",
                "Blocks",
                "--incoming",
                "Blocked_By",
                "--allows-override",
                "ArchivedTask",
                "--commit",
            ],
            "type create",
        ),
        (
            &[
                "lifecore",
                "type",
                "override",
                "Task",
                "--at",
                "4:abcde",
                "--as",
                "FocusedTask",
                "--drop-field",
                "legacy",
                "--allow-outgoing",
                "Relates_To",
                "--disallow-outgoing",
                "Blocks",
                "--allow-incoming",
                "Mentions",
                "--disallow-incoming",
                "Blocked_By",
                "--allows-override",
                "ArchivedTask",
                "--disallow-override",
                "OldTask",
            ],
            "type override",
        ),
        (
            &["lifecore", "sidecar", "attach", "entries.toml"],
            "sidecar attach",
        ),
        (&["lifecore", "sidecar", "detach"], "sidecar detach"),
        (&["lifecore", "sidecar", "dump"], "sidecar dump"),
        (
            &["lifecore", "sidecar", "commit", "--expect-head", "4:abcde"],
            "sidecar commit",
        ),
    ];

    for (args, operation) in cases {
        assert_unavailable(args, operation);
    }
}

#[test]
fn query_selection_actions_have_typed_nested_grammar() {
    let cases: &[(&[&str], &str)] = &[
        (
            &[
                "lifecore", "query", "nodes", "all", "--select", "4:abcde", "--", "alias", "set",
                "chosen",
            ],
            "query nodes --select -- alias set",
        ),
        (
            &[
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
            ],
            "query nodes --select -- type override",
        ),
        (
            &[
                "lifecore",
                "query",
                "nodes",
                "none",
                "--select",
                "4:abcde",
                "--",
                "add-child",
                "Task",
                "--field",
                "title = \"child\"",
            ],
            "query nodes --select -- add-child",
        ),
    ];

    for (args, operation) in cases {
        assert_unavailable(args, operation);
    }

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
            &["lifecore", "interactive", "--format", "json"],
            "invalid value 'json'",
        ),
        (
            &["lifecore", "interactive", "check"],
            "unexpected argument 'check'",
        ),
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
    match run(&["lifecore", "interactive", "--format", "table"]) {
    Err(AppError::Unavailable { format, .. }) => {
        assert_eq!(format, crate::cli::OutputFormat::Table);
    }
    result => panic!("expected table unavailable error, got {result:?}"),
    }

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
fn unavailable_errors_render_to_stderr_in_human_and_json_modes() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = test_io(&mut stdout, &mut stderr);
    let exit = run_cli(["lifecore", "type", "list"], &mut io);

    assert_eq!(exit, std::process::ExitCode::FAILURE);
    assert!(stdout.is_empty());
    assert_eq!(
        String::from_utf8(stderr).expect("stderr is UTF-8"),
        "error: operation 'type list' is unavailable in implementation stage 6\n"
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = test_io(&mut stdout, &mut stderr);
    let exit = run_cli(["lifecore", "type", "list", "--format", "json"], &mut io);

    assert_eq!(exit, std::process::ExitCode::FAILURE);
    assert!(stdout.is_empty());

    let value: serde_json::Value =
        serde_json::from_slice(&stderr).expect("stderr contains one JSON value");
    assert_eq!(value["error"]["kind"], "unavailable");
    assert_eq!(value["error"]["operation"], "type list");
}

#[test]
fn deferred_commands_do_not_create_or_modify_files() -> io::Result<()> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "lifecore-cli-stage1-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory)?;

    let sentinel = directory.join("sentinel");
    fs::write(&sentinel, b"unchanged")?;
    let before = directory_snapshot(&directory)?;

    match run_in(&["lifecore", "check"], &directory, None) {
    Err(AppError::StateIo { source, .. }) => {
        assert_eq!(source.kind(), io::ErrorKind::NotFound);
    }
    result => panic!("expected missing canonical state, got {result:?}"),
    }

    match run_in(
        &["lifecore", "alias", "set", "chosen", "0:abcde"],
        &directory,
        None,
    ) {
    Err(AppError::Unavailable { operation, .. }) => assert_eq!(operation, "alias set"),
    result => panic!("expected unavailable alias mutation, got {result:?}"),
    }

    let after = directory_snapshot(&directory)?;
    assert_eq!(after, before);
    assert!(!directory.join("LifeCoreState.json").exists());

    fs::remove_dir_all(directory)
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
        NodeReference::Alias("head".to_string())
    );

    let durable = state.cli.aliases["chosen"].clone();
    state.cli.aliases.insert("bad-name".to_string(), durable);
    assert!(matches!(
        state.validate(),
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
