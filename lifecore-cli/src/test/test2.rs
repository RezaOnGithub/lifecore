use std::{
    ffi::OsString,
    fs, io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use lifecore::{
    Body, Context, DiagnosticLocation, LifeCore, LifeCoreList,
    analysis::{Inconsistency, Message, SuggestedAction, Suggestion, Warning},
};
use uuid::Version;

use crate::{
    InvocationContext, Io,
    analysis::{AnalysisReport, AnalyzerRegistry, initial_graph, run_analysis},
    app::CommandOutcome,
    error::AppError,
    messages::{MessageUniverse, Severity},
    output::render_outcome,
    run_cli, run_from_in,
    state::{PersistedState, StatePath, StateStore},
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
        (&["lifecore", "entry", "show", "4:abcde"], "entry show"),
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
            &[
                "lifecore", "query", "nodes", "all", "--at", "4:abcde", "--where", "fresh",
            ],
            "query nodes",
        ),
        (
            &["lifecore", "query", "nodes", "any", "--explain", "4:abcde"],
            "query nodes",
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
                "lifecore", "query", "nodes", "all", "--select", "4:abcde", "--", "entry",
                "inspect",
            ],
            "query nodes --select -- entry inspect",
        ),
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
    match run(&["lifecore", "query", "nodes", "all", "--format", "json"]) {
    Err(AppError::Unavailable { format, .. }) => {
        assert_eq!(format, crate::cli::OutputFormat::Json);
    }
    result => panic!("expected JSON unavailable error, got {result:?}"),
    }

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
    let exit = run_cli(["lifecore", "entry", "show", "4:abcde"], &mut io);

    assert_eq!(exit, std::process::ExitCode::FAILURE);
    assert!(stdout.is_empty());
    assert_eq!(
        String::from_utf8(stderr).expect("stderr is UTF-8"),
        "error: operation 'entry show' is unavailable in implementation stage 2\n"
    );

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = test_io(&mut stdout, &mut stderr);
    let exit = run_cli(
        ["lifecore", "entry", "show", "4:abcde", "--format", "json"],
        &mut io,
    );

    assert_eq!(exit, std::process::ExitCode::FAILURE);
    assert!(stdout.is_empty());

    let value: serde_json::Value =
        serde_json::from_slice(&stderr).expect("stderr contains one JSON value");
    assert_eq!(value["error"]["kind"], "unavailable");
    assert_eq!(value["error"]["operation"], "entry show");
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
    let warning = Message::Warning(Warning {
        offending_event: DiagnosticLocation::Context(Context::universe()),
        system_name: "Test".to_string(),
        system_config: "{}".to_string(),
        relevance: Vec::new(),
        message: "same".to_string(),
    });
    let duplicate_report = AnalysisReport {
        suite: Default::default(),
        messages: vec![warning.clone(), warning],
        entry_count: 0,
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
            offending_event: DiagnosticLocation::Context(Context::universe()),
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
        entry_count: 0,
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
fn message_prefix_resolution_detects_syntax_unknown_and_ambiguity() {
    let graph = LifeCoreList::default();
    let mut messages = Vec::new();

    for index in 0..8_000 {
        messages.push(Message::Warning(Warning {
            offending_event: DiagnosticLocation::Context(Context::universe()),
            system_name: "Collision_Test".to_string(),
            system_config: String::new(),
            relevance: Vec::new(),
            message: format!("message {index}"),
        }));
    }

    let report = AnalysisReport {
        suite: Default::default(),
        messages,
        entry_count: 0,
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
            offending_event: DiagnosticLocation::Context(Context::universe()),
            system_name: "Test".to_string(),
            system_config: String::new(),
            message: "broken".to_string(),
        })],
        entry_count: 0,
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
