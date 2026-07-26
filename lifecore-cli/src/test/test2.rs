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
    app::CommandOutcome,
    entry::entry_record,
    error::AppError,
    interactive_analyzer::InteractiveFactory,
    locator::{
        EntryExpectation, EntryFingerprint, EntryLocator, entry_fingerprint_identity_bytes,
        resolve_entry, resolve_index,
    },
    messages::{MessageUniverse, Severity, encode_lower_hex, message_identity_bytes},
    output::render_outcome,
    query::{execute_query, parse_predicate, parse_query},
    reference::{NodeReference, resolve_node_reference},
    run_cli, run_from_in,
    state::{PersistedState, StatePath, StateStore, StoredEntryLocatorV1},
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
            &[
                "lifecore",
                "add",
                "Note",
                "--field",
                "title = \"hello\"",
                "--parent",
                "HEAD",
                "--commit",
                "--expect-head",
                "EMPTY",
            ],
            "add",
        ),
        (
            &[
                "lifecore",
                "connect",
                "--from",
                "HEAD",
                "--type",
                "Relates_To",
                "--to",
                "other",
                "--commit",
            ],
            "connect",
        ),
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
        (&["lifecore", "analyzer", "list"], "analyzer list"),
        (
            &[
                "lifecore",
                "analyzer",
                "add",
                "Type_Check",
                "{}",
                "--commit",
            ],
            "analyzer add",
        ),
        (
            &[
                "lifecore",
                "graphviz",
                "graph.svg",
                "--at",
                "4:abcde",
                "--full",
                "--fields",
            ],
            "graphviz",
        ),
        (&["lifecore", "graphviz", "graph.dot", "--dot"], "graphviz"),
        (
            &[
                "lifecore",
                "check",
                "--include-dismissed",
                "--include-resolved",
            ],
            "check",
        ),
        (&["lifecore", "check", "--dismissal-ids"], "check"),
        (
            &["lifecore", "message", "dismiss", "m:abcde"],
            "message dismiss",
        ),
        (
            &["lifecore", "message", "restore", "m:abcde"],
            "message restore",
        ),
        (
            &[
                "lifecore",
                "message",
                "apply",
                "m:abcde",
                "--commit",
                "--expect-head",
                "EMPTY",
                "--keep-message",
            ],
            "message apply",
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
    let exit = run_cli(["lifecore", "add", "Note"], &mut io);

    assert_eq!(exit, std::process::ExitCode::FAILURE);
    assert!(stdout.is_empty());
    assert_eq!(
        String::from_utf8(stderr).expect("stderr is UTF-8"),
        "error: operation 'add' is unavailable in implementation stage 3\n"
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = test_io(&mut stdout, &mut stderr);
    let exit = run_cli(["lifecore", "add", "Note", "--format", "json"], &mut io);

    assert_eq!(exit, std::process::ExitCode::FAILURE);
    assert!(stdout.is_empty());

    let value: serde_json::Value =
        serde_json::from_slice(&stderr).expect("stderr contains one JSON value");
    assert_eq!(value["error"]["kind"], "unavailable");
    assert_eq!(value["error"]["operation"], "add");
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

    match run_in(&["lifecore", "add", "Note", "--commit"], &directory, None) {
    Err(AppError::Unavailable { operation, .. }) => assert_eq!(operation, "add"),
    result => panic!("expected unavailable add, got {result:?}"),
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
    assert!(table_output.starts_with("ID\tSEVERITY\tANALYZER\tLOCATION\tMESSAGE\tACTION\n"));

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
