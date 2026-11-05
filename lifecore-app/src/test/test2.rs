use crate::{
    AppError, Service,
    proposal::{self, Format, PlanId, Proposal},
    query::{
        EdgeFilter, EntryInspection, Expansion, Incidence, Provenance, RefusedSeed, Seed,
        SeedRefusal, ViewQuery,
    },
    render::{self, GraphvizFailure, Limits, RenderFormat},
    rpc,
    schema::{self, NodeCounts},
    service::{
        AddAnalyzerRequest, AnalysisRunRequest, DismissRequest, DismissalList, EntryGetRequest,
        EntryInspectRequest, EntryListRequest, EntryRecord, NoParams, ProposalRequest, Published,
        RestoreRequest, RestoreResult, TypeListRequest, ViewQueryRequest, ViewRenderRequest,
    },
    storage::{self, DEFAULT_STATE_FILE, Publication, Snapshot, StatePath},
};
use lifecore::{
    Body, Entry, EntryId, LifeCoreList, Node, OffsetTimestamp, Payload, Timestamp,
    analysis::{
        AnalysisRunner, AnalyzerCertificate, AnalyzerFactory, AnalyzerIdentity, Config,
        EvaluatedView, FindingKey, MessageType,
        presentation::{Dismissal, DismissalReason},
    },
    inspection::{Currentness, Direction},
    transaction::{self, Change, EntryChange, NewEdge, NewNode, RevisionIntent},
    typing::{
        Declaration, FieldDecl, FieldStatus, PathPart, SchemaError, SchemaErrorKind, Type,
        TypeResolution, ValueType,
    },
};
use lifecore_dag_check::DagCheckFactory;
use lifecore_typecheck::TypeCheckFactory;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs, io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
use uuid::Uuid;

// Short enough to keep busy tests fast; see `with_busy_retry`.
const SHORT_RETRY: Duration = Duration::from_millis(50);

// A private directory per test, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("lifecore-app-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn state(&self) -> StatePath {
        StatePath::new(self.0.join(DEFAULT_STATE_FILE))
    }
    fn service(&self) -> Service {
        service(self.state())
    }
    fn bytes(&self) -> Vec<u8> {
        fs::read(self.state().path()).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn temporary_files(scratch: &Scratch) -> usize {
    fs::read_dir(&scratch.0)
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        })
        .count()
}

fn service(state: StatePath) -> Service {
    Service::new(
        state,
        Box::new(|| -> Vec<Box<dyn AnalyzerFactory>> {
            vec![Box::new(TypeCheckFactory), Box::new(DagCheckFactory)]
        }),
    )
}

fn typecheck() -> Vec<Config> {
    vec![Config {
        name: TypeCheckFactory::name().into(),
        configuration: "{}".into(),
    }]
}

fn task_type() -> Body {
    Type::new(
        "Task",
        &[
            ("title", (ValueType::Text, FieldStatus::Mandatory)),
            ("status", (ValueType::Text, FieldStatus::Optional)),
            ("notes", (ValueType::Text, FieldStatus::Encouraged)),
        ],
        &["DependsOn"],
        &["DependsOn"],
        &[],
    )
    .to_body()
}

fn proposal(entries: Value) -> Proposal {
    serde_json::from_value(json!({"entries": entries})).unwrap()
}

fn request(proposal: Proposal) -> ProposalRequest {
    ProposalRequest {
        proposal,
        evaluation_time: None,
        now: false,
    }
}

fn task(name: &str, title: &str) -> Value {
    json!({"name": name, "kind": "node", "ty_name": "Task",
        "payload": {"title": title, "notes": "n"}})
}

// Initialize with TypeCheck, declare Task, and add two tasks and an edge.
fn populated(scratch: &Scratch) -> Vec<EntryId> {
    let service = scratch.service();
    service.init(&typecheck()).unwrap();
    let applied = service
        .apply(request(proposal(json!([
            {"name": "task_type", "kind": "node", "ty_name": "Type", "payload": task_type()},
            task("a", "A"),
            task("b", "B"),
            {"name": "a_b", "kind": "edge", "ty_name": "DependsOn",
                "from": {"added": "a"}, "to": {"added": "b"}},
        ]))))
        .unwrap();
    applied
        .report
        .operations
        .iter()
        .map(|operation| operation.entry)
        .collect()
}

fn existing(id: EntryId) -> Value {
    json!({"existing": serde_json::to_value(id).unwrap()})
}

const FILE_JSON: &str = r#"{
  "plan_id": "lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc",
  "entries": [
    {"name": "task_done", "kind": "node", "ty_name": "Task", "intent": "revise",
     "predecessor": {"existing": {"origin": "11111111-1111-4111-8111-111111111111", "position": "42"}},
     "payload": {"title": "Write proposal", "status": "done", "priority": 9223372036854775807,
       "deadline": {"kind": "timestamp", "value": "2026-09-30T17:00:00.000000001Z"},
       "tags": [], "details": {"kind": "record", "value": {"kind": "integer", "value": "1"}},
       "rows": [{"kind": "record", "value": {}}, [], -9223372036854775808],
       "done": {"kind": "boolean", "value": true}}},
    {"name": "explanation_link", "kind": "edge", "ty_name": "Explains",
     "from": {"added": "task_done"}, "to": {"added": "task_done"}}
  ]
}"#;

const FILE_TOML: &str = r#"
plan_id = "lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc"

[[entries]]
name = "task_done"
kind = "node"
ty_name = "Task"
predecessor = { existing = { origin = "11111111-1111-4111-8111-111111111111", position = "42" } }
intent = "revise"

[entries.payload]
title = "Write proposal"
status = "done"
priority = { kind = "integer", value = "9223372036854775807" }
deadline = { kind = "timestamp", value = "2026-09-30T17:00:00.000000001Z" }
tags = []
details = { kind = "record", value = { kind = "integer", value = "1" } }
rows = [{ kind = "record", value = {} }, [], -9223372036854775808]
done = { kind = "boolean", value = true }

[[entries]]
kind = "edge"
name = "explanation_link"
ty_name = "Explains"
from = { added = "task_done" }
to = { added = "task_done" }
"#;

#[test]
fn equivalent_json_and_toml_proposals_decode_equally_and_export_canonically() {
    let from_json = proposal::decode(FILE_JSON, Format::Json).unwrap();
    let from_toml = proposal::decode(FILE_TOML, Format::Toml).unwrap();
    assert_eq!(from_json, from_toml);
    let proposal::ProposalEntry::Node { payload, .. } = &from_json.entries[0] else {
        panic!("expected a node")
    };
    assert_eq!(payload["priority"], Payload::Integer(i64::MAX));
    assert_eq!(
        payload["rows"],
        Payload::List(vec![
            Payload::Record(Body::new()),
            Payload::List(vec![]),
            Payload::Integer(i64::MIN)
        ])
    );
    // A record whose fields look like an envelope stays a record.
    assert_eq!(
        payload["details"],
        Payload::Record(Body::from([
            ("kind".into(), "integer".into()),
            ("value".into(), "1".into())
        ]))
    );
    let deadline: Timestamp = "2026-09-30T17:00:00.000000001Z".parse().unwrap();
    assert_eq!(payload["deadline"], Payload::Timestamp(deadline));
    for format in [Format::Json, Format::Toml] {
        let text = proposal::encode(&from_json, format).unwrap();
        assert_eq!(proposal::decode(&text, format).unwrap(), from_json);
        // Exports always use integer envelopes, never the bare shorthand.
        assert!(!text.contains("= 9223372036854775807"));
        assert!(!text.contains(": 9223372036854775807"));
        assert!(text.contains("9223372036854775807\""));
        assert!(text.contains("2026-09-30T17:00:00.000000001Z"));
        assert!(!text.contains("acknowledges"));
    }
    let json_text = proposal::encode(&from_json, Format::Json).unwrap();
    let toml_text = proposal::encode(&from_json, Format::Toml).unwrap();
    assert_eq!(
        proposal::decode(&toml_text, Format::Toml).unwrap(),
        proposal::decode(&json_text, Format::Json).unwrap()
    );
}

#[test]
fn acknowledges_and_plan_ids_are_preserved_across_formats() {
    let mut value: Value = serde_json::from_str(FILE_JSON).unwrap();
    value["acknowledges"] = json!({
        "database": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "analyzer": {"name": "Repairs", "version": "1.2.0-rc.1+build", "configuration": " {} "},
        "kind": "suggestion",
        "analyzer_certificate": "opaque \"certificate\"\n"
    });
    let original: Proposal = serde_json::from_value(value).unwrap();
    let toml_text = proposal::encode(&original, Format::Toml).unwrap();
    let from_toml = proposal::decode(&toml_text, Format::Toml).unwrap();
    assert_eq!(from_toml, original);
    let json_text = proposal::encode(&from_toml, Format::Json).unwrap();
    assert_eq!(
        proposal::decode(&json_text, Format::Json).unwrap(),
        original
    );
    let plan_id = original.plan_id.unwrap();
    assert_eq!(plan_id.to_string().parse::<PlanId>().unwrap(), plan_id);
    assert_eq!(
        plan_id.to_string(),
        "lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc"
    );
}

#[test]
fn malformed_files_envelopes_and_tokens_are_rejected_with_locations() {
    let toml_cases = [
        // A native TOML datetime is not timestamp payload syntax.
        "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\npayload = { at = 2026-09-30T17:00:00Z }\n",
        "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\npayload = { n = 9223372036854775808 }\n",
        "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\npayload = { n = 1.5 }\n",
        "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\npayload = { n = { kind = \"integer\", value = 1 } }\n",
        "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\npayload = { n = { kind = \"float\", value = \"1\" } }\n",
        "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\npayload = {}\ncolor = \"red\"\n",
        "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\n",
        "[[entries]]\nname = \"a\"\nkind = \"edge\"\nty_name = \"T\"\nfrom = { added = \"x\" }\nto = { added = \"y\" }\npayload = {}\n",
        "[[entries]]\nname = \"a\"\nkind = \"delete\"\nty_name = \"T\"\n",
        // The entry type name is `ty_name`; `type` is an unknown field.
        "[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\npayload = {}\n",
        "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\npayload = {}\nintent = \"replace\"\n",
        "",
        "entries = []\ndatabase = \"x\"\n",
        "plan_id = \"lc2:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc\"\nentries = []\n",
        "plan_id = \"lc1:AAAAAAAA-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc\"\nentries = []\n",
        "plan_id = \"lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb\"\nentries = []\n",
        "plan_id = \"\"\nentries = []\n",
        "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\npayload = {}\npredecessor = { existing = { origin = \"11111111-1111-4111-8111-111111111111\", position = 42 } }\n",
    ];
    for text in toml_cases {
        let error = proposal::decode(text, Format::Toml).unwrap_err();
        assert!(error.line.is_some(), "no location for {text}: {error}");
    }
    let error = proposal::decode(toml_cases[0], Format::Toml).unwrap_err();
    assert_eq!(error.line, Some(5));
    // A file that still carries the removed version field fails as unknown.
    for (text, format) in [
        ("format_version = 1\nentries = []\n", Format::Toml),
        (r#"{"format_version":1,"entries":[]}"#, Format::Json),
    ] {
        let error = proposal::decode(text, format).unwrap_err();
        assert_eq!(error.line, Some(1), "{text}");
        assert!(
            error.message.contains("unknown field `format_version`"),
            "{error}"
        );
    }
    // Plain timestamp-like and digit strings remain text.
    let text = "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\npayload = { at = \"2026-09-30T17:00:00Z\", n = \"42\" }\n";
    let decoded = proposal::decode(text, Format::Toml).unwrap();
    let proposal::ProposalEntry::Node { payload, .. } = &decoded.entries[0] else {
        panic!("expected a node")
    };
    assert_eq!(payload["at"], Payload::Text("2026-09-30T17:00:00Z".into()));
    assert_eq!(payload["n"], Payload::Text("42".into()));
    let error = proposal::decode("{\n \"entries\": [}", Format::Json).unwrap_err();
    assert_eq!(error.line, Some(2));
    for text in [
        r#"{"entries":[{"name":"a","kind":"node","ty_name":"T","payload":{"n":18446744073709551615}}]}"#,
        r#"{"entries":[{"name":"a","kind":"node","ty_name":"T","payload":{"n":null}}]}"#,
        r#"{"entries":[{"name":"a","kind":"node","ty_name":"T","payload":{"n":1,"n":2}}]}"#,
        r#"{"plan_id":null,"entries":[]}"#,
        r#"{"entries":[{"name":"a","kind":"node","ty_name":"T","payload":{},"predecessor":null}]}"#,
    ] {
        assert!(
            proposal::decode(text, Format::Json).is_err(),
            "accepted {text}"
        );
    }
}

#[test]
fn native_toml_datetimes_and_negative_zero_are_rejected_before_serde() {
    let entry = |payload: &str| {
        format!(
            "[[entries]]\nname = \"a\"\nkind = \"node\"\nty_name = \"T\"\npayload = {payload}\n"
        )
    };
    let datetime = "native TOML";
    let zero = "-0 is not";
    for (payload, path, token, message) in [
        // The whole payload, a record value, and a timestamp value.
        (
            "2026-09-30T17:00:00Z",
            "entries[0].payload",
            "2026",
            datetime,
        ),
        (
            "{ d = { kind = \"record\", value = 2026-09-30T17:00:00Z } }",
            "entries[0].payload.d.value",
            "2026",
            datetime,
        ),
        (
            "{ t = { kind = \"timestamp\", value = 2026-09-30T17:00:00Z } }",
            "entries[0].payload.t.value",
            "2026",
            datetime,
        ),
        // Local dates and times, and datetimes nested in lists.
        (
            "{ d = 2026-09-30 }",
            "entries[0].payload.d",
            "2026",
            datetime,
        ),
        (
            "{ l = [\"x\", [17:00:00]] }",
            "entries[0].payload.l[1][0]",
            "17",
            datetime,
        ),
        ("{ n = -0 }", "entries[0].payload.n", "-0", zero),
        (
            "{ n = { kind = \"record\", value = { z = -0 } } }",
            "entries[0].payload.n.value.z",
            "-0",
            zero,
        ),
    ] {
        let error = proposal::decode(&entry(payload), Format::Toml).unwrap_err();
        assert_eq!(error.path.as_deref(), Some(path), "{payload}: {error}");
        let column = "payload = ".len() + payload.find(token).unwrap() + 1;
        assert_eq!(
            (error.line, error.column),
            (Some(5), Some(column)),
            "{payload}"
        );
        assert!(error.message.starts_with(message), "{error}");
    }
    // Zero itself and quoted datetime text are ordinary values.
    let decoded = proposal::decode(
        &entry("{ n = 0, m = -1, t = \"2026-09-30T17:00:00Z\" }"),
        Format::Toml,
    )
    .unwrap();
    let proposal::ProposalEntry::Node { payload, .. } = &decoded.entries[0] else {
        panic!("expected a node")
    };
    assert_eq!(payload["n"], Payload::Integer(0));
    assert_eq!(payload["m"], Payload::Integer(-1));
    assert_eq!(payload["t"], Payload::Text("2026-09-30T17:00:00Z".into()));
    // JSON rejects `-0` too; serde_json reads it as a float.
    assert!(
        proposal::decode(
            r#"{"entries":[{"name":"a","kind":"node","ty_name":"T","payload":{"n":-0}}]}"#,
            Format::Json
        )
        .is_err()
    );
}

#[test]
fn json_decode_errors_carry_a_path_and_position() {
    let text = "{\"entries\": [\n  {\"name\": \"a\", \"kind\": \"node\", \"ty_name\": \"T\",\n   \"payload\": {\"n\": 1, \"n\": 2}}]}";
    let error = proposal::decode(text, Format::Json).unwrap_err();
    assert_eq!(error.path.as_deref(), Some("entries[0].payload"));
    assert_eq!(error.line, Some(3));
    assert!(error.message.contains("duplicate field `n`"), "{error}");
    let error = proposal::decode("{\"entries\": []} []", Format::Json).unwrap_err();
    assert_eq!(error.path, None);
    assert_eq!((error.line, error.column), (Some(1), Some(17)));
    assert_eq!(
        AppError::MalformedProposal(error).to_string(),
        "malformed JSON proposal: trailing characters at line 1 column 17"
    );
}

#[test]
fn lowering_resolves_names_and_rejects_invalid_references_with_locations() {
    use lifecore::transaction::Reference;
    let scratch = Scratch::new();
    let ids = populated(&scratch);
    let snapshot = storage::read(&scratch.state()).unwrap();
    let base = snapshot.graph.view();
    let lowered = proposal(json!([
        {"name": "a2", "kind": "node", "ty_name": "Task", "predecessor": existing(ids[1]),
            "intent": "revise", "payload": {"title": "A2"}},
        {"name": "link", "kind": "edge", "ty_name": "DependsOn",
            "from": {"added": "a2"}, "to": existing(ids[2])},
    ]))
    .lower(base)
    .unwrap();
    let EntryChange::Node(node) = &lowered[0] else {
        panic!("expected a node")
    };
    assert_eq!(node.predecessor, Some(Reference::Existing(ids[1])));
    assert_eq!(node.intent, Some(RevisionIntent::Revise));
    let EntryChange::Edge(edge) = &lowered[1] else {
        panic!("expected an edge")
    };
    assert_eq!(
        (edge.from, edge.to),
        (Reference::Added(0), Reference::Existing(ids[2]))
    );
    // A suggestion carries the same `Vec<EntryChange>`, so acknowledgment can
    // compare it with a lowered proposal by plain equality.
    assert_eq!(
        lowered,
        vec![
            EntryChange::Node(NewNode {
                ty_name: "Task".into(),
                payload: Body::from([("title".into(), "A2".into())]),
                predecessor: Some(Reference::Existing(ids[1])),
                intent: Some(RevisionIntent::Revise),
            }),
            EntryChange::Edge(NewEdge {
                from: Reference::Added(0),
                ty_name: "DependsOn".into(),
                to: Reference::Existing(ids[2]),
            }),
        ]
    );
    let node = |name: &str, predecessor: Option<Value>| {
        let mut entry = json!({"name": name, "kind": "node", "ty_name": "Task", "payload": {}});
        if let Some(predecessor) = predecessor {
            entry["predecessor"] = predecessor;
            entry["intent"] = "branch".into();
        }
        entry
    };
    let edge = |from: Value, to: Value| json!({"name": "e", "kind": "edge", "ty_name": "DependsOn", "from": from, "to": to});
    let mut missing = serde_json::to_value(ids[1]).unwrap();
    missing["position"] = "999".into();
    let cases = [
        (json!([node("1bad", None)]), 0, "name"),
        (json!([node("ok", None), node("ok", None)]), 1, "name"),
        (
            json!([
                node("x", Some(json!({"added": "later"}))),
                node("later", None)
            ]),
            0,
            "predecessor",
        ),
        (
            json!([
                node("n", None),
                node("m", None),
                edge(json!({"added": "n"}), json!({"added": "m"})),
                edge(json!({"added": "e"}), json!({"added": "n"}))
            ]),
            3,
            "name",
        ),
        (
            json!([
                node("n", None),
                edge(json!({"added": "n"}), json!({"existing": missing}))
            ]),
            1,
            "to",
        ),
        (json!([node("n", Some(existing(ids[3])))]), 0, "predecessor"),
    ];
    for (entries, operation, field) in cases {
        let (location, _) = proposal(entries.clone()).lower(base).unwrap_err();
        assert_eq!(location.operation, operation, "{entries}");
        assert_eq!(location.field.as_deref(), Some(field), "{entries}");
    }
    let (location, error) = proposal(json!([
        node("n", None),
        node("m", None),
        {"name": "e", "kind": "edge", "ty_name": "DependsOn", "from": {"added": "n"}, "to": {"added": "m"}},
        node("child", Some(json!({"added": "e"}))),
    ]))
    .lower(base)
    .unwrap_err();
    assert_eq!((location.operation, location.name.as_str()), (3, "child"));
    assert!(matches!(
        error,
        proposal::LoweringError::EdgeReference { .. }
    ));
}

#[test]
fn state_paths_use_the_variable_or_cwd_without_fallback() {
    let cwd = PathBuf::from("/work/dir");
    assert_eq!(
        StatePath::resolve_from(None, &cwd).unwrap().path(),
        cwd.join("LifeCoreState.json")
    );
    assert_eq!(
        StatePath::resolve_from(Some("sub/db.json".into()), &cwd)
            .unwrap()
            .path(),
        cwd.join("sub/db.json")
    );
    let absolute = StatePath::resolve_from(Some("/elsewhere/db.json".into()), &cwd).unwrap();
    assert_eq!(absolute.path(), PathBuf::from("/elsewhere/db.json"));
    assert_eq!(
        absolute.lock_path(),
        PathBuf::from("/elsewhere/db.json.lock")
    );
    assert!(matches!(
        StatePath::resolve_from(Some("".into()), &cwd),
        Err(AppError::EmptyStatePath)
    ));
}

#[test]
fn init_supports_both_suites_and_never_clobbers() {
    let scratch = Scratch::new();
    let service = scratch.service();
    assert!(matches!(
        service.describe(NoParams {}),
        Err(AppError::DatabaseMissing { .. })
    ));
    assert!(matches!(
        service.apply(request(proposal(json!([])))),
        Err(AppError::DatabaseMissing { .. })
    ));
    // Ordinary operations never create the database or its lock file.
    assert!(!scratch.state().path().exists());
    assert!(!scratch.state().lock_path().exists());
    let initialized = service.init(&typecheck()).unwrap();
    assert_eq!(initialized.suite.0, typecheck());
    assert!(scratch.state().lock_path().exists());
    let before = scratch.bytes();
    assert!(matches!(
        service.init(&[]),
        Err(AppError::DatabaseExists { .. })
    ));
    assert_eq!(scratch.bytes(), before);
    let described = service.describe(NoParams {}).unwrap();
    assert_eq!(described.database, initialized.database);
    assert_eq!(described.graph_revision, initialized.graph_revision);
    assert_eq!(described.view.end_exclusive, 0);
    let registered = [DagCheckFactory::name(), TypeCheckFactory::name()];
    assert_eq!(described.registered, registered);

    let bare = Scratch::new();
    let initialized = bare.service().init(&[]).unwrap();
    assert!(initialized.suite.0.is_empty());
    // The host's factories, not the stored suite, name the registered ones.
    assert_eq!(
        bare.service().describe(NoParams {}).unwrap().registered,
        registered
    );
    assert!(matches!(
        bare.service().init(&[Config {
            name: "Missing".into(),
            configuration: "{}".into()
        }]),
        Err(AppError::DatabaseExists { .. })
    ));
    let other = Scratch::new();
    assert!(matches!(
        other.service().init(&[Config {
            name: TypeCheckFactory::name().into(),
            configuration: "not json".into()
        }]),
        Err(AppError::Analyzer(_))
    ));
    // A refused init leaves neither the database nor a lock file behind.
    assert!(!other.state().path().exists());
    assert!(!other.state().lock_path().exists());
    let copied = Scratch::new();
    fs::write(copied.state().path(), scratch.bytes()).unwrap();
    assert!(matches!(
        copied.service().init(&[]),
        Err(AppError::DatabaseExists { .. })
    ));
    assert!(!copied.state().lock_path().exists());
    let nested = self::service(StatePath::new(other.0.join("missing/LifeCoreState.json")));
    let error = nested.init(&[]).unwrap_err();
    assert!(
        matches!(error, AppError::DirectoryMissing { .. }),
        "{error}"
    );
    let object = error.to_object();
    assert_eq!(object.code, 1001);
    assert_eq!(object.data.unwrap()["kind"], "directory_missing");
    assert!(!other.0.join("missing").exists());
    // A directory or garbage at the selected path is an error, not a fallback.
    fs::write(other.state().path(), "not a snapshot").unwrap();
    assert!(matches!(
        other.service().describe(NoParams {}),
        Err(AppError::DatabaseUnreadable { .. })
    ));
}

#[test]
fn apply_publishes_revisions_that_reload_and_lookup_through_rpc() {
    let scratch = Scratch::new();
    let ids = populated(&scratch);
    let service = scratch.service();
    let line =
        r#"{"jsonrpc":"2.0","id":"list","method":"entries.list","params":{"ty_name":"Task"}}"#;
    let response = serde_json::to_value(rpc::handle(&service, line)).unwrap();
    assert_eq!(response["id"], "list");
    let entries = response["result"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["id"], serde_json::to_value(ids[1]).unwrap());
    assert_eq!(entries[0]["payload"]["title"], "A");
    assert_eq!(entries[0]["current"], true);
    assert_eq!(response["result"]["view"]["end_exclusive"], "4");
    // Revise A through the exact identity found above.
    let a: EntryId = serde_json::from_value(entries[0]["id"].clone()).unwrap();
    let before = service.describe(NoParams {}).unwrap();
    let applied = service
        .apply(request(proposal(json!([
            {"name": "a2", "kind": "node", "ty_name": "Task", "predecessor": existing(a),
                "intent": "revise", "payload": {"title": "A", "status": "done", "notes": "n"}},
            {"name": "a2_b", "kind": "edge", "ty_name": "DependsOn",
                "from": {"added": "a2"}, "to": existing(ids[2])},
        ]))))
        .unwrap();
    assert_ne!(applied.graph_revision, before.graph_revision);
    assert_eq!(applied.report.base_revision, Some(before.graph_revision));
    assert_eq!(applied.report.base, before.view);
    let names = applied
        .report
        .operations
        .iter()
        .map(|o| o.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["a2", "a2_b"]);
    // The reported mapping is the actual stored identity after reload.
    let reloaded = service
        .get_entry(EntryGetRequest {
            id: applied.report.operations[0].entry,
            as_of: None,
        })
        .unwrap();
    let EntryRecord {
        entry:
            Entry::Node(Node {
                predecessor,
                payload,
                ..
            }),
        current,
    } = reloaded.entry
    else {
        panic!("expected a node")
    };
    assert_eq!(predecessor, Some(a));
    assert_eq!(payload["status"], Payload::Text("done".into()));
    assert!(current);
    let old = service
        .get_entry(EntryGetRequest { id: a, as_of: None })
        .unwrap();
    assert!(matches!(
        old.entry,
        EntryRecord {
            entry: Entry::Node(_),
            current: false
        }
    ));
    let historical = service
        .list_entries(EntryListRequest {
            currentness: Currentness::Historical,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        historical
            .entries
            .iter()
            .map(|record| record.entry.id())
            .collect::<Vec<_>>(),
        vec![ids[1], ids[3]]
    );
    // A standalone check of a further revision previews without storing.
    let stored = scratch.bytes();
    let preview = service
        .check(request(proposal(json!([
            {"name": "a3", "kind": "node", "ty_name": "Task",
                "predecessor": existing(applied.report.operations[0].entry),
                "intent": "revise", "payload": {"title": "A3", "notes": "n"}},
        ]))))
        .unwrap();
    assert!(preview.accepted);
    assert_eq!(preview.report.base_revision, Some(applied.graph_revision));
    assert_eq!(preview.report.operations.len(), 1);
    assert_eq!(scratch.bytes(), stored);
    assert!(matches!(
        service.get_entry(EntryGetRequest {
            id: preview.report.operations[0].entry,
            as_of: None,
        }),
        Err(AppError::EntryNotFound { .. })
    ));
}

#[test]
fn revision_intent_is_enforced_at_intermediate_prefixes() {
    let scratch = Scratch::new();
    let ids = populated(&scratch);
    let service = scratch.service();
    let stored = scratch.bytes();
    let revise_twice = |second: &str| {
        proposal(json!([
            {"name": "first", "kind": "node", "ty_name": "Task", "predecessor": existing(ids[1]),
                "intent": "revise", "payload": {"title": "1", "notes": "n"}},
            {"name": "second", "kind": "node", "ty_name": "Task", "predecessor": existing(ids[1]),
                "intent": second, "payload": {"title": "2", "notes": "n"}},
        ]))
    };
    for result in [
        service
            .check(request(revise_twice("revise")))
            .map(|c| c.report),
        service
            .apply(request(revise_twice("revise")))
            .map(|a| a.report),
    ] {
        let Err(AppError::InvalidProposal {
            location: Some(location),
            ..
        }) = result
        else {
            panic!("the second revision must be rejected")
        };
        assert_eq!(location.operation, 1);
        assert_eq!(location.name, "second");
        assert_eq!(location.field.as_deref(), Some("intent"));
    }
    assert_eq!(scratch.bytes(), stored);
    for (entry, message) in [
        (
            json!({"name": "r", "kind": "node", "ty_name": "Task", "intent": "revise", "payload": {"title": "r", "notes": "n"}}),
            "root",
        ),
        (
            json!({"name": "r", "kind": "node", "ty_name": "Task", "predecessor": existing(ids[1]), "payload": {"title": "r", "notes": "n"}}),
            "without",
        ),
    ] {
        let Err(AppError::InvalidProposal { message: text, .. }) =
            service.check(request(proposal(json!([entry]))))
        else {
            panic!("inconsistent intent must be rejected")
        };
        assert!(text.contains(message), "{text}");
    }
    // A split through `branch` is valid at the same intermediate prefix.
    let split = service.apply(request(revise_twice("branch"))).unwrap();
    assert_eq!(split.report.operations.len(), 2);
}

#[test]
fn plan_ids_detect_stale_foreign_divergent_and_suite_only_bases() {
    let scratch = Scratch::new();
    populated(&scratch);
    let service = scratch.service();
    let with = |plan_id: PlanId, entries: Value| {
        let mut proposal = proposal(entries);
        proposal.plan_id = Some(plan_id);
        request(proposal)
    };
    let described = service.describe(NoParams {}).unwrap();
    let first = service.mint_plan_id(NoParams {}).unwrap().plan_id;
    let second = service.mint_plan_id(NoParams {}).unwrap().plan_id;
    // Minting shares the base, distinguishes proposals, and changes nothing.
    assert_eq!(
        (first.database, first.graph_revision),
        (second.database, second.graph_revision)
    );
    assert_ne!(first.proposal, second.proposal);
    assert_eq!(first.graph_revision, described.graph_revision);
    assert_eq!(service.describe(NoParams {}).unwrap(), described);
    // A matching token checks; an empty apply is a no-op keeping the revision.
    service.check(with(first, json!([task("c", "C")]))).unwrap();
    let before = scratch.bytes();
    let noop = service.apply(with(first, json!([]))).unwrap();
    assert_eq!(noop.graph_revision, described.graph_revision);
    assert_eq!(scratch.bytes(), before);
    let foreign = PlanId {
        database: Uuid::new_v4(),
        ..first
    };
    assert!(matches!(
        service.apply(with(foreign, json!([task("c", "C")]))),
        Err(AppError::ForeignDatabase { .. })
    ));
    // Keep a copy of this state to build an equal-count divergent history.
    let copy = scratch.bytes();
    let applied = service.apply(with(first, json!([task("c", "C")]))).unwrap();
    let stored = scratch.bytes();
    for plan_id in [first, second] {
        assert!(matches!(
            service.check(with(plan_id, json!([task("d", "D")]))),
            Err(AppError::StaleBase { .. })
        ));
        assert!(matches!(
            service.apply(with(plan_id, json!([task("d", "D")]))),
            Err(AppError::StaleBase { .. })
        ));
    }
    assert_eq!(scratch.bytes(), stored);
    let current = service.mint_plan_id(NoParams {}).unwrap().plan_id;
    assert_eq!(current.graph_revision, applied.graph_revision);
    fs::write(scratch.state().path(), &copy).unwrap();
    // Without its encouraged notes, the divergent task draws one warning.
    let divergent = service
        .apply(request(proposal(json!([
            {"name": "x", "kind": "node", "ty_name": "Task", "payload": {"title": "X"}}
        ]))))
        .unwrap();
    assert_eq!(
        divergent.report.analysis.view.end_exclusive,
        applied.report.analysis.view.end_exclusive
    );
    // The report carries the analysis that certified the stored state.
    let stored_graph = storage::read(&scratch.state()).unwrap().graph;
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory))
        .unwrap();
    let analysis = &divergent.report.analysis;
    assert_eq!(*analysis, runner.run(&stored_graph).unwrap());
    assert_eq!(analysis.view, stored_graph.view().id());
    let x = divergent.report.operations[0].entry;
    let warnings = analysis.warnings().collect::<Vec<_>>();
    assert_eq!(analysis.observations().count(), 1);
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].evaluated_view,
        EvaluatedView::Prefixes {
            first: analysis.view,
            last: analysis.view,
        }
    );
    assert_eq!(warnings[0].diagnostic.focus, Some(x));
    assert_eq!(
        serde_json::from_str::<Value>(&warnings[0].key.analyzer_certificate.0).unwrap(),
        json!({"Field": {"subject": x,
            "issue": {"path": ["notes"], "kind": "MissingEncouraged"}}})
    );
    assert_eq!(applied.report.analysis.observations().count(), 0);
    assert!(matches!(
        service.apply(with(current, json!([task("d", "D")]))),
        Err(AppError::StaleBase { .. })
    ));
    // A suite-only change also rotates the revision.
    let before_suite = service.mint_plan_id(NoParams {}).unwrap().plan_id;
    let added = service
        .add_analyzer(AddAnalyzerRequest {
            name: DagCheckFactory::name().into(),
            configuration: r#"{"on_cycle":"warning"}"#.into(),
            evaluation_time: None,
            now: false,
        })
        .unwrap();
    assert_ne!(added.graph_revision, before_suite.graph_revision);
    assert_eq!(added.suite.0.len(), 2);
    assert!(matches!(
        service.apply(with(before_suite, json!([task("d", "D")]))),
        Err(AppError::StaleBase { .. })
    ));
}

// A hand-written declaration in the FILES §2.6 form. `fields` is a record
// payload, so it keeps its envelope; scalar and list shapes need none.
const DECLARATION_TOML: &str = r#"
[[entries]]
name = "task_type"
kind = "node"
ty_name = "Type"

[entries.payload]
name = "Task"
outgoing = ["DependsOn"]
incoming = ["DependsOn"]

[entries.payload.fields]
kind = "record"

[entries.payload.fields.value]
title = ["mandatory", "text"]
tags = ["optional", "list", "text"]
grid = ["optional", "list", "list", "integer"]
details = ["optional", "record", { kind = "record", value = { owner = ["mandatory", "text"] } }]
"#;

#[test]
fn a_hand_written_toml_declaration_applies_and_typechecks_nodes() {
    let scratch = Scratch::new();
    let service = scratch.service();
    service.init(&typecheck()).unwrap();
    let declaration = proposal::decode(DECLARATION_TOML, Format::Toml).unwrap();
    let proposal::ProposalEntry::Node { payload, .. } = &declaration.entries[0] else {
        panic!("expected a node")
    };
    let owner = ValueType::Record {
        fields: [(
            "owner".into(),
            FieldDecl {
                status: FieldStatus::Mandatory,
                shape: ValueType::Text,
            },
        )]
        .into(),
    };
    let list = |element| ValueType::List {
        element: Box::new(element),
    };
    let expected = Type::new(
        "Task",
        &[
            ("title", (ValueType::Text, FieldStatus::Mandatory)),
            ("tags", (list(ValueType::Text), FieldStatus::Optional)),
            (
                "grid",
                (list(list(ValueType::Integer)), FieldStatus::Optional),
            ),
            ("details", (owner, FieldStatus::Optional)),
        ],
        &["DependsOn"],
        &["DependsOn"],
        &[],
    );
    assert_eq!(Type::from_body(payload).unwrap(), expected);
    service.apply(request(declaration)).unwrap();
    let task = |payload: Value| {
        request(proposal(
            json!([{"name": "t", "kind": "node", "ty_name": "Task", "payload": payload}]),
        ))
    };
    let passing = service
        .check(task(
            json!({"title": "T", "tags": ["a"], "grid": [[1, 2], []],
            "details": {"kind": "record", "value": {"owner": "Teto"}}}),
        ))
        .unwrap();
    assert!(passing.accepted);
    assert_eq!(passing.report.analysis.errors().count(), 0);
    assert_eq!(passing.report.analysis.warnings().count(), 0);
    let failing = service
        .check(task(json!({"title": "T", "grid": [["x"]],
            "details": {"kind": "record", "value": {}}})))
        .unwrap();
    assert!(!failing.accepted);
    let entry = failing.report.operations[0].entry;
    let mut findings = failing
        .report
        .analysis
        .errors()
        .map(|o| serde_json::from_str::<Value>(&o.key.analyzer_certificate.0).unwrap())
        .collect::<Vec<_>>();
    findings.sort_by_key(|finding| finding.to_string());
    assert_eq!(
        findings,
        [
            json!({"Field": {"subject": entry, "issue": {
                "path": ["details", "owner"], "kind": "MissingMandatory"}}}),
            json!({"Field": {"subject": entry, "issue": {
                "path": ["grid", 0, 0],
                "kind": {"TypeMismatch": {"expected": "integer", "actual": "text"}}}}}),
        ]
    );
}

#[test]
fn rejected_proposals_report_full_context_and_store_nothing() {
    let scratch = Scratch::new();
    let ids = populated(&scratch);
    let service = scratch.service();
    let stored = scratch.bytes();
    // The second task lacks its mandatory title; the first lacks encouraged notes.
    let rejected = || {
        request(proposal(json!([
            {"name": "ok", "kind": "node", "ty_name": "Task", "payload": {"title": "ok"}},
            {"name": "bad", "kind": "node", "ty_name": "Task", "payload": {"notes": "n"}},
            {"name": "link", "kind": "edge", "ty_name": "DependsOn",
                "from": {"added": "ok"}, "to": existing(ids[2])},
        ])))
    };
    // A preview reports the rejection as a result; apply refuses it as an
    // error carrying the same complete report.
    let checked = service.check(rejected()).unwrap();
    assert!(!checked.accepted);
    let Err(AppError::Rejected(applied)) = service.apply(rejected()) else {
        panic!("an inconsistent proposal must be rejected")
    };
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "proposal.check",
        "params": {"proposal": serde_json::to_value(&rejected().proposal).unwrap()}})
    .to_string();
    let response = serde_json::to_value(rpc::handle(&service, &line)).unwrap();
    assert_eq!(response["result"]["accepted"], false, "{response}");
    assert_eq!(response["result"]["report"]["operations"][1]["name"], "bad");
    assert_eq!(
        response["result"]["report"]["analysis"]["implementations"],
        serde_json::to_value(&checked.report.analysis.implementations).unwrap()
    );
    for report in [Box::new(checked.report), applied] {
        assert_eq!(report.base.end_exclusive, 4);
        assert_eq!(report.operations.len(), 3);
        let analysis = &report.analysis;
        assert_eq!(analysis.view.end_exclusive, 7);
        assert_eq!(analysis.implementations[0].name, "Type_Check");
        let errors = analysis.errors().collect::<Vec<_>>();
        assert!(!errors.is_empty());
        assert!(
            errors
                .iter()
                .all(|o| o.diagnostic.subjects.contains(&report.operations[1].entry))
        );
        assert!(
            analysis
                .warnings()
                .any(|o| o.diagnostic.subjects.contains(&report.operations[0].entry))
        );
        let object = AppError::Rejected(report.clone()).to_object();
        assert_eq!(object.data.as_ref().unwrap()["kind"], "rejected");
        assert_eq!(
            object.data.as_ref().unwrap()["report"]["operations"][1]["name"],
            "bad"
        );
    }
    assert_eq!(scratch.bytes(), stored);
}

#[test]
fn add_analyzer_replays_history_and_rejects_an_old_cycle() {
    let scratch = Scratch::new();
    let ids = populated(&scratch);
    let service = scratch.service();
    // Close a cycle, then revise B so the current graph is acyclic again.
    service
        .apply(request(proposal(json!([
            {"name": "b_a", "kind": "edge", "ty_name": "DependsOn",
                "from": existing(ids[2]), "to": existing(ids[1])},
            {"name": "b2", "kind": "node", "ty_name": "Task", "predecessor": existing(ids[2]),
                "intent": "revise", "payload": {"title": "B2", "notes": "n"}},
        ]))))
        .unwrap();
    let stored = scratch.bytes();
    let add = |configuration: &str| {
        service.add_analyzer(AddAnalyzerRequest {
            name: DagCheckFactory::name().into(),
            configuration: configuration.into(),
            evaluation_time: None,
            now: false,
        })
    };
    let Err(AppError::Rejected(report)) = add(r#"{"on_cycle":"inconsistency"}"#) else {
        panic!("a historical cycle must reject the analyzer")
    };
    assert!(report.operations.is_empty());
    let boundaries = report
        .analysis
        .errors()
        .map(|o| match o.evaluated_view {
        EvaluatedView::Prefixes { first, last } => (first.end_exclusive, last.end_exclusive),
        EvaluatedView::Initialization => panic!("cycles have a prefix"),
        })
        .collect::<Vec<_>>();
    // The revision appended after the closing edge ends the cycle.
    assert_eq!(boundaries, vec![(5, 5)]);
    assert_eq!(scratch.bytes(), stored);
    assert!(matches!(add("not json"), Err(AppError::Analyzer(_))));
    assert!(matches!(
        service.add_analyzer(AddAnalyzerRequest {
            name: "Missing".into(),
            configuration: "{}".into(),
            evaluation_time: None,
            now: false,
        }),
        Err(AppError::Analyzer(_))
    ));
    assert_eq!(scratch.bytes(), stored);
    let added = add(r#"{"on_cycle":"warning"}"#).unwrap();
    assert_eq!(added.suite.0[1].name, "DAG_Check");
    assert!(
        added
            .report
            .analysis
            .warnings()
            .any(|o| o.key.kind == MessageType::Warning)
    );
}

#[test]
fn competing_writers_get_a_busy_error_and_no_mutation() {
    let scratch = Scratch::new();
    populated(&scratch);
    let service = scratch.service().with_busy_retry(SHORT_RETRY);
    let stored = scratch.bytes();
    let holder = storage::lock(&scratch.state(), Duration::ZERO).unwrap();
    assert!(matches!(
        service.apply(request(proposal(json!([task("c", "C")])))),
        Err(AppError::Busy { .. })
    ));
    // An existing database is refused before the lock is consulted.
    assert!(matches!(
        service.init(&[]),
        Err(AppError::DatabaseExists { .. })
    ));
    // Readers and standalone checks need no lock.
    assert!(
        service
            .check(request(proposal(json!([task("c", "C")]))))
            .unwrap()
            .accepted
    );
    assert_eq!(scratch.bytes(), stored);
    drop(holder);
    // A new database is initialized under the same exclusion.
    let fresh = Scratch::new();
    let fresh_holder = storage::lock(&fresh.state(), Duration::ZERO).unwrap();
    assert!(matches!(
        fresh.service().with_busy_retry(SHORT_RETRY).init(&[]),
        Err(AppError::Busy { .. })
    ));
    assert!(!fresh.state().path().exists());
    drop(fresh_holder);
    fresh.service().init(&[]).unwrap();
}

// The lock holder publishes and releases while other writers wait. Each
// waiting writer must reload after acquiring the lock, never check a snapshot
// read before it.
#[test]
fn a_waiting_writer_reloads_the_state_published_under_the_lock() {
    let scratch = Scratch::new();
    populated(&scratch);
    let state = scratch.state();
    let old = scratch.service().mint_plan_id(NoParams {}).unwrap().plan_id;
    let lock = storage::lock(&state, Duration::ZERO).unwrap();
    let waiting = |name: &'static str, plan_id: Option<PlanId>| {
        let state = state.clone();
        std::thread::spawn(move || {
            let mut proposal = proposal(json!([task(name, name)]));
            proposal.plan_id = plan_id;
            service(state)
                .with_busy_retry(Duration::from_secs(10))
                .apply(request(proposal))
        })
    };
    let stale = waiting("stale", Some(old));
    let tokenless = waiting("tokenless", None);
    // Give both writers time to start waiting on the held lock.
    std::thread::sleep(Duration::from_millis(200));
    let mut locked = lock.read().unwrap();
    let snapshot = locked.snapshot_mut();
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory))
        .unwrap();
    let held = Body::from([
        ("title".into(), "Held".into()),
        ("notes".into(), "n".into()),
    ]);
    transaction::prepare(
        &snapshot.graph,
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Task".into(),
            payload: held,
            predecessor: None,
            intent: None,
        }))],
        &mut runner,
    )
    .unwrap()
    .accepted()
    .unwrap()
    .commit(&mut snapshot.graph)
    .unwrap();
    snapshot.graph_revision = Uuid::new_v4();
    let held_revision = snapshot.graph_revision;
    let published = locked.publish(storage::sync_directory);
    assert!(matches!(published, Ok(Publication::Complete)));
    drop(locked);
    let Err(AppError::StaleBase { expected, actual }) = stale.join().unwrap() else {
        panic!("the old plan ID must be stale after the held publication")
    };
    assert_eq!(expected, old.graph_revision);
    assert_ne!(actual, old.graph_revision);
    // The tokenless writer checked the held publication, not its own earlier
    // view of the file.
    let applied = tokenless.join().unwrap().unwrap();
    assert_eq!(applied.report.base_revision, Some(held_revision));
    assert_eq!(applied.report.base.end_exclusive, 5);
    let listed = scratch
        .service()
        .list_entries(EntryListRequest {
            text: Some("Held".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(listed.entries.len(), 1);
    assert_eq!(listed.graph_revision, applied.graph_revision);
}

#[cfg(unix)]
#[test]
fn failed_publication_before_replacement_preserves_state() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    populated(&scratch);
    let service = scratch.service();
    let stored = scratch.bytes();
    fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o555)).unwrap();
    // Privileged users bypass directory permissions; nothing to observe then.
    // `publication_onto_a_directory_fails_before_replacement` covers them.
    let enforced = fs::write(scratch.0.join("probe"), "").is_err();
    let result = service.apply(request(proposal(json!([task("c", "C")]))));
    fs::set_permissions(&scratch.0, fs::Permissions::from_mode(0o755)).unwrap();
    if enforced {
        assert!(matches!(result, Err(AppError::PublicationFailed { .. })));
        assert_eq!(scratch.bytes(), stored);
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 2);
    }
}

// Replacement fails even for privileged users when the target is a
// non-empty directory, so this runs as root too. The snapshot is read under
// the lock first; the path then turns into a directory before publication.
#[test]
fn publication_onto_a_directory_fails_before_replacement() {
    let scratch = Scratch::new();
    populated(&scratch);
    let state = scratch.state();
    let locked = storage::lock(&state, Duration::ZERO)
        .unwrap()
        .read()
        .unwrap();
    fs::remove_file(state.path()).unwrap();
    fs::create_dir(state.path()).unwrap();
    fs::write(state.path().join("inside"), "kept").unwrap();
    let result = locked.publish(storage::sync_directory);
    assert!(matches!(result, Err(AppError::PublicationFailed { .. })));
    assert_eq!(fs::read(state.path().join("inside")).unwrap(), b"kept");
    assert_eq!(fs::read_dir(state.path()).unwrap().count(), 1);
    assert_eq!(temporary_files(&scratch), 0);
}

#[test]
fn publish_new_refuses_a_path_that_appeared_after_the_check() {
    let scratch = Scratch::new();
    let target = StatePath::new(scratch.0.join("taken.json"));
    fs::write(target.path(), "someone else's file").unwrap();
    let lock = storage::lock(&target, Duration::ZERO).unwrap();
    let snapshot = Snapshot::new(LifeCoreList::default());
    let result = lock.publish_new(&snapshot, storage::sync_directory);
    assert!(matches!(result, Err(AppError::DatabaseExists { .. })));
    assert_eq!(fs::read(target.path()).unwrap(), b"someone else's file");
    assert_eq!(temporary_files(&scratch), 0);
}

fn failed_sync(_: &Path) -> io::Result<()> {
    Err(io::Error::other("injected synchronization failure"))
}

#[test]
fn synchronization_failure_after_replacement_is_uncertain_and_carries_the_outcome() {
    let scratch = Scratch::new();
    let error = scratch
        .service()
        .with_sync_directory(failed_sync)
        .init(&typecheck())
        .unwrap_err();
    let AppError::PublicationUncertain { outcome, .. } = &error else {
        panic!("expected an uncertain publication, got {error}")
    };
    let Published::Initialized(initialized) = outcome.as_ref() else {
        panic!("expected the initialization outcome")
    };
    // The initialized database is already visible.
    let described = scratch.service().describe(NoParams {}).unwrap();
    assert_eq!(described.database, initialized.database);
    assert_eq!(described.graph_revision, initialized.graph_revision);
    assert_eq!(error.to_object().code, 1008);

    let scratch = Scratch::new();
    populated(&scratch);
    let failing = scratch.service().with_sync_directory(failed_sync);
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "proposal.apply",
        "params": {"proposal": {"entries": [task("c", "C")]}}})
    .to_string();
    let response = serde_json::to_value(rpc::handle(&failing, &line)).unwrap();
    let error = &response["error"];
    assert_eq!(error["code"], 1008, "{response}");
    assert_eq!(error["data"]["kind"], "publication_uncertain");
    let outcome = &error["data"]["outcome"];
    assert_eq!(outcome["kind"], "applied");
    assert_eq!(outcome["report"]["operations"][0]["name"], "c");
    // The carried outcome names the new revision and the actual entry, both
    // visible in the stored state.
    let described = scratch.service().describe(NoParams {}).unwrap();
    assert_eq!(
        outcome["graph_revision"],
        json!(described.graph_revision.to_string())
    );
    let id: EntryId =
        serde_json::from_value(outcome["report"]["operations"][0]["entry"].clone()).unwrap();
    let Entry::Node(node) = scratch
        .service()
        .get_entry(EntryGetRequest { id, as_of: None })
        .unwrap()
        .entry
        .entry
    else {
        panic!("expected the applied node")
    };
    assert_eq!(node.payload["title"], Payload::Text("C".into()));
}

#[cfg(unix)]
#[test]
fn publication_keeps_the_snapshot_file_mode() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    populated(&scratch);
    let mode = |scratch: &Scratch| {
        fs::metadata(scratch.state().path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    for (expected, name) in [(0o600, "c"), (0o640, "d")] {
        fs::set_permissions(scratch.state().path(), fs::Permissions::from_mode(expected)).unwrap();
        scratch
            .service()
            .apply(request(proposal(json!([task(name, name)]))))
            .unwrap();
        assert_eq!(mode(&scratch), expected);
    }
}

#[test]
fn bare_state_file_names_publish_in_the_current_directory() {
    let bare = StatePath::new(PathBuf::from(DEFAULT_STATE_FILE));
    assert_eq!(bare.directory(), Path::new("."));
    storage::sync_directory(bare.directory()).unwrap();
    assert_eq!(
        StatePath::new(PathBuf::from("sub/db.json")).directory(),
        Path::new("sub")
    );
    assert_eq!(
        StatePath::new(PathBuf::from("/db.json")).directory(),
        Path::new("/")
    );
}

#[test]
fn acknowledgments_are_rejected_until_verification_exists() {
    let scratch = Scratch::new();
    populated(&scratch);
    let service = scratch.service();
    let stored = scratch.bytes();
    let described = service.describe(NoParams {}).unwrap();
    let mut acknowledging = proposal(json!([task("c", "C")]));
    acknowledging.acknowledges = Some(lifecore::analysis::FindingKey {
        database: described.database,
        analyzer: AnalyzerIdentity {
            name: "Type_Check".into(),
            version: "0.1.0".parse().unwrap(),
            configuration: "{}".into(),
        },
        kind: MessageType::Suggestion,
        analyzer_certificate: lifecore::analysis::AnalyzerCertificate("x".into()),
    });
    assert!(matches!(
        service.check(request(acknowledging.clone())),
        Err(AppError::AcknowledgmentUnsupported)
    ));
    assert!(matches!(
        service.apply(request(acknowledging)),
        Err(AppError::AcknowledgmentUnsupported)
    ));
    assert_eq!(scratch.bytes(), stored);
}

#[test]
fn evaluation_time_is_explicit_or_captured_once_by_the_host() {
    let scratch = Scratch::new();
    populated(&scratch);
    let service = scratch.service();
    let absent = service.check(request(proposal(json!([])))).unwrap();
    assert_eq!(absent.report.analysis.inputs.evaluation_time, None);
    let at: OffsetTimestamp = "2026-01-02T04:04:05.000000006+01:00".parse().unwrap();
    let explicit = service
        .check(ProposalRequest {
            evaluation_time: Some(at),
            ..request(proposal(json!([])))
        })
        .unwrap();
    assert_eq!(explicit.report.analysis.inputs.evaluation_time, Some(at));
    let now = service
        .check(ProposalRequest {
            now: true,
            ..request(proposal(json!([])))
        })
        .unwrap();
    assert!(now.report.analysis.inputs.evaluation_time.is_some());
    assert!(matches!(
        service.check(ProposalRequest {
            evaluation_time: Some(at),
            now: true,
            ..request(proposal(json!([])))
        }),
        Err(AppError::InvalidParams { .. })
    ));
}

#[test]
fn rpc_envelope_rejects_invalid_calls_without_mutation() {
    let scratch = Scratch::new();
    populated(&scratch);
    let service = scratch.service();
    let stored = scratch.bytes();
    let call = |line: &str| serde_json::to_value(rpc::handle(&service, line)).unwrap();
    let apply = json!({"proposal": {"entries": [task("c", "C")]}});
    for (line, code, id) in [
        ("".to_string(), -32700, Value::Null),
        ("{not json".to_string(), -32700, Value::Null),
        (format!("[{}]", json!({"jsonrpc": "2.0", "id": 1, "method": "proposal.apply", "params": apply})), -32600, Value::Null),
        (json!({"jsonrpc": "2.0", "method": "proposal.apply", "params": apply}).to_string(), -32600, Value::Null),
        (json!({"jsonrpc": "2.0", "id": null, "method": "proposal.apply", "params": apply}).to_string(), -32600, Value::Null),
        (json!({"jsonrpc": "1.0", "id": 7, "method": "proposal.apply", "params": apply}).to_string(), -32600, json!(7)),
        (json!({"jsonrpc": "2.0", "id": 7, "method": "proposal.apply", "params": [apply]}).to_string(), -32600, json!(7)),
        (json!({"jsonrpc": "2.0", "id": 7, "method": "proposal.apply", "params": apply, "extra": 1}).to_string(), -32600, json!(7)),
        (json!({"jsonrpc": "2.0", "id": "x", "method": "init", "params": {}}).to_string(), -32601, json!("x")),
        (json!({"jsonrpc": "2.0", "id": "x", "method": "proposal.apply", "params": {"proposal": {}}}).to_string(), 2001, json!("x")),
        (json!({"jsonrpc": "2.0", "id": "x", "method": "proposal.apply", "params": {}}).to_string(), -32602, json!("x")),
        (r#"{"jsonrpc":"2.0","id":1,"id":2,"method":"plan_id.mint"}"#.to_string(), -32600, Value::Null),
        (json!({"jsonrpc": "2.0", "id": "x", "method": "entries.get", "params": {"id": {"origin": Uuid::new_v4(), "position": 1}}}).to_string(), -32602, json!("x")),
    ] {
        let response = call(&line);
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], id, "{line}");
        assert_eq!(response["error"]["code"], code, "{line}");
        assert!(response.get("result").is_none());
    }
    assert_eq!(scratch.bytes(), stored);
    let response = call(
        &json!({"jsonrpc": "2.0", "id": 1, "method": "plan_id.mint", "params": {}}).to_string(),
    );
    assert!(
        response["result"]["plan_id"]
            .as_str()
            .unwrap()
            .starts_with("lc1:")
    );
    let response = call(
        &json!({"jsonrpc": "2.0", "id": 2, "method": "proposal.decode",
            "params": {"text": "[[entries]]\nname = \"n\"\nkind = \"node\"\nty_name = \"Task\"\npayload = { big = 9223372036854775807 }\n", "format": "toml"}})
        .to_string(),
    );
    assert_eq!(
        response["result"]["entries"][0]["payload"]["big"],
        json!({"kind": "integer", "value": "9223372036854775807"})
    );
    let response = call(
        &json!({"jsonrpc": "2.0", "id": 3, "method": "proposal.apply", "params": apply})
            .to_string(),
    );
    assert_eq!(response["result"]["report"]["operations"][0]["name"], "c");
    let response = call(
        &json!({"jsonrpc": "2.0", "id": 4, "method": "proposal.apply", "params": apply})
            .to_string(),
    );
    assert_eq!(response["error"]["code"], 2002);
    assert_eq!(response["error"]["data"]["kind"], "invalid_proposal");
    assert_eq!(response["error"]["data"]["location"]["name"], "c");
    assert_eq!(response["id"], 4);
}

#[test]
fn rpc_proposal_parameters_fail_like_change_files() {
    let scratch = Scratch::new();
    populated(&scratch);
    let service = scratch.service();
    let stored = scratch.bytes();
    let call = |method: &str, params: String| {
        let line = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#);
        serde_json::to_value(rpc::handle(&service, &line)).unwrap()
    };
    let bad = r#"{"entries":[{"name":"a","kind":"node","ty_name":"Task","payload":{"n":1.5}}]}"#;
    // Raw text keeps a repeated payload field visible to the decoder.
    let repeated = r#"{"entries":[{"name":"a","kind":"node","ty_name":"Task","payload":{"title":"x","title":"y"}}]}"#;
    let file_error = proposal::decode(bad, Format::Json).unwrap_err();
    assert_eq!(file_error.path.as_deref(), Some("entries[0].payload.n"));
    assert!(file_error.line.is_some());
    for (proposal, path) in [
        (bad, "entries[0].payload.n"),
        (repeated, "entries[0].payload"),
    ] {
        for (method, extra) in [
            ("proposal.check", ""),
            ("proposal.apply", ""),
            ("proposal.encode", r#","format":"toml""#),
        ] {
            let response = call(method, format!(r#"{{"proposal":{proposal}{extra}}}"#));
            let error = &response["error"];
            assert_eq!(error["code"], 2001, "{method} {response}");
            assert_eq!(error["data"]["kind"], "malformed_proposal");
            assert_eq!(error["data"]["decode"]["path"], path);
            assert_eq!(error["data"]["decode"]["format"], "json");
            // Positions would count from the member, so only the path is given.
            assert!(error["data"]["decode"].get("line").is_none());
        }
    }
    let response = call("proposal.check", format!(r#"{{"proposal":{bad}}}"#));
    assert_eq!(
        response["error"]["data"]["decode"]["message"],
        file_error.message
    );
    // Every other parameter failure, including null for an optional
    // parameter, is an invalid-parameters error.
    let good = r#"{"entries":[]}"#;
    for (method, params) in [
        (
            "proposal.check",
            format!(r#"{{"proposal":{good},"evaluation_time":null}}"#),
        ),
        (
            "proposal.check",
            format!(r#"{{"proposal":{good},"evaluation_time":"yesterday"}}"#),
        ),
        (
            "proposal.apply",
            format!(r#"{{"proposal":{good},"now":null}}"#),
        ),
        (
            "proposal.apply",
            format!(r#"{{"proposal":{good},"extra":1}}"#),
        ),
        (
            "proposal.apply",
            format!(r#"{{"proposal":{good},"proposal":{good}}}"#),
        ),
        (
            "proposal.encode",
            format!(r#"{{"proposal":{good},"format":"yaml"}}"#),
        ),
        ("entries.list", r#"{"kind":null}"#.into()),
        ("entries.list", r#"{"ty_name":null}"#.into()),
        ("entries.list", r#"{"text":null}"#.into()),
        ("entries.list", r#"{"currentness":null}"#.into()),
        (
            "analyzers.add",
            r#"{"name":"DAG_Check","evaluation_time":null}"#.into(),
        ),
        (
            "analyzers.add",
            r#"{"name":"DAG_Check","configuration":null}"#.into(),
        ),
    ] {
        let response = call(method, params.clone());
        assert_eq!(
            response["error"]["code"], -32602,
            "{method} {params} {response}"
        );
    }
    assert_eq!(scratch.bytes(), stored);
    // Surrounding whitespace is not part of the request.
    let response = serde_json::to_value(rpc::handle(
        &service,
        r#"  {"jsonrpc":"2.0","id":9,"method":"plan_id.mint"}  "#,
    ))
    .unwrap();
    assert!(response["result"]["plan_id"].is_string(), "{response}");
}

// Records are the core entry, in proposal vocabulary, with `current` beside
// its fields; currentness is judged in the listed view, before and after the
// revision that supersedes a node and its edge.
#[test]
fn entry_records_are_core_entries_with_currentness_beside_them() {
    let scratch = Scratch::new();
    let ids = populated(&scratch);
    let service = scratch.service();
    let list = |service: &Service| {
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "entries.list",
            "params": {"currentness": "all"}})
        .to_string();
        serde_json::to_value(rpc::handle(service, &line)).unwrap()["result"]["entries"].clone()
    };
    let id = |id: EntryId| serde_json::to_value(id).unwrap();
    let before = list(&service);
    assert_eq!(
        before[1],
        json!({"kind": "node", "id": id(ids[1]), "ty_name": "Task",
            "payload": {"title": "A", "notes": "n"}, "current": true})
    );
    assert_eq!(
        before[3],
        json!({"kind": "edge", "id": id(ids[3]), "ty_name": "DependsOn",
            "from": id(ids[1]), "to": id(ids[2]), "current": true})
    );
    let revised = service
        .apply(request(proposal(json!([
            {"name": "a2", "kind": "node", "ty_name": "Task", "predecessor": existing(ids[1]),
                "intent": "revise", "payload": {"title": "A2", "notes": "n"}},
        ]))))
        .unwrap()
        .report
        .operations[0]
        .entry;
    let after = list(&service);
    assert_eq!(after[1]["current"], false);
    assert_eq!(after[3]["current"], false);
    assert_eq!(
        after[4],
        json!({"kind": "node", "id": id(revised), "ty_name": "Task", "predecessor": id(ids[1]),
            "payload": {"title": "A2", "notes": "n"}, "current": true})
    );
    // The same shape decodes back into the record.
    let record: EntryRecord = serde_json::from_value(after[4].clone()).unwrap();
    assert_eq!(record.entry.id(), revised);
    assert!(record.current);
}

#[test]
fn every_listed_method_dispatches_and_no_other_does() {
    let scratch = Scratch::new();
    populated(&scratch);
    let service = scratch.service();
    let code = |method: &str| {
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": {}}).to_string();
        serde_json::to_value(rpc::handle(&service, &line)).unwrap()["error"]["code"].clone()
    };
    for method in rpc::Method::ALL {
        assert_eq!(rpc::Method::parse(method.name()), Some(*method));
        assert_ne!(code(method.name()), json!(-32601), "{method:?}");
    }
    for method in ["init", "entries", "database.describe.", "Database.describe"] {
        assert_eq!(code(method), json!(-32601), "{method}");
    }
}

// HTTP bodies may be formatted across lines; the envelope rules are the same.
#[test]
fn a_request_may_span_lines_when_its_framing_allows() {
    let scratch = Scratch::new();
    populated(&scratch);
    let service = scratch.service();
    let text = serde_json::to_string_pretty(
        &json!({"jsonrpc": "2.0", "id": "pretty", "method": "database.describe", "params": {}}),
    )
    .unwrap();
    assert!(text.contains('\n'));
    let response = serde_json::to_value(rpc::handle(&service, &format!("\n{text}\n"))).unwrap();
    assert_eq!(response["id"], "pretty");
    assert!(response["result"]["database"].is_string(), "{response}");
    let repeated = text.replace("\"method\"", "\"id\": 2,\n  \"method\"");
    let response = serde_json::to_value(rpc::handle(&service, &repeated)).unwrap();
    assert_eq!(response["error"]["code"], -32600);
}

// A split revision tree over `populated`: A is revised to A2 and branched to
// A3, A2 depends on B, and B depends on A3. Positions 0 to 7: the
// declaration, A, B, A->B, A2, A3, A2->B, B->A3.
fn split(scratch: &Scratch) -> Vec<EntryId> {
    let mut ids = populated(scratch);
    let applied = scratch
        .service()
        .apply(request(proposal(json!([
            {"name": "a2", "kind": "node", "ty_name": "Task", "predecessor": existing(ids[1]),
                "intent": "revise", "payload": {"title": "A2", "notes": "n"}},
            {"name": "a3", "kind": "node", "ty_name": "Task", "predecessor": existing(ids[1]),
                "intent": "branch", "payload": {"title": "A3", "notes": "n"}},
            {"name": "a2_b", "kind": "edge", "ty_name": "DependsOn",
                "from": {"added": "a2"}, "to": existing(ids[2])},
            {"name": "b_a3", "kind": "edge", "ty_name": "DependsOn",
                "from": existing(ids[2]), "to": {"added": "a3"}},
        ]))))
        .unwrap();
    ids.extend(applied.report.operations.iter().map(|o| o.entry));
    ids
}

// A database without analyzers, so declarations TypeCheck would refuse are
// stored. Positions 0 to 7: two current `Note` declarations, a `Ghost` node
// with no declaration, a `Note` node, a malformed `Tag` declaration, a
// nameless malformed one, and two differently typed edges between the nodes.
fn unchecked(scratch: &Scratch) -> Vec<EntryId> {
    let service = scratch.service();
    service.init(&[]).unwrap();
    let applied = service
        .apply(request(proposal(json!([
            {"name": "note", "kind": "node", "ty_name": "Type", "payload": note_declaration()},
            {"name": "note_again", "kind": "node", "ty_name": "Type", "payload": note_again()},
            {"name": "ghost", "kind": "node", "ty_name": "Ghost", "payload": {"title": "G"}},
            {"name": "a_note", "kind": "node", "ty_name": "Note", "payload": {"text": "t"}},
            {"name": "tag", "kind": "node", "ty_name": "Type", "payload": tag_declaration()},
            {"name": "nameless", "kind": "node", "ty_name": "Type", "payload": nameless_declaration()},
            {"name": "likes", "kind": "edge", "ty_name": "Likes",
                "from": {"added": "ghost"}, "to": {"added": "a_note"}},
            {"name": "hates", "kind": "edge", "ty_name": "Hates",
                "from": {"added": "a_note"}, "to": {"added": "ghost"}},
        ]))))
        .unwrap();
    applied.report.operations.iter().map(|o| o.entry).collect()
}

fn note_declaration() -> Value {
    json!({"name": "Note", "fields": {"kind": "record", "value": {"text": ["mandatory", "text"]}}})
}

fn note_again() -> Value {
    json!({"name": "Note", "fields": {"kind": "record", "value": {"text": ["optional", "list", "text"]}}})
}

fn tag_declaration() -> Value {
    json!({"name": "Tag", "fields": "text"})
}

fn nameless_declaration() -> Value {
    json!({"fields": {"kind": "record", "value": {}}})
}

fn declaration_body(payload: Value) -> Body {
    serde_json::from_value(payload).unwrap()
}

// The error a malformed declaration payload fails with.
fn schema_error(payload: Value) -> SchemaError {
    Type::from_body(&declaration_body(payload)).unwrap_err()
}

fn parsed(declaration: EntryId, payload: Value) -> Declaration {
    Declaration::Parsed {
        declaration,
        schema: Type::from_body(&declaration_body(payload)).unwrap(),
    }
}

#[test]
fn declarations_travel_as_the_core_schema_in_tagged_form() {
    let list = |element| ValueType::List {
        element: Box::new(element),
    };
    let owner = ValueType::Record {
        fields: [(
            "owner".into(),
            FieldDecl {
                status: FieldStatus::Mandatory,
                shape: ValueType::Text,
            },
        )]
        .into(),
    };
    let nested = Type::new(
        "Nested",
        &[
            (
                "grid",
                (list(list(ValueType::Integer)), FieldStatus::Optional),
            ),
            ("details", (owner, FieldStatus::Deprecated)),
            ("when", (ValueType::Timestamp, FieldStatus::Encouraged)),
            ("done", (ValueType::Boolean, FieldStatus::Mandatory)),
        ],
        &["Explains", "DependsOn"],
        &[],
        &["Nested"],
    );
    // Permissions keep their set order; shapes and statuses are tagged words.
    let encoded = json!({
        "name": "Nested",
        "fields": {
            "details": {"status": "deprecated", "shape": {"kind": "record", "fields": {
                "owner": {"status": "mandatory", "shape": {"kind": "text"}}}}},
            "done": {"status": "mandatory", "shape": {"kind": "boolean"}},
            "grid": {"status": "optional", "shape": {"kind": "list", "element": {
                "kind": "list", "element": {"kind": "integer"}}}},
            "when": {"status": "encouraged", "shape": {"kind": "timestamp"}},
        },
        "outgoing": ["DependsOn", "Explains"],
        "incoming": [],
        "successors": ["Nested"],
    });
    assert_eq!(serde_json::to_value(&nested).unwrap(), encoded);
    assert_eq!(serde_json::from_value::<Type>(encoded).unwrap(), nested);
    // The payload encoding (FILES 2.6) is a separate, word-list form.
    assert_eq!(
        serde_json::to_value(nested.to_body()).unwrap()["fields"]["value"]["grid"],
        json!(["optional", "list", "list", "integer"])
    );
}

#[test]
fn types_list_summarizes_every_name_the_view_knows_at_each_boundary() {
    let scratch = Scratch::new();
    let ids = split(&scratch);
    let service = scratch.service();
    let list = |as_of: Option<usize>| service.list_types(TypeListRequest { as_of }).unwrap();
    let names = |listed: &schema::TypeList| {
        listed
            .types
            .iter()
            .map(|summary| summary.name.clone())
            .collect::<Vec<_>>()
    };
    let full = list(None);
    assert_eq!(full.view.end_exclusive, 8);
    assert_eq!(names(&full), ["Task", "Type"]);
    let task = &full.types[0];
    let TypeResolution::Declared {
        declaration,
        schema,
    } = &task.resolution
    else {
        panic!("Task is declared: {:?}", task.resolution)
    };
    assert_eq!(*declaration, ids[0]);
    assert_eq!(*schema, Type::from_body(&task_type()).unwrap());
    assert_eq!(
        schema.fields["title"],
        FieldDecl {
            status: FieldStatus::Mandatory,
            shape: ValueType::Text,
        }
    );
    assert_eq!(schema.outgoing, BTreeSet::from(["DependsOn".to_string()]));
    // A, A2, A3, and B are tasks; A is superseded.
    assert_eq!(
        task.nodes,
        NodeCounts {
            current: 3,
            historical: 1,
        }
    );
    assert_eq!(full.types[1].resolution, TypeResolution::Builtin);
    assert_eq!(
        full.types[1].nodes,
        NodeCounts {
            current: 1,
            historical: 0,
        }
    );
    // Before the revisions both tasks are current; before any task only the
    // declaration is there; before everything only the metatype is known.
    let before = list(Some(4));
    assert_eq!(before.view.end_exclusive, 4);
    assert_eq!(
        before.types[0].nodes,
        NodeCounts {
            current: 2,
            historical: 0,
        }
    );
    let after_revision = list(Some(5));
    assert_eq!(
        after_revision.types[0].nodes,
        NodeCounts {
            current: 2,
            historical: 1,
        }
    );
    let declared = list(Some(1));
    assert_eq!(names(&declared), ["Task", "Type"]);
    assert_eq!(declared.types[0].nodes, NodeCounts::default());
    assert!(matches!(
        declared.types[0].resolution,
        TypeResolution::Declared { declaration, .. } if declaration == ids[0]
    ));
    let empty = list(Some(0));
    assert_eq!(empty.view.end_exclusive, 0);
    assert_eq!(names(&empty), ["Type"]);
    assert_eq!(empty.types[0].resolution, TypeResolution::Builtin);
    assert_eq!(empty.types[0].nodes, NodeCounts::default());
    assert!(matches!(
        service.list_types(TypeListRequest { as_of: Some(9) }),
        Err(AppError::BeyondBoundary {
            requested: 9,
            visible: 8
        })
    ));
    // Counts travel as decimal strings; resolutions are tagged by kind.
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "types.list", "params": {"as_of": "4"}})
        .to_string();
    let response = serde_json::to_value(rpc::handle(&service, &line)).unwrap();
    let types = &response["result"]["types"];
    assert_eq!(response["result"]["view"]["end_exclusive"], "4");
    assert_eq!(
        types[0]["nodes"],
        json!({"current": "2", "historical": "0"})
    );
    assert_eq!(types[0]["resolution"]["kind"], "declared");
    assert_eq!(types[0]["resolution"]["declaration"], json!(ids[0]));
    assert_eq!(
        types[0]["resolution"]["schema"]["fields"]["notes"],
        json!({"status": "encouraged", "shape": {"kind": "text"}})
    );
    assert_eq!(types[1]["resolution"], json!({"kind": "builtin"}));
}

#[test]
fn types_list_judges_each_name_by_its_own_declarations() {
    let scratch = Scratch::new();
    let ids = unchecked(&scratch);
    let service = scratch.service();
    let listed = service.list_types(TypeListRequest::default()).unwrap();
    let names = listed
        .types
        .iter()
        .map(|summary| summary.name.as_str())
        .collect::<Vec<_>>();
    // The nameless declaration claims no name, not even the empty one.
    assert_eq!(names, ["Ghost", "Note", "Tag", "Type"]);
    let summary = |name: &str| listed.types.iter().find(|t| t.name == name).unwrap();
    let one = NodeCounts {
        current: 1,
        historical: 0,
    };
    assert_eq!(
        summary("Tag").resolution,
        TypeResolution::Malformed {
            declaration: ids[4],
            error: schema_error(tag_declaration()),
        }
    );
    assert_eq!(
        summary("Note").resolution,
        TypeResolution::Ambiguous {
            claimants: vec![
                parsed(ids[0], note_declaration()),
                parsed(ids[1], note_again())
            ],
        }
    );
    assert_eq!(summary("Note").nodes, one);
    assert_eq!(summary("Ghost").resolution, TypeResolution::Missing);
    assert_eq!(summary("Ghost").nodes, one);
    assert_eq!(summary("Type").resolution, TypeResolution::Builtin);
    assert_eq!(
        summary("Type").nodes,
        NodeCounts {
            current: 4,
            historical: 0,
        }
    );
    // Before the malformed declarations were appended.
    let earlier = service
        .list_types(TypeListRequest { as_of: Some(4) })
        .unwrap();
    assert_eq!(
        earlier
            .types
            .iter()
            .map(|summary| summary.name.as_str())
            .collect::<Vec<_>>(),
        ["Ghost", "Note", "Type"]
    );
    // Before any node used `Ghost`, the ambiguous name is still listed.
    let earliest = service
        .list_types(TypeListRequest { as_of: Some(2) })
        .unwrap();
    assert_eq!(
        earliest
            .types
            .iter()
            .map(|summary| (summary.name.as_str(), summary.nodes))
            .collect::<Vec<_>>(),
        [
            ("Note", NodeCounts::default()),
            (
                "Type",
                NodeCounts {
                    current: 2,
                    historical: 0
                }
            )
        ]
    );
    // The structured error travels with its path and kind.
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "types.list", "params": {}}).to_string();
    let response = serde_json::to_value(rpc::handle(&service, &line)).unwrap();
    assert_eq!(
        response["result"]["types"][2]["resolution"],
        json!({"kind": "malformed", "declaration": ids[4], "error": {
            "path": ["fields"], "kind": "wrong_shape", "expected": "record", "actual": "text"}})
    );
    assert_eq!(
        response["result"]["types"][1]["resolution"]["claimants"][1]["kind"],
        "parsed"
    );
}

#[test]
fn inspecting_a_node_gives_its_type_lineage_and_incident_edges_in_the_view() {
    let scratch = Scratch::new();
    let ids = split(&scratch);
    let service = scratch.service();
    let inspect = |id: EntryId, as_of: Option<usize>| {
        service.inspect_entry(EntryInspectRequest { id, as_of })
    };
    let node = |id: EntryId, as_of: Option<usize>| match inspect(id, as_of) {
    Ok(EntryInspection::Node {
        view,
        entry,
        ty,
        lineage,
        edges,
        neighbours,
    }) => (view, entry, ty, lineage, edges, neighbours),
    other => panic!("expected a node inspection, got {other:?}"),
    };
    let positions = |records: &[EntryRecord]| {
        records
            .iter()
            .map(|record| record.entry.id())
            .collect::<Vec<_>>()
    };
    // B: a historical incoming edge from the superseded A, a current incoming
    // one from A2, and a current outgoing one to A3, in append order.
    let (view, entry, ty, lineage, edges, neighbours) = node(ids[2], None);
    assert_eq!(view.end_exclusive, 8);
    assert_eq!((entry.entry.id(), entry.current), (ids[2], true));
    assert!(matches!(ty, TypeResolution::Declared { declaration, .. } if declaration == ids[0]));
    assert_eq!(positions(&lineage), [ids[2]]);
    let incident = edges
        .iter()
        .map(|incident| {
            (
                incident.edge.entry.id(),
                incident.edge.current,
                incident.direction,
                incident.other,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        incident,
        [
            (ids[3], false, Incidence::Incoming, ids[1]),
            (ids[6], true, Incidence::Incoming, ids[4]),
            (ids[7], true, Incidence::Outgoing, ids[5]),
        ]
    );
    assert_eq!(
        neighbours
            .iter()
            .map(|record| (record.entry.id(), record.current))
            .collect::<Vec<_>>(),
        [(ids[1], false), (ids[4], true), (ids[5], true)]
    );
    // The whole revision tree, root first, from any of its nodes, as far as
    // the inspected view sees it.
    for (id, as_of, expected) in [
        (ids[5], None, vec![ids[1], ids[4], ids[5]]),
        (ids[1], None, vec![ids[1], ids[4], ids[5]]),
        (ids[4], Some(6), vec![ids[1], ids[4], ids[5]]),
        (ids[4], Some(5), vec![ids[1], ids[4]]),
        (ids[1], Some(4), vec![ids[1]]),
    ] {
        let (view, _, _, lineage, ..) = node(id, as_of);
        assert_eq!(view.end_exclusive, as_of.unwrap_or(8));
        assert_eq!(positions(&lineage), expected, "{as_of:?}");
    }
    // The superseded root and its edge are historical now, current before.
    let (_, entry, _, _, edges, _) = node(ids[1], None);
    assert!(!entry.current);
    assert_eq!(
        edges
            .iter()
            .map(|e| (e.edge.entry.id(), e.edge.current, e.direction))
            .collect::<Vec<_>>(),
        [(ids[3], false, Incidence::Outgoing)]
    );
    let (view, entry, _, _, edges, neighbours) = node(ids[1], Some(4));
    assert_eq!(view.end_exclusive, 4);
    assert!(entry.current);
    assert_eq!((edges[0].edge.current, edges[0].other), (true, ids[2]));
    assert_eq!(
        neighbours
            .iter()
            .map(|record| (record.entry.id(), record.current))
            .collect::<Vec<_>>(),
        [(ids[2], true)]
    );
    // Just after the revision, A is stale but the later edges are not visible.
    let (_, entry, _, _, edges, neighbours) = node(ids[2], Some(5));
    assert!(entry.current);
    assert_eq!(
        edges
            .iter()
            .map(|e| (e.edge.entry.id(), e.edge.current))
            .collect::<Vec<_>>(),
        [(ids[3], false)]
    );
    assert_eq!(positions(&neighbours), [ids[1]]);
    // An edge carries both endpoints as records of the same view.
    let Ok(EntryInspection::Edge {
        view,
        entry,
        from,
        to,
    }) = inspect(ids[3], None)
    else {
        panic!("expected an edge inspection")
    };
    assert_eq!(view.end_exclusive, 8);
    assert_eq!((entry.entry.id(), entry.current), (ids[3], false));
    assert_eq!((from.entry.id(), from.current), (ids[1], false));
    assert_eq!((to.entry.id(), to.current), (ids[2], true));
    let Ok(EntryInspection::Edge { entry, from, .. }) = inspect(ids[3], Some(4)) else {
        panic!("expected an edge inspection")
    };
    assert!(entry.current && from.current);
    // Absent from the view, unknown, or past the end.
    assert!(matches!(
        inspect(ids[5], Some(5)),
        Err(AppError::EntryNotFound { id }) if id == ids[5]
    ));
    let unknown: EntryId =
        serde_json::from_value(json!({"origin": Uuid::new_v4(), "position": "2"})).unwrap();
    assert!(matches!(
        inspect(unknown, None),
        Err(AppError::EntryNotFound { id }) if id == unknown
    ));
    assert!(matches!(
        inspect(ids[2], Some(9)),
        Err(AppError::BeyondBoundary {
            requested: 9,
            visible: 8
        })
    ));
    // On the wire the inspection, its records, and its type are tagged.
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "entries.inspect",
        "params": {"id": ids[5], "as_of": "8"}})
    .to_string();
    let response = serde_json::to_value(rpc::handle(&service, &line)).unwrap();
    let result = &response["result"];
    assert_eq!(result["kind"], "node", "{response}");
    assert_eq!(result["view"]["end_exclusive"], "8");
    assert_eq!(result["entry"]["kind"], "node");
    assert_eq!(result["type"]["kind"], "declared");
    assert_eq!(result["lineage"].as_array().unwrap().len(), 3);
    assert_eq!(result["lineage"][0]["current"], false);
    assert_eq!(result["edges"][0]["direction"], "incoming");
    assert_eq!(result["edges"][0]["edge"]["kind"], "edge");
    assert_eq!(result["edges"][0]["other"], json!(ids[2]));
    assert_eq!(result["neighbours"][0]["kind"], "node");
    assert_eq!(result["neighbours"][0]["id"], json!(ids[2]));
    let line =
        json!({"jsonrpc": "2.0", "id": 2, "method": "entries.inspect", "params": {"id": ids[3]}})
            .to_string();
    let response = serde_json::to_value(rpc::handle(&service, &line)).unwrap();
    assert_eq!(response["result"]["kind"], "edge");
    assert_eq!(response["result"]["from"]["kind"], "node");
}

// However many edges join two nodes, the inspection carries the other
// node's record once, and the records follow append order, not edge order.
#[test]
fn an_inspection_lists_each_neighbour_once_by_the_edges_that_reach_it() {
    let scratch = Scratch::new();
    let mut ids = unchecked(&scratch);
    let service = scratch.service();
    let applied = service
        .apply(request(proposal(json!([
            {"name": "cites", "kind": "edge", "ty_name": "Cites",
                "from": existing(ids[2]), "to": existing(ids[0])},
        ]))))
        .unwrap();
    ids.extend(applied.report.operations.iter().map(|o| o.entry));
    let (ghost, note) = (ids[2], ids[3]);
    for (as_of, edges, neighbours) in [
        (Some(6), vec![], vec![]),
        (
            Some(8),
            vec![
                (ids[6], Incidence::Outgoing, note),
                (ids[7], Incidence::Incoming, note),
            ],
            vec![note],
        ),
        (
            None,
            vec![
                (ids[6], Incidence::Outgoing, note),
                (ids[7], Incidence::Incoming, note),
                (ids[8], Incidence::Outgoing, ids[0]),
            ],
            vec![ids[0], note],
        ),
    ] {
        let Ok(EntryInspection::Node {
            edges: incident,
            neighbours: listed,
            ..
        }) = service.inspect_entry(EntryInspectRequest { id: ghost, as_of })
        else {
            panic!("expected a node inspection")
        };
        assert_eq!(
            incident
                .iter()
                .map(|e| (e.edge.entry.id(), e.direction, e.other))
                .collect::<Vec<_>>(),
            edges,
            "{as_of:?}"
        );
        assert_eq!(
            listed
                .iter()
                .map(|record| record.entry.id())
                .collect::<Vec<_>>(),
            neighbours,
            "{as_of:?}"
        );
    }
}

#[test]
fn inspection_resolves_the_node_type_in_the_inspected_view() {
    let scratch = Scratch::new();
    let ids = unchecked(&scratch);
    let service = scratch.service();
    let ty = |id: EntryId, as_of: Option<usize>| match service
        .inspect_entry(EntryInspectRequest { id, as_of })
    {
    Ok(EntryInspection::Node { ty, .. }) => ty,
    other => panic!("expected a node inspection, got {other:?}"),
    };
    assert_eq!(ty(ids[0], None), TypeResolution::Builtin);
    assert_eq!(ty(ids[4], None), TypeResolution::Builtin);
    // Each name resolves by its own current declarations, before and after
    // the malformed ones are appended, as `types.list` judges it.
    for as_of in [Some(4), Some(5), None] {
        assert_eq!(ty(ids[2], as_of), TypeResolution::Missing, "{as_of:?}");
        assert_eq!(
            ty(ids[3], as_of),
            TypeResolution::Ambiguous {
                claimants: vec![
                    parsed(ids[0], note_declaration()),
                    parsed(ids[1], note_again())
                ],
            },
            "{as_of:?}"
        );
    }
    // Only a node of the malformed type sees the malformed declaration.
    let applied = service
        .apply(request(proposal(json!([
            {"name": "a_tag", "kind": "node", "ty_name": "Tag", "payload": {"text": "t"}},
        ]))))
        .unwrap();
    let tag = applied.report.operations[0].entry;
    assert_eq!(
        ty(tag, None),
        TypeResolution::Malformed {
            declaration: ids[4],
            error: schema_error(tag_declaration()),
        }
    );
    assert_eq!(ty(ids[2], None), TypeResolution::Missing);
}

// Positions 0 to 5 under TypeCheck: the `Task` declaration, tasks A and B,
// their edge, a same-name revision of the declaration adding an optional
// field, and task C.
fn revised_task(scratch: &Scratch) -> (Vec<EntryId>, Type) {
    let mut ids = populated(scratch);
    let revised = Type::new(
        "Task",
        &[
            ("title", (ValueType::Text, FieldStatus::Mandatory)),
            ("status", (ValueType::Text, FieldStatus::Optional)),
            ("notes", (ValueType::Text, FieldStatus::Encouraged)),
            ("priority", (ValueType::Integer, FieldStatus::Optional)),
        ],
        &["DependsOn"],
        &["DependsOn"],
        &[],
    );
    let applied = scratch
        .service()
        .apply(request(proposal(json!([
            {"name": "task_type_2", "kind": "node", "ty_name": "Type",
                "predecessor": existing(ids[0]), "intent": "revise", "payload": revised.to_body()},
            task("c", "C"),
        ]))))
        .unwrap();
    ids.extend(applied.report.operations.iter().map(|o| o.entry));
    (ids, revised)
}

fn node_type(service: &Service, id: EntryId, as_of: Option<usize>) -> TypeResolution {
    match service.inspect_entry(EntryInspectRequest { id, as_of }) {
    Ok(EntryInspection::Node { ty, .. }) => ty,
    other => panic!("expected a node inspection, got {other:?}"),
    }
}

fn summary(listed: &schema::TypeList, name: &str) -> schema::TypeSummary {
    listed
        .types
        .iter()
        .find(|summary| summary.name == name)
        .cloned()
        .unwrap_or_else(|| panic!("{name} is not listed: {listed:?}"))
}

#[test]
fn a_same_name_declaration_revision_resolves_from_its_append_onward() {
    let scratch = Scratch::new();
    let (ids, revised) = revised_task(&scratch);
    let service = scratch.service();
    let old_schema = Type::from_body(&task_type()).unwrap();
    let new_schema = revised;
    let counts = |current, historical| NodeCounts {
        current,
        historical,
    };
    // Before, at, and after the revision: the name resolves to the
    // declaration current in that view, and the superseded one counts as a
    // historical `Type` node from the revision onward.
    for (as_of, declaration, schema, tasks, types) in [
        (Some(4), ids[0], &old_schema, counts(2, 0), counts(1, 0)),
        (Some(5), ids[4], &new_schema, counts(2, 0), counts(1, 1)),
        (None, ids[4], &new_schema, counts(3, 0), counts(1, 1)),
    ] {
        let listed = service.list_types(TypeListRequest { as_of }).unwrap();
        assert_eq!(listed.view.end_exclusive, as_of.unwrap_or(6));
        assert_eq!(
            listed
                .types
                .iter()
                .map(|summary| summary.name.as_str())
                .collect::<Vec<_>>(),
            ["Task", "Type"],
            "{as_of:?}"
        );
        let expected = TypeResolution::Declared {
            declaration,
            schema: schema.clone(),
        };
        let task = summary(&listed, "Task");
        assert_eq!(task.resolution, expected, "{as_of:?}");
        assert_eq!(task.nodes, tasks, "{as_of:?}");
        let ty = summary(&listed, "Type");
        assert_eq!(ty.resolution, TypeResolution::Builtin);
        assert_eq!(ty.nodes, types, "{as_of:?}");
        for id in [ids[1], ids[2]] {
            assert_eq!(node_type(&service, id, as_of), expected, "{as_of:?}");
        }
    }
    assert_eq!(
        node_type(&service, ids[5], None),
        TypeResolution::Declared {
            declaration: ids[4],
            schema: new_schema,
        }
    );
    // The declarations themselves are `Type` nodes forming one revision tree.
    for id in [ids[0], ids[4]] {
        let Ok(EntryInspection::Node {
            entry, ty, lineage, ..
        }) = service.inspect_entry(EntryInspectRequest { id, as_of: None })
        else {
            panic!("expected a node inspection")
        };
        assert_eq!(entry.current, id == ids[4]);
        assert_eq!(ty, TypeResolution::Builtin);
        assert_eq!(
            lineage.iter().map(|r| r.entry.id()).collect::<Vec<_>>(),
            [ids[0], ids[4]]
        );
    }
}

#[test]
fn a_declaration_revised_to_another_name_leaves_its_old_nodes_missing() {
    let scratch = Scratch::new();
    let service = scratch.service();
    service.init(&[]).unwrap();
    let text = [("text", (ValueType::Text, FieldStatus::Mandatory))];
    let note = Type::new("Note", &text, &[], &[], &[]);
    let memo = Type::new("Memo", &text, &[], &[], &[]);
    // Positions 0 to 3: the `Note` declaration, a note, the declaration
    // revised under the name `Memo`, and another note.
    let applied = service
        .apply(request(proposal(json!([
            {"name": "note", "kind": "node", "ty_name": "Type", "payload": note.to_body()},
            {"name": "first", "kind": "node", "ty_name": "Note", "payload": {"text": "one"}},
        ]))))
        .unwrap();
    let mut ids = applied
        .report
        .operations
        .iter()
        .map(|o| o.entry)
        .collect::<Vec<_>>();
    let applied = service
        .apply(request(proposal(json!([
            {"name": "memo", "kind": "node", "ty_name": "Type", "predecessor": existing(ids[0]),
                "intent": "revise", "payload": memo.to_body()},
            {"name": "second", "kind": "node", "ty_name": "Note", "payload": {"text": "two"}},
        ]))))
        .unwrap();
    ids.extend(applied.report.operations.iter().map(|o| o.entry));
    let counts = |current, historical| NodeCounts {
        current,
        historical,
    };
    let declared = |declaration, ty: &Type| TypeResolution::Declared {
        declaration,
        schema: ty.clone(),
    };
    // Before the revision `Note` is declared; from the revision onward it is
    // missing while its nodes still use it, and `Memo` is declared unused.
    let before = service
        .list_types(TypeListRequest { as_of: Some(2) })
        .unwrap();
    assert_eq!(
        before
            .types
            .iter()
            .map(|summary| summary.name.as_str())
            .collect::<Vec<_>>(),
        ["Note", "Type"]
    );
    assert_eq!(summary(&before, "Note").resolution, declared(ids[0], &note));
    assert_eq!(summary(&before, "Note").nodes, counts(1, 0));
    assert_eq!(summary(&before, "Type").nodes, counts(1, 0));
    assert_eq!(
        node_type(&service, ids[1], Some(2)),
        declared(ids[0], &note)
    );
    for (as_of, notes) in [(Some(3), counts(1, 0)), (None, counts(2, 0))] {
        let listed = service.list_types(TypeListRequest { as_of }).unwrap();
        assert_eq!(
            listed
                .types
                .iter()
                .map(|summary| summary.name.as_str())
                .collect::<Vec<_>>(),
            ["Memo", "Note", "Type"],
            "{as_of:?}"
        );
        assert_eq!(summary(&listed, "Note").resolution, TypeResolution::Missing);
        assert_eq!(summary(&listed, "Note").nodes, notes, "{as_of:?}");
        assert_eq!(summary(&listed, "Memo").resolution, declared(ids[2], &memo));
        assert_eq!(summary(&listed, "Memo").nodes, counts(0, 0));
        assert_eq!(summary(&listed, "Type").nodes, counts(1, 1));
        assert_eq!(node_type(&service, ids[1], as_of), TypeResolution::Missing);
    }
    assert_eq!(node_type(&service, ids[3], None), TypeResolution::Missing);
    // The empty prefix holds no entry and selects no node.
    let call = |method: &str, params: Value| {
        let line =
            json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
        serde_json::to_value(rpc::handle(&service, &line)).unwrap()
    };
    let response = call("entries.inspect", json!({"id": ids[1], "as_of": "0"}));
    assert_eq!(response["error"]["code"], 4001, "{response}");
    assert_eq!(response["error"]["data"]["kind"], "entry_not_found");
    let response = call(
        "views.query",
        json!({"seeds": [{"kind": "nodes"}], "expand": [{}], "as_of": "0"}),
    );
    assert_eq!(
        response["result"],
        json!({"view": {"database": response["result"]["view"]["database"],
            "end_exclusive": "0", "head": null}, "nodes": [], "edges": []}),
        "{response}"
    );
}

#[test]
fn view_queries_expand_from_seeds_with_provenance_in_append_order() {
    let scratch = Scratch::new();
    let ids = split(&scratch);
    let service = scratch.service();
    let query = |seeds: Vec<Seed>, expand: Vec<Expansion>, edges: EdgeFilter, as_of| {
        service.query_view(ViewQueryRequest {
            seeds,
            expand,
            edges,
            as_of,
        })
    };
    let seeds = |ids: &[EntryId]| vec![Seed::Ids { ids: ids.to_vec() }];
    let step = Expansion {
        hops: NonZeroUsize::MIN,
        direction: Direction::Both,
        ty_names: None,
        currentness: Currentness::Current,
    };
    let all_edges = EdgeFilter {
        ty_names: None,
        currentness: Currentness::All,
    };
    let nodes = |result: &ViewQuery| {
        result
            .nodes
            .iter()
            .map(|queried| (queried.node.entry.id(), queried.provenance.clone()))
            .collect::<Vec<_>>()
    };
    let edges = |result: &ViewQuery| {
        result
            .edges
            .iter()
            .map(|record| record.entry.id())
            .collect::<Vec<_>>()
    };
    let reached = |hop, via, from| Provenance::Reached { hop, via, from };
    // One hop over current edges from B, in both directions.
    let result = query(
        seeds(&[ids[2]]),
        vec![step.clone()],
        EdgeFilter::default(),
        None,
    )
    .unwrap();
    assert_eq!(result.view.end_exclusive, 8);
    assert_eq!(
        nodes(&result),
        [
            (ids[2], Provenance::Seed),
            (ids[4], reached(1, ids[6], ids[2])),
            (ids[5], reached(1, ids[7], ids[2])),
        ]
    );
    assert_eq!(edges(&result), [ids[6], ids[7]]);
    // Historical edges reach the superseded root; the edge filter decides
    // which edges among the result come back.
    let all = Expansion {
        currentness: Currentness::All,
        ..step.clone()
    };
    let result = query(
        seeds(&[ids[2]]),
        vec![all.clone()],
        EdgeFilter::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        nodes(&result)[..2],
        [
            (ids[1], reached(1, ids[3], ids[2])),
            (ids[2], Provenance::Seed)
        ]
    );
    assert_eq!(edges(&result), [ids[6], ids[7]]);
    let result = query(seeds(&[ids[2]]), vec![all.clone()], all_edges.clone(), None).unwrap();
    assert_eq!(edges(&result), [ids[3], ids[6], ids[7]]);
    assert_eq!(
        result.edges.iter().map(|e| e.current).collect::<Vec<_>>(),
        [false, true, true]
    );
    let historical = EdgeFilter {
        ty_names: None,
        currentness: Currentness::Historical,
    };
    let result = query(seeds(&[ids[2]]), vec![all], historical, None).unwrap();
    assert_eq!(edges(&result), [ids[3]]);
    // Direction is relative to the frontier node.
    let outgoing = Expansion {
        direction: Direction::Outgoing,
        ..step.clone()
    };
    let result = query(
        seeds(&[ids[2]]),
        vec![outgoing],
        EdgeFilter::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        nodes(&result),
        [
            (ids[2], Provenance::Seed),
            (ids[5], reached(1, ids[7], ids[2]))
        ]
    );
    assert_eq!(edges(&result), [ids[7]]);
    // A selection seeds the current tasks whose text contains "A".
    let selection = Seed::Nodes {
        ty_name: Some("Task".into()),
        currentness: Currentness::Current,
        text: Some("A".into()),
    };
    let result = query(vec![selection.clone()], vec![], EdgeFilter::default(), None).unwrap();
    assert_eq!(
        nodes(&result),
        [(ids[4], Provenance::Seed), (ids[5], Provenance::Seed)]
    );
    assert!(result.edges.is_empty());
    let result = query(
        vec![selection],
        vec![step.clone()],
        EdgeFilter::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        nodes(&result),
        [
            (ids[2], reached(1, ids[6], ids[4])),
            (ids[4], Provenance::Seed),
            (ids[5], Provenance::Seed),
        ]
    );
    assert_eq!(edges(&result), [ids[6], ids[7]]);
    // Seeds stay seeds however they were listed; a historical seed's
    // historical edge is not followed over current edges.
    let result = query(
        vec![
            Seed::Ids {
                ids: vec![ids[5], ids[2], ids[5]],
            },
            Seed::Nodes {
                ty_name: Some("Task".into()),
                currentness: Currentness::Historical,
                text: None,
            },
        ],
        vec![step.clone()],
        EdgeFilter::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        nodes(&result),
        [
            (ids[1], Provenance::Seed),
            (ids[2], Provenance::Seed),
            (ids[4], reached(1, ids[6], ids[2])),
            (ids[5], Provenance::Seed),
        ]
    );
    // Steps continue from the last hop's additions and hops count from the
    // seed across steps.
    let incoming = Expansion {
        direction: Direction::Incoming,
        ..step.clone()
    };
    let result = query(
        seeds(&[ids[5]]),
        vec![
            incoming.clone(),
            Expansion {
                currentness: Currentness::All,
                ..incoming.clone()
            },
        ],
        all_edges,
        None,
    )
    .unwrap();
    assert_eq!(
        nodes(&result),
        [
            (ids[1], reached(2, ids[3], ids[2])),
            (ids[2], reached(1, ids[7], ids[5])),
            (ids[4], reached(2, ids[6], ids[2])),
            (ids[5], Provenance::Seed),
        ]
    );
    assert_eq!(edges(&result), [ids[3], ids[6], ids[7]]);
    // Several hops in one step stop when the frontier empties.
    let result = query(
        seeds(&[ids[5]]),
        vec![Expansion {
            hops: NonZeroUsize::new(5).unwrap(),
            ..incoming
        }],
        EdgeFilter::default(),
        None,
    )
    .unwrap();
    assert_eq!(
        nodes(&result),
        [
            (ids[2], reached(1, ids[7], ids[5])),
            (ids[4], reached(2, ids[6], ids[2])),
            (ids[5], Provenance::Seed),
        ]
    );
    // Relationship types restrict what is followed and what is returned.
    let other = Some(vec!["Other".to_string()]);
    let result = query(
        seeds(&[ids[2]]),
        vec![Expansion {
            ty_names: other.clone(),
            ..step.clone()
        }],
        EdgeFilter::default(),
        None,
    )
    .unwrap();
    assert_eq!(nodes(&result), [(ids[2], Provenance::Seed)]);
    assert!(result.edges.is_empty());
    let result = query(
        seeds(&[ids[2], ids[5]]),
        vec![],
        EdgeFilter {
            ty_names: other,
            currentness: Currentness::Current,
        },
        None,
    )
    .unwrap();
    assert!(result.edges.is_empty());
    let result = query(
        seeds(&[ids[2], ids[5]]),
        vec![],
        EdgeFilter {
            ty_names: Some(vec!["DependsOn".into(), "Other".into()]),
            currentness: Currentness::Current,
        },
        None,
    )
    .unwrap();
    assert_eq!(edges(&result), [ids[7]]);
    // Before the revisions the old edge is current and the new nodes absent;
    // just before the last edge only A2 is reached.
    let result = query(
        seeds(&[ids[2]]),
        vec![step.clone()],
        EdgeFilter::default(),
        Some(4),
    )
    .unwrap();
    assert_eq!(result.view.end_exclusive, 4);
    assert_eq!(
        nodes(&result),
        [
            (ids[1], reached(1, ids[3], ids[2])),
            (ids[2], Provenance::Seed)
        ]
    );
    assert_eq!(edges(&result), [ids[3]]);
    assert!(result.edges[0].current && result.nodes[0].node.current);
    let result = query(
        seeds(&[ids[2]]),
        vec![step.clone()],
        EdgeFilter::default(),
        Some(7),
    )
    .unwrap();
    assert_eq!(
        nodes(&result),
        [
            (ids[2], Provenance::Seed),
            (ids[4], reached(1, ids[6], ids[2]))
        ]
    );
    assert_eq!(edges(&result), [ids[6]]);
    // A boundary past the end is an error.
    assert!(matches!(
        query(seeds(&[ids[2]]), vec![], EdgeFilter::default(), Some(9)),
        Err(AppError::BeyondBoundary {
            requested: 9,
            visible: 8
        })
    ));
    // So are seeds the view lacks or that are edges, refused together, each
    // once and in request order, whichever seed lists them; A3 is absent
    // before its append and a node after it.
    let unknown: EntryId =
        serde_json::from_value(json!({"origin": Uuid::new_v4(), "position": "2"})).unwrap();
    let mixed = vec![
        Seed::Ids {
            ids: vec![ids[5], ids[2], ids[3]],
        },
        Seed::Nodes {
            ty_name: None,
            currentness: Currentness::All,
            text: None,
        },
        Seed::Ids {
            ids: vec![unknown, ids[3], ids[5]],
        },
    ];
    let refusal = |id, reason| RefusedSeed { id, reason };
    for (as_of, expected) in [
        (
            Some(5),
            vec![
                refusal(ids[5], SeedRefusal::Absent),
                refusal(ids[3], SeedRefusal::Edge),
                refusal(unknown, SeedRefusal::Absent),
            ],
        ),
        (
            Some(6),
            vec![
                refusal(ids[3], SeedRefusal::Edge),
                refusal(unknown, SeedRefusal::Absent),
            ],
        ),
    ] {
        match query(
            mixed.clone(),
            vec![step.clone()],
            EdgeFilter::default(),
            as_of,
        ) {
        Err(AppError::InvalidSeeds { refused }) => assert_eq!(refused, expected, "{as_of:?}"),
        other => panic!("expected refused seeds, got {other:?}"),
        }
    }
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "views.query",
        "params": {"seeds": [{"kind": "ids", "ids": [ids[3], ids[2], ids[5]]}], "as_of": "5"}})
    .to_string();
    let response = serde_json::to_value(rpc::handle(&service, &line)).unwrap();
    assert_eq!(response["error"]["code"], 4006, "{response}");
    assert_eq!(
        response["error"]["data"],
        json!({"kind": "invalid_seeds", "refused": [
            {"id": ids[3], "reason": "edge"},
            {"id": ids[5], "reason": "absent"},
        ]})
    );
    // No seeds is an empty result.
    let result = query(vec![], vec![step], EdgeFilter::default(), None).unwrap();
    assert!(result.nodes.is_empty() && result.edges.is_empty());
}

#[test]
fn edge_type_filters_apply_to_expansion_and_to_the_returned_edges() {
    let scratch = Scratch::new();
    let ids = unchecked(&scratch);
    let service = scratch.service();
    let query = |expand: Vec<Expansion>, edges: EdgeFilter| {
        service
            .query_view(ViewQueryRequest {
                seeds: vec![Seed::Ids { ids: vec![ids[2]] }],
                expand,
                edges,
                as_of: None,
            })
            .unwrap()
    };
    let step = |ty_names: Option<Vec<String>>| Expansion {
        hops: NonZeroUsize::MIN,
        direction: Direction::Both,
        ty_names,
        currentness: Currentness::Current,
    };
    let likes = Some(vec!["Likes".to_string()]);
    let hates = Some(vec!["Hates".to_string()]);
    let via_likes = Provenance::Reached {
        hop: 1,
        via: ids[6],
        from: ids[2],
    };
    // Followed over Likes, returned edges restricted to Hates.
    let result = query(
        vec![step(likes)],
        EdgeFilter {
            ty_names: hates.clone(),
            currentness: Currentness::Current,
        },
    );
    assert_eq!(
        result
            .nodes
            .iter()
            .map(|n| (n.node.entry.id(), n.provenance.clone()))
            .collect::<Vec<_>>(),
        [(ids[2], Provenance::Seed), (ids[3], via_likes.clone())]
    );
    assert_eq!(
        result
            .edges
            .iter()
            .map(|e| e.entry.id())
            .collect::<Vec<_>>(),
        [ids[7]]
    );
    // Hates runs into the seed, so outgoing Hates reaches nothing; with any
    // type, the earlier edge decides the provenance.
    let result = query(
        vec![Expansion {
            direction: Direction::Outgoing,
            ..step(hates)
        }],
        EdgeFilter::default(),
    );
    assert_eq!(result.nodes.len(), 1);
    assert!(result.edges.is_empty());
    let result = query(vec![step(None)], EdgeFilter::default());
    assert_eq!(result.nodes[1].provenance, via_likes);
    assert_eq!(
        result
            .edges
            .iter()
            .map(|e| e.entry.id())
            .collect::<Vec<_>>(),
        [ids[6], ids[7]]
    );
}

#[test]
fn as_of_and_query_parameters_are_decoded_strictly_through_rpc() {
    let scratch = Scratch::new();
    let ids = split(&scratch);
    let service = scratch.service();
    let call = |method: &str, params: Value| {
        let line =
            json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
        serde_json::to_value(rpc::handle(&service, &line)).unwrap()
    };
    // `as_of` narrows every read and the result's view names the boundary.
    let listed = call("entries.list", json!({"currentness": "all", "as_of": "4"}));
    assert_eq!(listed["result"]["view"]["end_exclusive"], "4");
    assert_eq!(listed["result"]["entries"].as_array().unwrap().len(), 4);
    assert_eq!(listed["result"]["entries"][1]["current"], true);
    let got = call("entries.get", json!({"id": ids[1], "as_of": "5"}));
    assert_eq!(got["result"]["view"]["end_exclusive"], "5");
    assert_eq!(got["result"]["entry"]["current"], false);
    let got = call("entries.get", json!({"id": ids[1], "as_of": "4"}));
    assert_eq!(got["result"]["entry"]["current"], true);
    assert_eq!(
        call("entries.get", json!({"id": ids[4], "as_of": "4"}))["error"]["code"],
        4001
    );
    // A boundary past the end is its own error, with decimal positions.
    let beyond = call("entries.list", json!({"as_of": "9"}));
    assert_eq!(beyond["error"]["code"], 4002);
    assert_eq!(
        beyond["error"]["data"],
        json!({"kind": "beyond_boundary", "requested": "9", "visible": "8"})
    );
    for (method, params) in [
        ("entries.get", json!({"id": ids[1], "as_of": "9"})),
        ("entries.inspect", json!({"id": ids[1], "as_of": "9"})),
        ("types.list", json!({"as_of": "9"})),
        ("views.query", json!({"seeds": [], "as_of": "9"})),
    ] {
        let response = call(method, params);
        assert_eq!(response["error"]["code"], 4002, "{method} {response}");
        assert_eq!(response["error"]["data"]["kind"], "beyond_boundary");
    }
    // Null, numbers, non-canonical strings, and unknown members are invalid
    // parameters, as are zero hops (refused before the boundary is applied),
    // empty type lists, and foreign seed kinds.
    for (method, params) in [
        ("entries.list", json!({"as_of": null})),
        ("entries.list", json!({"as_of": 4})),
        ("entries.list", json!({"as_of": "04"})),
        ("entries.list", json!({"as_of": "-1"})),
        ("entries.get", json!({"id": ids[1], "as_of": null})),
        ("entries.inspect", json!({"id": ids[1], "as_of": null})),
        ("entries.inspect", json!({})),
        ("types.list", json!({"as_of": null})),
        ("types.list", json!({"kind": "node"})),
        ("views.query", json!({})),
        ("views.query", json!({"seeds": null})),
        ("views.query", json!({"seeds": [{"kind": "ids"}]})),
        (
            "views.query",
            json!({"seeds": [{"kind": "edges", "ids": []}]}),
        ),
        (
            "views.query",
            json!({"seeds": [{"kind": "nodes", "text": null}]}),
        ),
        (
            "views.query",
            json!({"seeds": [{"kind": "nodes", "fields": []}]}),
        ),
        (
            "views.query",
            json!({"seeds": [{"kind": "ids", "ids": [], "text": "x"}]}),
        ),
        ("views.query", json!({"seeds": [], "expand": null})),
        ("views.query", json!({"seeds": [], "expand": [{"hops": 0}]})),
        (
            "views.query",
            json!({"seeds": [], "expand": [{"hops": 0}], "as_of": "9"}),
        ),
        (
            "views.query",
            json!({"seeds": [], "expand": [{"ty_names": []}]}),
        ),
        (
            "views.query",
            json!({"seeds": [], "edges": {"ty_names": []}}),
        ),
        (
            "views.query",
            json!({"seeds": [], "expand": [{"hops": "1"}]}),
        ),
        (
            "views.query",
            json!({"seeds": [], "expand": [{"direction": "sideways"}]}),
        ),
        (
            "views.query",
            json!({"seeds": [], "expand": [{"ty_names": null}]}),
        ),
        (
            "views.query",
            json!({"seeds": [], "expand": [{"ty_name": "DependsOn"}]}),
        ),
        ("views.query", json!({"seeds": [], "edges": null})),
        (
            "views.query",
            json!({"seeds": [], "edges": {"currentness": "stale"}}),
        ),
        (
            "views.query",
            json!({"seeds": [], "edges": {"kind": "edge"}}),
        ),
    ] {
        let response = call(method, params.clone());
        assert_eq!(
            response["error"]["code"], -32602,
            "{method} {params} {response}"
        );
        assert_eq!(response["error"]["data"]["kind"], "invalid_params");
    }
    let empty = call(
        "views.query",
        json!({"seeds": [], "edges": {"ty_names": []}}),
    );
    assert!(
        empty["error"]["message"]
            .as_str()
            .unwrap()
            .contains("an empty ty_names list matches nothing; omit it for any type"),
        "{empty}"
    );
    // Defaults: one hop, both directions, any type, current edges.
    let result = call(
        "views.query",
        json!({"seeds": [{"kind": "ids", "ids": [ids[2]]}], "expand": [{}], "edges": {}}),
    );
    let nodes = result["result"]["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3, "{result}");
    assert_eq!(nodes[0]["provenance"], json!({"kind": "seed"}));
    assert_eq!(
        nodes[1]["provenance"],
        json!({"kind": "reached", "hop": 1, "via": ids[6], "from": ids[2]})
    );
    assert_eq!(nodes[1]["node"]["kind"], "node");
    assert_eq!(nodes[1]["node"]["current"], true);
    assert_eq!(result["result"]["edges"].as_array().unwrap().len(), 2);
    assert_eq!(result["result"]["edges"][0]["kind"], "edge");
    assert_eq!(result["result"]["view"]["end_exclusive"], "8");
}

// Appends to `populated` a task without its encouraged `notes`, which
// TypeCheck warns about from its append (position 4, boundary 5) onward.
fn warned(scratch: &Scratch) -> EntryId {
    let applied = scratch
        .service()
        .apply(request(proposal(json!([
            {"name": "c", "kind": "node", "ty_name": "Task", "payload": {"title": "C"}},
        ]))))
        .unwrap();
    applied.report.operations[0].entry
}

fn render_dot(service: &Service, seeds: Vec<Seed>, as_of: Option<usize>) -> String {
    service
        .render_view(ViewRenderRequest {
            query: ViewQueryRequest {
                seeds,
                expand: vec![],
                edges: EdgeFilter::default(),
                as_of,
            },
            format: RenderFormat::Dot,
        })
        .unwrap()
        .text
}

fn run(service: &Service, as_of: Option<usize>) -> crate::service::AnalysisRun {
    service
        .run_analysis(AnalysisRunRequest {
            as_of,
            evaluation_time: None,
            now: false,
        })
        .unwrap()
}

fn boundaries(view: EvaluatedView) -> (usize, usize) {
    match view {
    EvaluatedView::Prefixes { first, last } => (first.end_exclusive, last.end_exclusive),
    EvaluatedView::Initialization => panic!("expected a prefix range"),
    }
}

#[test]
fn analysis_run_replays_the_stored_suite_at_the_selected_boundary() {
    let scratch = Scratch::new();
    let service = scratch.service();
    populated(&scratch);
    let clean = run(&service, None);
    assert_eq!(clean.analysis.view.end_exclusive, 4);
    assert_eq!(clean.analysis.inputs.evaluation_time, None);
    assert_eq!(
        clean
            .analysis
            .implementations
            .iter()
            .map(|identity| identity.name.as_str())
            .collect::<Vec<_>>(),
        [TypeCheckFactory::name()]
    );
    assert_eq!(clean.analysis.observations().count(), 0);
    assert!(clean.dismissals.is_empty());
    let c = warned(&scratch);
    let at: OffsetTimestamp = "2026-01-02T04:04:05.000000006+01:00".parse().unwrap();
    let warned = service
        .run_analysis(AnalysisRunRequest {
            as_of: None,
            evaluation_time: Some(at),
            now: false,
        })
        .unwrap();
    assert_eq!(warned.analysis.view.end_exclusive, 5);
    assert_eq!(warned.analysis.inputs.evaluation_time, Some(at));
    let observations = warned.analysis.observations().collect::<Vec<_>>();
    assert_eq!(observations.len(), 1);
    let warning = observations[0];
    assert_eq!(warning.key.kind, MessageType::Warning);
    assert_eq!(warning.key.analyzer.name, TypeCheckFactory::name());
    assert_eq!(
        warning.key.database,
        service.describe(NoParams {}).unwrap().database
    );
    assert_eq!(warning.diagnostic.focus, Some(c));
    assert_eq!(warning.diagnostic.subjects, BTreeSet::from([c]));
    assert_eq!(boundaries(warning.evaluated_view), (5, 5));
    // The prefix before the append is clean; the one ending with it warns,
    // and a later append extends the range.
    assert_eq!(run(&service, Some(4)).analysis.observations().count(), 0);
    let at_append = run(&service, Some(5));
    assert_eq!(at_append.analysis.view.end_exclusive, 5);
    assert_eq!(
        at_append
            .analysis
            .observations()
            .map(|o| (o.key.clone(), o.evaluated_view))
            .collect::<Vec<_>>(),
        [(warning.key.clone(), warning.evaluated_view)]
    );
    service
        .apply(request(proposal(json!([task("d", "D")]))))
        .unwrap();
    let extended = run(&service, None);
    let ranges = extended
        .analysis
        .observations()
        .map(|o| boundaries(o.evaluated_view))
        .collect::<Vec<_>>();
    assert_eq!(ranges, [(5, 6)]);
    assert_eq!(
        boundaries(
            run(&service, Some(5))
                .analysis
                .observations()
                .next()
                .unwrap()
                .evaluated_view
        ),
        (5, 5)
    );
    // The suite is not versioned by history: an analyzer added now replays
    // every prefix, so an earlier boundary reports it too.
    service
        .add_analyzer(AddAnalyzerRequest {
            name: DagCheckFactory::name().into(),
            configuration: r#"{"on_cycle":"warning"}"#.into(),
            evaluation_time: None,
            now: false,
        })
        .unwrap();
    let names = |as_of| {
        run(&service, as_of)
            .analysis
            .implementations
            .iter()
            .map(|identity| identity.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(Some(2)),
        [TypeCheckFactory::name(), DagCheckFactory::name()]
    );
    assert_eq!(names(None), names(Some(2)));
    // The host captures `now` once, refuses both inputs, and bounds the view.
    let now = service
        .run_analysis(AnalysisRunRequest {
            as_of: None,
            evaluation_time: None,
            now: true,
        })
        .unwrap();
    assert!(now.analysis.inputs.evaluation_time.is_some());
    assert!(matches!(
        service.run_analysis(AnalysisRunRequest {
            as_of: None,
            evaluation_time: Some(at),
            now: true,
        }),
        Err(AppError::InvalidParams { .. })
    ));
    assert!(matches!(
        service.run_analysis(AnalysisRunRequest {
            as_of: Some(7),
            evaluation_time: None,
            now: false,
        }),
        Err(AppError::BeyondBoundary {
            requested: 7,
            visible: 6
        })
    ));
    // Through RPC: the carried inputs, strict parameters, and the records.
    let call = |params: Value| {
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "analysis.run", "params": params})
            .to_string();
        serde_json::to_value(rpc::handle(&service, &line)).unwrap()
    };
    let response =
        call(json!({"as_of": "5", "evaluation_time": "2026-01-02T04:04:05.000000006+01:00"}));
    let result = &response["result"];
    assert_eq!(
        result["analysis"]["view"]["end_exclusive"], "5",
        "{response}"
    );
    assert_eq!(
        result["analysis"]["inputs"]["evaluation_time"],
        "2026-01-02T04:04:05.000000006+01:00"
    );
    assert_eq!(
        result["analysis"]["observations"][0]["key"]["kind"],
        "warning"
    );
    assert_eq!(result["dismissals"], json!([]));
    assert_eq!(
        call(json!({}))["result"]["analysis"]["inputs"]["evaluation_time"],
        Value::Null
    );
    for params in [
        json!({"as_of": null}),
        json!({"as_of": 5}),
        json!({"evaluation_time": null}),
        // An instant without its offset is not an evaluation time.
        json!({"evaluation_time": "2026-01-02T03:04:05Z"}),
        json!({"now": "yes"}),
        json!({"seeds": []}),
    ] {
        let response = call(params.clone());
        assert_eq!(response["error"]["code"], -32602, "{params} {response}");
    }
    assert_eq!(call(json!({"as_of": "9"}))["error"]["code"], 4002);
}

#[test]
fn dismissals_persist_without_rotating_the_revision_and_refuse_inconsistencies() {
    let scratch = Scratch::new();
    let service = scratch.service();
    populated(&scratch);
    warned(&scratch);
    let key = run(&service, None)
        .analysis
        .observations()
        .next()
        .unwrap()
        .key
        .clone();
    let revision = service.describe(NoParams {}).unwrap().graph_revision;
    let stored = scratch.bytes();
    let dismiss = |key: &FindingKey, reason, note: Option<&str>| {
        service.dismiss_finding(DismissRequest {
            key: key.clone(),
            reason,
            note: note.map(Into::into),
        })
    };
    let dismissed = dismiss(
        &key,
        DismissalReason::FalsePositive,
        Some("notes come later"),
    )
    .unwrap();
    assert_eq!(
        dismissed.dismissals,
        [Dismissal {
            key: key.clone(),
            reason: DismissalReason::FalsePositive,
            note: Some("notes come later".into()),
        }]
    );
    assert_ne!(scratch.bytes(), stored);
    assert_eq!(
        service.describe(NoParams {}).unwrap().graph_revision,
        revision
    );
    // A fresh service rereads the record beside the replayed analysis, which
    // the dismissal never changes.
    let reread = run(&scratch.service(), None);
    assert_eq!(reread.dismissals, dismissed.dismissals);
    assert_eq!(reread.analysis.observations().count(), 1);
    // Dismissing again updates the one record in place.
    let updated = dismiss(&key, DismissalReason::Acknowledged, None).unwrap();
    assert_eq!(
        updated.dismissals,
        [Dismissal {
            key: key.clone(),
            reason: DismissalReason::Acknowledged,
            note: None,
        }]
    );
    // A key nothing currently emits is still stored, after the existing one.
    let unseen = FindingKey {
        kind: MessageType::Suggestion,
        analyzer_certificate: AnalyzerCertificate("never emitted".into()),
        ..key.clone()
    };
    let two = dismiss(&unseen, DismissalReason::NoLongerRelevant, None).unwrap();
    assert_eq!(
        two.dismissals.iter().map(|d| &d.key).collect::<Vec<_>>(),
        [&key, &unseen]
    );
    // An inconsistency key is refused before anything is stored, emitted or
    // not; the error carries the key.
    let inconsistency = FindingKey {
        kind: MessageType::Inconsistency,
        ..key.clone()
    };
    let stored = scratch.bytes();
    let error = dismiss(&inconsistency, DismissalReason::Acknowledged, None).unwrap_err();
    assert!(
        matches!(&error, AppError::CannotDismissInconsistency { key } if **key == inconsistency),
        "{error}"
    );
    let object = error.to_object();
    assert_eq!(object.code, 4003);
    assert_eq!(
        object.data,
        Some(json!({"kind": "cannot_dismiss_inconsistency", "key": inconsistency}))
    );
    assert_eq!(scratch.bytes(), stored);
    // Restore removes exactly the matching record and says whether one was
    // there; nothing is published otherwise.
    let restored = service
        .restore_finding(RestoreRequest { key: key.clone() })
        .unwrap();
    assert!(restored.restored);
    assert_eq!(
        restored
            .dismissals
            .iter()
            .map(|d| &d.key)
            .collect::<Vec<_>>(),
        [&unseen]
    );
    let stored = scratch.bytes();
    let again = service
        .restore_finding(RestoreRequest { key: key.clone() })
        .unwrap();
    assert!(!again.restored);
    assert_eq!(again.dismissals, restored.dismissals);
    assert_eq!(scratch.bytes(), stored);
    assert_eq!(
        service.describe(NoParams {}).unwrap().graph_revision,
        revision
    );
    // The remaining record's key is never emitted, so a run returns none.
    assert_eq!(run(&scratch.service(), None).dismissals, []);
    // Writers queue behind the lock like every mutation, but an inconsistency
    // is refused before the lock is consulted.
    let holder = storage::lock(&scratch.state(), Duration::ZERO).unwrap();
    let quick = scratch.service().with_busy_retry(SHORT_RETRY);
    assert!(matches!(
        quick.dismiss_finding(DismissRequest {
            key: key.clone(),
            reason: DismissalReason::Acknowledged,
            note: None,
        }),
        Err(AppError::Busy { .. })
    ));
    assert!(matches!(
        quick.restore_finding(RestoreRequest {
            key: unseen.clone()
        }),
        Err(AppError::Busy { .. })
    ));
    assert!(matches!(
        quick.dismiss_finding(DismissRequest {
            key: inconsistency.clone(),
            reason: DismissalReason::Acknowledged,
            note: None,
        }),
        Err(AppError::CannotDismissInconsistency { .. })
    ));
    drop(holder);
    assert_eq!(scratch.bytes(), stored);
    // Through RPC: snake_case reasons, a null note refused, and the 4003
    // payload.
    let call = |method: &str, params: Value| {
        let line =
            json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
        serde_json::to_value(rpc::handle(&service, &line)).unwrap()
    };
    for params in [
        json!({"key": key, "reason": "no_longer_relevant", "note": null}),
        json!({"key": key, "reason": "NoLongerRelevant"}),
        json!({"key": key}),
        json!({"reason": "acknowledged"}),
        json!({"key": key, "reason": "acknowledged", "sticky": true}),
    ] {
        let response = call("findings.dismiss", params.clone());
        assert_eq!(response["error"]["code"], -32602, "{params} {response}");
    }
    let refused = call(
        "findings.dismiss",
        json!({"key": inconsistency, "reason": "acknowledged"}),
    );
    assert_eq!(refused["error"]["code"], 4003, "{refused}");
    assert_eq!(
        refused["error"]["data"],
        json!({"kind": "cannot_dismiss_inconsistency", "key": inconsistency})
    );
    let response = call(
        "findings.dismiss",
        json!({"key": key, "reason": "no_longer_relevant"}),
    );
    assert_eq!(
        response["result"]["dismissals"][1],
        json!({"key": key, "reason": "no_longer_relevant", "note": null}),
        "{response}"
    );
    let response = call("findings.restore", json!({"key": unseen}));
    assert_eq!(response["result"]["restored"], true, "{response}");
    assert_eq!(
        response["result"]["dismissals"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        call(
            "findings.restore",
            json!({"key": unseen, "reason": "acknowledged"})
        )["error"]["code"],
        -32602
    );
    // An uncertain publication carries the stored records, tagged for the
    // client like every other outcome, a restore with its `restored`.
    let failing = scratch.service().with_sync_directory(failed_sync);
    let error = failing
        .restore_finding(RestoreRequest { key: key.clone() })
        .unwrap_err();
    let AppError::PublicationUncertain { outcome, .. } = &error else {
        panic!("expected an uncertain publication, got {error}")
    };
    assert_eq!(
        **outcome,
        Published::Restored(RestoreResult {
            dismissals: vec![],
            restored: true,
        })
    );
    assert_eq!(
        serde_json::to_value(outcome).unwrap(),
        json!({"kind": "restored", "dismissals": [], "restored": true})
    );
    assert_eq!(run(&scratch.service(), None).dismissals, []);
    let error = failing
        .dismiss_finding(DismissRequest {
            key: key.clone(),
            reason: DismissalReason::Acknowledged,
            note: Some("uncertain".into()),
        })
        .unwrap_err();
    let AppError::PublicationUncertain { outcome, .. } = &error else {
        panic!("expected an uncertain publication, got {error}")
    };
    let record = Dismissal {
        key: key.clone(),
        reason: DismissalReason::Acknowledged,
        note: Some("uncertain".into()),
    };
    assert_eq!(
        **outcome,
        Published::Dismissed(DismissalList {
            dismissals: vec![record.clone()],
        })
    );
    assert_eq!(
        serde_json::to_value(outcome).unwrap(),
        json!({"kind": "dismissed", "dismissals": [record]})
    );
    assert_eq!(error.to_object().code, 1008);
    // The new state is visible anyway.
    assert_eq!(run(&scratch.service(), None).dismissals, [record]);
}

// The analysis `as_of` selects, never a later one, decides which records a
// run returns: the warning about C is first emitted at boundary 5.
#[test]
fn analysis_run_returns_only_the_records_of_findings_it_emitted() {
    let scratch = Scratch::new();
    let service = scratch.service();
    populated(&scratch);
    let c = warned(&scratch);
    let warning = run(&service, None)
        .analysis
        .observations()
        .next()
        .unwrap()
        .clone();
    assert_eq!(warning.diagnostic.focus, Some(c));
    assert_eq!(c.position(), 4);
    let record = Dismissal {
        key: warning.key.clone(),
        reason: DismissalReason::Acknowledged,
        note: Some("later".into()),
    };
    service
        .dismiss_finding(DismissRequest {
            key: record.key.clone(),
            reason: record.reason,
            note: record.note.clone(),
        })
        .unwrap();
    for (as_of, expected) in [
        (Some(0), vec![]),
        (Some(4), vec![]),
        (Some(5), vec![record.clone()]),
        (None, vec![record.clone()]),
    ] {
        let ran = run(&service, as_of);
        assert_eq!(ran.dismissals, expected, "as_of {as_of:?}");
        assert_eq!(
            ran.analysis.observations().count(),
            expected.len(),
            "as_of {as_of:?}"
        );
    }
    // A record differing from an emitted key in any part of it is not
    // returned, even beside the matching record.
    let near = [
        FindingKey {
            analyzer_certificate: AnalyzerCertificate(format!(
                "{} ",
                warning.key.analyzer_certificate.0
            )),
            ..warning.key.clone()
        },
        FindingKey {
            kind: MessageType::Suggestion,
            ..warning.key.clone()
        },
        FindingKey {
            analyzer: AnalyzerIdentity {
                configuration: "{ }".into(),
                ..warning.key.analyzer.clone()
            },
            ..warning.key.clone()
        },
    ];
    for key in &near {
        service
            .dismiss_finding(DismissRequest {
                key: key.clone(),
                reason: DismissalReason::FalsePositive,
                note: None,
            })
            .unwrap();
    }
    let all = service
        .restore_finding(RestoreRequest {
            key: near[0].clone(),
        })
        .unwrap();
    assert_eq!(all.dismissals.len(), 3);
    assert_eq!(
        run(&service, None).dismissals,
        std::slice::from_ref(&record)
    );
    assert_eq!(run(&service, Some(4)).dismissals, []);
    // Through RPC, the same boundary decides.
    let call = |params: Value| {
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "analysis.run", "params": params})
            .to_string();
        serde_json::to_value(rpc::handle(&service, &line)).unwrap()
    };
    assert_eq!(
        call(json!({"as_of": "4"}))["result"]["dismissals"],
        json!([])
    );
    assert_eq!(
        call(json!({"as_of": "5"}))["result"]["dismissals"],
        json!([record])
    );
}

// Dismissal edits change only the records: the graph is equal before and
// after, keys of another database are refused, and an edit that changes
// nothing publishes nothing.
#[test]
fn dismissal_edits_keep_the_graph_and_publish_only_changes() {
    let scratch = Scratch::new();
    let service = scratch.service();
    populated(&scratch);
    warned(&scratch);
    let key = run(&service, None)
        .analysis
        .observations()
        .next()
        .unwrap()
        .key
        .clone();
    let before = storage::read(&scratch.state()).unwrap();
    let dismiss = |service: &Service, key: &FindingKey, note: Option<&str>| {
        service.dismiss_finding(DismissRequest {
            key: key.clone(),
            reason: DismissalReason::FalsePositive,
            note: note.map(Into::into),
        })
    };
    dismiss(&service, &key, Some("why")).unwrap();
    let after = storage::read(&scratch.state()).unwrap();
    assert!(after.graph == before.graph);
    assert_eq!(after.graph_revision, before.graph_revision);
    assert_ne!(after.dismissals, before.dismissals);
    // A service whose publication always ends uncertain shows whether an
    // edit published at all.
    let failing = scratch.service().with_sync_directory(failed_sync);
    let stored = scratch.bytes();
    let same = dismiss(&failing, &key, Some("why")).unwrap();
    assert_eq!(
        same.dismissals,
        after.dismissals.records().cloned().collect::<Vec<_>>()
    );
    assert_eq!(scratch.bytes(), stored);
    assert!(matches!(
        dismiss(&failing, &key, Some("other")),
        Err(AppError::PublicationUncertain { .. })
    ));
    assert!(matches!(
        dismiss(&failing, &key, None),
        Err(AppError::PublicationUncertain { .. })
    ));
    let stored = scratch.bytes();
    assert!(dismiss(&failing, &key, None).is_ok());
    assert_eq!(scratch.bytes(), stored);
    // A key of another database is foreign to a dismissal, and restoring
    // one matches nothing and publishes nothing.
    let foreign = FindingKey {
        database: Uuid::new_v4(),
        ..key.clone()
    };
    let error = dismiss(&service, &foreign, None).unwrap_err();
    let selected = before.graph.view().id().database;
    assert!(
        matches!(&error, AppError::ForeignDatabase { expected, actual, .. }
            if *expected == foreign.database && *actual == selected),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        format!(
            "the finding key names database {}, but the selected database is {selected}",
            foreign.database
        )
    );
    assert_eq!(error.to_object().code, 2004);
    assert_eq!(scratch.bytes(), stored);
    let restored = failing
        .restore_finding(RestoreRequest {
            key: foreign.clone(),
        })
        .unwrap();
    assert!(!restored.restored);
    assert_eq!(restored.dismissals.len(), 1);
    assert_eq!(scratch.bytes(), stored);
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "findings.dismiss",
        "params": {"key": foreign, "reason": "acknowledged"}})
    .to_string();
    let response = serde_json::to_value(rpc::handle(&service, &line)).unwrap();
    assert_eq!(response["error"]["code"], 2004, "{response}");
    assert_eq!(
        response["error"]["data"],
        json!({"kind": "foreign_database", "expected": foreign.database, "actual": selected})
    );
    assert_eq!(scratch.bytes(), stored);
    assert!(storage::read(&scratch.state()).unwrap().graph == before.graph);
}

// A dismissal waiting on the lock rereads after acquiring it, so records and
// a revision published under the held lock all survive its edit.
#[test]
fn interleaved_dismissal_edits_keep_each_other_and_the_revision() {
    let scratch = Scratch::new();
    populated(&scratch);
    warned(&scratch);
    let state = scratch.state();
    let key = run(&scratch.service(), None)
        .analysis
        .observations()
        .next()
        .unwrap()
        .key
        .clone();
    let held_key = FindingKey {
        analyzer_certificate: AnalyzerCertificate("held".into()),
        ..key.clone()
    };
    let lock = storage::lock(&state, Duration::ZERO).unwrap();
    let waiting = {
        let state = state.clone();
        let key = key.clone();
        std::thread::spawn(move || {
            service(state)
                .with_busy_retry(Duration::from_secs(10))
                .dismiss_finding(DismissRequest {
                    key,
                    reason: DismissalReason::Acknowledged,
                    note: Some("waiting".into()),
                })
        })
    };
    // Give the writer time to start waiting on the held lock.
    std::thread::sleep(Duration::from_millis(200));
    let mut locked = lock.read().unwrap();
    let snapshot = locked.snapshot_mut();
    snapshot
        .dismissals
        .dismiss(held_key.clone(), DismissalReason::FalsePositive, None)
        .unwrap();
    let held = Body::from([
        ("title".into(), "Held".into()),
        ("notes".into(), "n".into()),
    ]);
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory))
        .unwrap();
    transaction::prepare(
        &snapshot.graph,
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Task".into(),
            payload: held,
            predecessor: None,
            intent: None,
        }))],
        &mut runner,
    )
    .unwrap()
    .accepted()
    .unwrap()
    .commit(&mut snapshot.graph)
    .unwrap();
    snapshot.graph_revision = Uuid::new_v4();
    let held_revision = snapshot.graph_revision;
    let held_graph = snapshot.graph.clone();
    assert!(matches!(
        locked.publish(storage::sync_directory),
        Ok(Publication::Complete)
    ));
    drop(locked);
    let dismissed = waiting.join().unwrap().unwrap();
    assert_eq!(
        dismissed
            .dismissals
            .iter()
            .map(|record| (&record.key, record.note.as_deref()))
            .collect::<Vec<_>>(),
        [(&held_key, None), (&key, Some("waiting"))]
    );
    let stored = storage::read(&state).unwrap();
    assert_eq!(stored.graph_revision, held_revision);
    assert!(stored.graph == held_graph);
    assert_eq!(
        stored.dismissals.records().cloned().collect::<Vec<_>>(),
        dismissed.dismissals
    );
}

// The `split` subgraph one hop around `seed` over every edge.
fn around(
    service: &Service,
    seed: EntryId,
    as_of: Option<usize>,
    format: RenderFormat,
) -> Result<crate::render::Rendered, AppError> {
    service.render_view(ViewRenderRequest {
        query: ViewQueryRequest {
            seeds: vec![Seed::Ids { ids: vec![seed] }],
            expand: vec![Expansion {
                hops: NonZeroUsize::MIN,
                direction: Direction::Both,
                ty_names: None,
                currentness: Currentness::All,
            }],
            edges: EdgeFilter {
                ty_names: None,
                currentness: Currentness::All,
            },
            as_of,
        },
        format,
    })
}

// The quoted values of `attribute` on each node or edge line that has it.
fn dot_attributes(text: &str, attribute: &str) -> Vec<String> {
    let starts = [format!("[{attribute}="), format!(" {attribute}=")];
    text.lines()
        .filter_map(|line| {
            let (at, start) = starts
                .iter()
                .find_map(|start| Some((line.find(start.as_str())?, start)))?;
            let value = &line[at + start.len()..];
            // Every value here is quoted and followed by another attribute.
            Some(value[..value.find("\", ")? + 1].to_string())
        })
        .collect()
}

#[test]
fn render_draws_only_the_queried_subgraph_as_dot() {
    let scratch = Scratch::new();
    let ids = split(&scratch);
    let service = scratch.service();
    let rendered = around(&service, ids[2], None, RenderFormat::Dot).unwrap();
    let described = service.describe(NoParams {}).unwrap();
    // Each apply mints its own origin, so an href names the entry's own.
    let href = |position: usize| format!("href=\"#inspect/{}/{position}\"", ids[position].origin());
    let gray = |stale: bool| if stale { ", color=gray" } else { "" };
    let node = |position: usize, title: &str, stale: bool| {
        let label = format!("\"{title}\\n@{position} Task\"");
        format!(
            "  n{position} [label={label}, tooltip={label}, id=\"n{position}\", {}{}];",
            href(position),
            gray(stale)
        )
    };
    let edge = |from: usize, to: usize, position: usize, stale: bool| {
        let label = format!("\"@{position} DependsOn\"");
        format!(
            "  n{from} -> n{to} [label={label}, tooltip={label}, labeltooltip={label}, id=\"e{position}\", {}, style=solid{}];",
            href(position),
            gray(stale)
        )
    };
    assert_eq!(rendered.format, RenderFormat::Dot);
    let lines = rendered.text.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "digraph lifecore {");
    assert_eq!(
        lines[1],
        format!("  label=\"{} boundary 8\";", described.database)
    );
    assert_eq!(lines.last(), Some(&"}"));
    // Nodes in append order, the superseded root gray; then revision links,
    // both ends present; then the edges, the historical one gray.
    let expected = [
        node(1, "A", true),
        node(2, "B", false),
        node(4, "A2", false),
        node(5, "A3", false),
        "  n1 -> n4 [label=\"revision\", style=dashed, color=blue];".into(),
        "  n1 -> n5 [label=\"revision\", style=dashed, color=blue];".into(),
        edge(1, 2, 3, true),
        edge(4, 2, 6, false),
        edge(2, 5, 7, false),
    ];
    assert_eq!(lines[2..lines.len() - 1], expected, "{}", rendered.text);
    // Staleness is that of the selected boundary: A and its edge to B are
    // current until A2 revises A at position 4.
    for (as_of, stale) in [(4, false), (5, true)] {
        let early = around(&service, ids[2], Some(as_of), RenderFormat::Dot)
            .unwrap()
            .text;
        let lines = early.lines().collect::<Vec<_>>();
        assert_eq!(
            lines[1],
            format!("  label=\"{} boundary {as_of}\";", described.database)
        );
        assert_eq!(
            lines[2..lines.len() - 1],
            [
                node(1, "A", stale),
                node(2, "B", false),
                edge(1, 2, 3, stale)
            ],
            "{early}"
        );
    }
    // Without the root in the result there is no revision link, and no edge
    // leaves the result.
    let revisions = render_dot(
        &service,
        vec![Seed::Ids {
            ids: vec![ids[4], ids[5]],
        }],
        None,
    );
    let lines = revisions.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 5, "{revisions}");
    assert!(lines[2].starts_with("  n4 [") && lines[3].starts_with("  n5 ["));
    assert!(!revisions.contains("revision") && !revisions.contains(" -> "));
    // `as_of` bounds the picture and its label.
    let early = render_dot(&service, vec![Seed::Ids { ids: vec![ids[1]] }], Some(2));
    assert!(early.contains("boundary 2\""), "{early}");
    assert!(early.contains(&node(1, "A", false)), "{early}");
    assert!(matches!(
        service.render_view(ViewRenderRequest {
            query: ViewQueryRequest {
                seeds: vec![Seed::Ids { ids: vec![ids[4]] }],
                expand: vec![],
                edges: EdgeFilter::default(),
                as_of: Some(2),
            },
            format: RenderFormat::Dot,
        }),
        Err(AppError::InvalidSeeds { refused })
            if refused == [RefusedSeed { id: ids[4], reason: SeedRefusal::Absent }]
    ));
    // Through RPC: the query is one member, and the format is required and
    // spelled in lowercase.
    let call = |params: Value| {
        let line = json!({"jsonrpc": "2.0", "id": 1, "method": "views.render", "params": params})
            .to_string();
        serde_json::to_value(rpc::handle(&service, &line)).unwrap()
    };
    let response = call(json!({"query": {"seeds": []}, "format": "dot"}));
    assert_eq!(response["result"]["format"], "dot", "{response}");
    assert!(
        response["result"]["text"]
            .as_str()
            .unwrap()
            .starts_with("digraph lifecore {\n")
    );
    for params in [
        json!({"query": {"seeds": []}}),
        json!({"query": {"seeds": []}, "format": "DOT"}),
        json!({"query": {"seeds": []}, "format": "png"}),
        json!({"query": {"seeds": []}, "format": null}),
        json!({"query": {"seeds": [], "as_of": 1}, "format": "dot"}),
        json!({"seeds": [], "format": "dot"}),
        json!({"query": {"seeds": [], "format": "dot"}, "format": "dot"}),
        json!({"query": null, "format": "dot"}),
        json!({"format": "dot"}),
    ] {
        let response = call(params.clone());
        assert_eq!(response["error"]["code"], -32602, "{params} {response}");
    }
    assert_eq!(
        call(json!({"query": {"seeds": [], "as_of": "9"}, "format": "dot"}))["error"]["code"],
        4002
    );
}

#[test]
fn render_labels_read_like_table_rows_with_the_full_text_as_tooltip() {
    // Labels follow the client's row label: `title`, then `name`, then the
    // first text field, else the type and position, on one line and cut at
    // 60 characters; the tooltip has the uncut text. DOT metacharacters are
    // escaped, control characters blanked, and the empty type name reads
    // "(unnamed)", on edges too.
    let scratch = Scratch::new();
    let service = scratch.service();
    service.init(&[]).unwrap();
    let applied = service
        .apply(request(proposal(json!([
            {"name": "memo", "kind": "node", "ty_name": "Type", "payload": {"name": "Memo"}},
            {"name": "fields", "kind": "node", "ty_name": "Memo",
                "payload": {"zeta": "z", "beta": "b", "title": "  ", "alpha": {"kind": "integer", "value": "1"}}},
            {"name": "bare", "kind": "node", "ty_name": "Memo",
                "payload": {"n": {"kind": "integer", "value": "1"}}},
            {"name": "quoted", "kind": "node", "ty_name": "",
                "payload": {"title": "Say \"hi\"\\\n\tnow"}},
            {"name": "long", "kind": "node", "ty_name": "Memo", "payload": {"title": "x".repeat(70)}},
            {"name": "sixty", "kind": "node", "ty_name": "Memo", "payload": {"title": "y".repeat(60)}},
            {"name": "controls", "kind": "node", "ty_name": "Memo",
                "payload": {"title": "nul\u{0}tab\tcr\r\nlf\n\n&amp;"}},
            {"name": "slashes", "kind": "node", "ty_name": "Memo",
                "payload": {"title": "\\N \\G and a trailing \\"}},
            {"name": "unnamed", "kind": "edge", "ty_name": "",
                "from": {"added": "fields"}, "to": {"added": "bare"}},
        ]))))
        .unwrap();
    let ids = applied
        .report
        .operations
        .iter()
        .map(|operation| operation.entry)
        .filter(|id| id.position() != 8)
        .collect::<Vec<_>>();
    let text = render_dot(&service, vec![Seed::Ids { ids }], None);
    let x59 = "x".repeat(59);
    let x70 = "x".repeat(70);
    let y60 = "y".repeat(60);
    assert_eq!(
        dot_attributes(&text, "label"),
        [
            r#""Memo\n@0 Type""#.to_string(),
            r#""b\n@1 Memo""#.into(),
            r#""Memo @2\n@2 Memo""#.into(),
            r#""Say \"hi\"\\  now\n@3 (unnamed)""#.into(),
            format!("\"{x59}\u{2026}\\n@4 Memo\""),
            format!("\"{y60}\\n@5 Memo\""),
            r#""nul tab cr lf &amp;amp;\n@6 Memo""#.into(),
            r#""\\N \\G and a trailing \\\n@7 Memo""#.into(),
            r#""@8 (unnamed)""#.into(),
        ],
        "{text}"
    );
    assert_eq!(
        dot_attributes(&text, "tooltip"),
        [
            r#""Memo\n@0 Type""#.to_string(),
            r#""b\n@1 Memo""#.into(),
            r#""Memo @2\n@2 Memo""#.into(),
            r#""Say \"hi\"\\\\  now\n@3 (unnamed)""#.into(),
            format!("\"{x70}\\n@4 Memo\""),
            format!("\"{y60}\\n@5 Memo\""),
            r#""nul tab cr lf &amp;amp;\n@6 Memo""#.into(),
            r#""\\\\N \\\\G and a trailing \\\\\n@7 Memo""#.into(),
            r#""@8 (unnamed)""#.into(),
        ],
        "{text}"
    );
    assert_eq!(dot_attributes(&text, "labeltooltip"), [r#""@8 (unnamed)""#]);
    assert!(!text.chars().any(|c| c.is_control() && c != '\n'));
}

#[test]
fn dot_quote_escapes_metacharacters_and_blanks_control_characters() {
    assert_eq!(
        render::dot_quote("a\"b\\N c\\"),
        "\"a\\\"b\\\\N c\\\\\"",
        "quotes and backslashes, a trailing one too"
    );
    assert_eq!(
        render::dot_quote("one\ntwo\rthree"),
        "\"one\\ntwo\\rthree\""
    );
    assert_eq!(
        render::dot_quote("&amp; &#60; &"),
        "\"&amp;amp; &amp;#60; &amp;\""
    );
    assert_eq!(
        render::dot_quote("nul\0tab\tvt\u{b}del\u{7f}c1\u{85}end"),
        "\"nul tab vt del c1 end\""
    );
    assert_eq!(
        render::dot_quote("Ünïcode \u{2026}"),
        "\"Ünïcode \u{2026}\""
    );
}

#[test]
fn graphviz_that_cannot_start_is_unavailable() {
    let missing = render::graphviz(
        Command::new("lifecore-no-such-graphviz"),
        "digraph {}",
        Limits::GRAPHVIZ,
    )
    .unwrap_err();
    assert!(
        matches!(&missing, AppError::GraphvizUnavailable { message } if message.contains("os error")),
        "{missing}"
    );
    assert_eq!(missing.to_object().code, 4004);
    assert_eq!(
        missing.to_object().data.unwrap()["kind"],
        "graphviz_unavailable"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new();
        let file = scratch.0.join("dot");
        fs::write(&file, "not executable").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        // The search path is this directory alone, since the search would
        // go on past a file it may not execute.
        let mut command = Command::new("dot");
        command.env("PATH", &scratch.0);
        let refused = render::graphviz(command, "digraph {}", Limits::GRAPHVIZ).unwrap_err();
        assert!(
            matches!(&refused, AppError::GraphvizUnavailable { message }
                if *message == io::Error::from(io::ErrorKind::PermissionDenied).to_string()
                    || message.contains("os error 13")),
            "{refused}"
        );
        assert_eq!(refused.to_object().code, 4004);
    }
}

// Graphviz stand-ins are shell scripts, so these checks need `/bin/sh` but
// not Graphviz. Each script gets its own directory, which comes first on the
// stand-in's own search path; the test process's environment is untouched.
#[cfg(unix)]
struct StandIn {
    path: std::ffi::OsString,
    // Where a script records the process ID of a descendant it leaves
    // behind, which is stopped on drop.
    descendant: PathBuf,
}

#[cfg(unix)]
impl StandIn {
    fn new(scratch: &Scratch, name: &str, script: &str) -> Option<Self> {
        use std::os::unix::fs::PermissionsExt;
        if !Path::new("/bin/sh").exists() {
            eprintln!("note: /bin/sh is missing; skipping a Graphviz stand-in check");
            return None;
        }
        let dir = scratch.0.join(name);
        fs::create_dir(&dir).unwrap();
        let descendant = dir.join("descendant");
        let file = dir.join("dot");
        let script = script.replace("$DESCENDANT", &format!("'{}'", descendant.display()));
        fs::write(&file, format!("#!/bin/sh\n{script}")).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path =
            std::env::join_paths(std::iter::once(dir).chain(std::env::split_paths(&path))).unwrap();
        Some(Self { path, descendant })
    }

    fn run(&self, dot: &str) -> Result<String, AppError> {
        self.run_within(dot, Limits::GRAPHVIZ)
    }

    fn run_within(&self, dot: &str, limits: Limits) -> Result<String, AppError> {
        // A test forking while another wrote its script can briefly hold the
        // script open for writing in that child, and executing it then fails
        // with ETXTBSY, so such a start is retried.
        for _ in 0..50 {
            let mut command = Command::new("dot");
            command.env("PATH", &self.path);
            match render::graphviz(command, dot, limits) {
            Err(AppError::GraphvizUnavailable { message }) if message.contains("os error 26") => {
                std::thread::sleep(Duration::from_millis(20));
            }
            result => return result,
            }
        }
        panic!("the stand-in stayed busy")
    }
}

#[cfg(unix)]
impl Drop for StandIn {
    fn drop(&mut self) {
        if let Ok(pid) = fs::read_to_string(&self.descendant) {
            Command::new("kill").arg(pid.trim()).status().ok();
        }
    }
}

// The cause and stderr of a Graphviz failure, which its error object
// carries as data beside a message that names both.
#[cfg(unix)]
fn graphviz_failure(result: Result<String, AppError>) -> (GraphvizFailure, String) {
    match result {
    Err(error @ AppError::GraphvizFailed { .. }) => {
        let object = error.to_object();
        assert_eq!(object.code, 4005);
        let AppError::GraphvizFailed { cause, stderr } = error else {
            unreachable!()
        };
        assert_eq!(
            object.data.unwrap(),
            json!({"kind": "graphviz_failed", "cause": cause, "stderr": stderr})
        );
        assert!(
            object.message.contains(&cause.to_string()),
            "{}",
            object.message
        );
        assert!(object.message.ends_with(&stderr), "{}", object.message);
        (cause, stderr)
    }
    other => panic!("expected a Graphviz failure, got {other:?}"),
    }
}

// The program and every pipe are abandoned at the deadline, even with a
// descendant still holding them all and never reading its input.
#[cfg(unix)]
#[test]
fn graphviz_is_killed_at_the_deadline_even_when_its_pipes_stay_open() {
    let scratch = Scratch::new();
    // A background job's stdin is otherwise /dev/null, hence fd 3.
    let Some(dot) = StandIn::new(
        &scratch,
        "hang",
        "exec 3<&0\nsleep 100 <&3 &\necho $! > $DESCENDANT\nexec sleep 100\n",
    ) else {
        return;
    };
    // More input than a pipe buffers, so the writer blocks for good.
    let input = format!("digraph {{}}\n{}", " ".repeat(1 << 20));
    let limits = Limits {
        deadline: Duration::from_millis(300),
        grace: Duration::from_millis(200),
        ..Limits::GRAPHVIZ
    };
    let started = Instant::now();
    let (cause, _) = graphviz_failure(dot.run_within(&input, limits));
    let elapsed = started.elapsed();
    assert_eq!(cause, GraphvizFailure::Deadline { milliseconds: 300 });
    assert_eq!(
        cause.to_string(),
        "dot did not finish within 300 ms and was killed"
    );
    // Past the deadline, the call waits out the grace period for the pipes
    // and no longer: the descendant would hold them for 100 s.
    assert!(elapsed >= limits.deadline, "{elapsed:?}");
    assert!(
        elapsed < limits.deadline + limits.grace + Duration::from_secs(2),
        "{elapsed:?}"
    );
}

#[cfg(unix)]
#[test]
fn graphviz_stand_ins_report_output_exit_and_pipe_failures() {
    let scratch = Scratch::new();
    let stand_in = |name: &str, script: &str| StandIn::new(&scratch, name, script);
    let Some(ok) = stand_in("ok", "cat > /dev/null\nprintf '<svg/>'\n") else {
        return;
    };
    assert_eq!(ok.run("digraph {}\n").unwrap(), "<svg/>");
    // Output past the cap kills the program at once, long before the
    // deadline, at the production cap and at a smaller one.
    let flood = stand_in(
        "flood",
        "cat > /dev/null\nexec yes aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .unwrap();
    let started = Instant::now();
    let (cause, _) = graphviz_failure(flood.run("digraph {}\n"));
    assert_eq!(cause, GraphvizFailure::Oversized { bytes: 16 << 20 });
    assert_eq!(
        cause.to_string(),
        "dot produced more than 16 MiB of SVG and was killed"
    );
    let small = Limits {
        output: 1000,
        ..Limits::GRAPHVIZ
    };
    let (cause, _) = graphviz_failure(flood.run_within("digraph {}\n", small));
    assert_eq!(cause, GraphvizFailure::Oversized { bytes: 1000 });
    assert!(started.elapsed() < Duration::from_secs(5));
    // A failed exit reports its status and stderr.
    let exit = stand_in("exit", "cat > /dev/null\necho 'bad graph' >&2\nexit 3\n").unwrap();
    let (cause, stderr) = graphviz_failure(exit.run("digraph {}\n"));
    assert!(
        matches!(&cause, GraphvizFailure::Exited { status } if status.contains('3')),
        "{cause:?}"
    );
    assert_eq!(stderr, "bad graph");
    // A successful exit whose output a descendant keeps open is incomplete
    // after the grace period.
    let held = stand_in(
        "held",
        "cat > /dev/null\nprintf '<svg/>'\nsleep 100 &\necho $! > $DESCENDANT\n",
    )
    .unwrap();
    let started = Instant::now();
    let (cause, _) = graphviz_failure(held.run("digraph {}\n"));
    assert_eq!(cause, GraphvizFailure::OutputOpen { milliseconds: 500 });
    assert!(started.elapsed() < Duration::from_secs(5));
    // So is a successful exit that left input unread.
    let unread = stand_in("unread", "printf '<svg/>'\n").unwrap();
    let (cause, stderr) = graphviz_failure(unread.run(&" ".repeat(1 << 20)));
    assert!(
        matches!(cause, GraphvizFailure::InputUnread { .. }),
        "{cause:?}"
    );
    assert_eq!(stderr, "");
    // Output that is not UTF-8 is no SVG.
    let binary = stand_in("binary", "cat > /dev/null\nprintf '<svg>\\377</svg>'\n").unwrap();
    let (cause, _) = graphviz_failure(binary.run("digraph {}\n"));
    assert!(
        matches!(cause, GraphvizFailure::InvalidUtf8 { .. }),
        "{cause:?}"
    );
}

// Decodes the XML escapes Graphviz writes.
fn xml_text(escaped: &str) -> String {
    let mut text = String::new();
    let mut rest = escaped;
    while let Some(at) = rest.find('&') {
        text.push_str(&rest[..at]);
        let end = at + rest[at..].find(';').unwrap();
        text.push(match &rest[at + 1..end] {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        numeric => char::from_u32(numeric.strip_prefix('#').unwrap().parse().unwrap()).unwrap(),
        });
        rest = &rest[end + 1..];
    }
    text.push_str(rest);
    text
}

// The tooltips and the text lines, decoded, of the SVG group of one node or
// edge, which ends where the next one starts.
fn svg_element(svg: &str, id: &str) -> (Vec<String>, Vec<String>) {
    let start = svg
        .find(&format!("<g id=\"{id}\" class="))
        .unwrap_or_else(|| panic!("no {id} in {svg}"));
    let body = &svg[start + svg[start..].find('>').unwrap() + 1..];
    let end = [" class=\"node\">", " class=\"edge\">", "</svg>"]
        .iter()
        .filter_map(|marker| body.find(marker))
        .min()
        .unwrap();
    let group = &body[..end];
    let tooltips = group
        .match_indices("xlink:title=\"")
        .map(|(at, marker)| {
            let value = &group[at + marker.len()..];
            xml_text(&value[..value.find('"').unwrap()])
        })
        .collect();
    let lines = group
        .match_indices("<text ")
        .map(|(at, _)| {
            let value = &group[at..];
            let open = value.find('>').unwrap() + 1;
            xml_text(&value[open..value.find("</text>").unwrap()])
        })
        .collect();
    (tooltips, lines)
}

#[test]
#[ignore = "requires Graphviz dot"]
fn render_svg_runs_graphviz_with_clickable_identities() {
    let scratch = Scratch::new();
    let ids = split(&scratch);
    let service = scratch.service();
    let rendered = around(&service, ids[2], None, RenderFormat::Svg).unwrap();
    assert_eq!(rendered.format, RenderFormat::Svg);
    let svg = &rendered.text;
    assert!(svg.contains("<svg"), "{svg}");
    for id in ["n1", "n2", "n4", "n5", "e3", "e6", "e7"] {
        assert!(svg.contains(&format!("id=\"{id}\"")), "{id}");
    }
    assert!(!svg.contains("id=\"n0\""));
    assert!(
        svg.contains(&format!("href=\"#inspect/{}/4\"", ids[4].origin())),
        "{svg}"
    );
    // Tooltips are the plain labels, not their DOT escapes.
    assert_eq!(
        svg_element(svg, "n4"),
        (
            vec!["A2\n@4 Task".into()],
            vec!["A2".into(), "@4 Task".into()]
        )
    );
    assert_eq!(
        svg_element(svg, "e6"),
        (
            vec!["@6 DependsOn".into(), "@6 DependsOn".into()],
            vec!["@6 DependsOn".into()]
        )
    );
    // A DOT syntax error is a failed exit that carries Graphviz's own
    // complaint.
    let failed = render::graphviz(Command::new("dot"), "digraph {", Limits::GRAPHVIZ).unwrap_err();
    let AppError::GraphvizFailed { cause, stderr } = &failed else {
        panic!("expected a Graphviz failure, got {failed}")
    };
    assert!(matches!(cause, GraphvizFailure::Exited { .. }), "{cause:?}");
    assert!(stderr.contains("syntax error"), "{stderr}");
    assert_eq!(failed.to_object().code, 4005);
    assert_eq!(
        failed.to_object().data.unwrap(),
        json!({"kind": "graphviz_failed", "cause": cause, "stderr": stderr})
    );
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "views.render",
        "params": {"query": {"seeds": []}, "format": "svg"}})
    .to_string();
    let response = serde_json::to_value(rpc::handle(&service, &line)).unwrap();
    assert_eq!(response["result"]["format"], "svg", "{response}");
    assert!(
        response["result"]["text"]
            .as_str()
            .unwrap()
            .contains("</svg>")
    );
}

// Payload text reads literally in the picture and its tooltips, whatever
// DOT, Graphviz, or XML would make of it.
#[test]
#[ignore = "requires Graphviz dot"]
fn rendered_svg_shows_payload_text_literally() {
    let scratch = Scratch::new();
    let service = scratch.service();
    service.init(&[]).unwrap();
    let long = format!("{} \\N end", "z".repeat(60));
    // Each title and how it reads: control characters are blanked and line
    // breaks joined.
    let titles = [
        ("nul\u{0}tab\tline\nbreak\r\nend", "nul tab line break end"),
        (
            "Say \"hi\" to \\N, \\G, \\E, \\T, \\H and \\L",
            "Say \"hi\" to \\N, \\G, \\E, \\T, \\H and \\L",
        ),
        ("ends with \\", "ends with \\"),
        (
            "</text><script>alert(1)</script>",
            "</text><script>alert(1)</script>",
        ),
        ("&amp; &#60; & &x; '", "&amp; &#60; & &x; '"),
        (long.as_str(), long.as_str()),
    ];
    let mut entries = titles
        .iter()
        .enumerate()
        .map(|(n, (title, _))| {
            json!({"name": format!("t{n}"), "kind": "node", "ty_name": "Memo",
                "payload": {"title": title}})
        })
        .collect::<Vec<_>>();
    entries.push(json!({"name": "unnamed", "kind": "node", "ty_name": "",
        "payload": {"title": "of no type"}}));
    entries.push(
        json!({"name": "link", "kind": "edge", "ty_name": "Links\\E\"&amp;",
        "from": {"added": "t0"}, "to": {"added": "t1"}}),
    );
    let applied = service
        .apply(request(proposal(Value::Array(entries))))
        .unwrap();
    let ids = applied
        .report
        .operations
        .iter()
        .map(|operation| operation.entry)
        .collect::<Vec<_>>();
    let svg = service
        .render_view(ViewRenderRequest {
            query: ViewQueryRequest {
                seeds: vec![Seed::Ids {
                    ids: ids[..ids.len() - 1].to_vec(),
                }],
                expand: vec![],
                edges: EdgeFilter::default(),
                as_of: None,
            },
            format: RenderFormat::Svg,
        })
        .unwrap()
        .text;
    assert!(!svg.contains("<script"), "{svg}");
    assert!(!svg.chars().any(|c| c.is_control() && c != '\n'), "{svg}");
    for (position, (_, shown)) in titles.iter().enumerate() {
        let identity = format!("@{position} Memo");
        let label = if shown.chars().count() > 60 {
            format!("{}\u{2026}", shown.chars().take(59).collect::<String>())
        } else {
            shown.to_string()
        };
        assert_eq!(
            svg_element(&svg, &format!("n{position}")),
            (vec![format!("{shown}\n{identity}")], vec![label, identity]),
            "{svg}"
        );
    }
    assert_eq!(
        svg_element(&svg, "n6"),
        (
            vec!["of no type\n@6 (unnamed)".into()],
            vec!["of no type".into(), "@6 (unnamed)".into()]
        )
    );
    let edge = "@7 Links\\E\"&amp;".to_string();
    assert_eq!(
        svg_element(&svg, "e7"),
        (vec![edge.clone(), edge.clone()], vec![edge])
    );
}

// `types.list`, `entries.inspect`, and TypeCheck read one declaration index,
// so they agree on every name in each view: before and after a malformed and
// a duplicate declaration, and after the revisions that repair them.
#[test]
fn types_list_and_inspection_agree_with_typecheck_at_each_boundary() {
    use lifecore::analysis::TypedAnalyzer;
    use lifecore_typecheck::{TypeCheck, TypeCheckMessage};
    let scratch = Scratch::new();
    let service = scratch.service();
    service.init(&[]).unwrap();
    let task = |status| {
        Type::new(
            "Task",
            &[("title", (ValueType::Text, status))],
            &["link"],
            &["link"],
            &[],
        )
    };
    let note = Type::new(
        "Note",
        &[("text", (ValueType::Text, FieldStatus::Mandatory))],
        &[],
        &[],
        &[],
    );
    let broken_note = json!({"name": "Note", "fields": "text"});
    let broken_task = json!({"name": "Task", "outgoing": "x"});
    let mut ids = vec![];
    let mut apply = |entries: Value| {
        let applied = service.apply(request(proposal(entries))).unwrap();
        ids.extend(applied.report.operations.iter().map(|o| o.entry));
        ids.clone()
    };
    // Positions 0 to 5: `Task`, two tasks and an edge, a `Note` and a node of
    // the empty type name, neither declared.
    apply(json!([
        {"name": "task_type", "kind": "node", "ty_name": "Type",
            "payload": task(FieldStatus::Mandatory).to_body()},
        {"name": "untitled", "kind": "node", "ty_name": "Task", "payload": {}},
        {"name": "titled", "kind": "node", "ty_name": "Task", "payload": {"title": "t"}},
        {"name": "link", "kind": "edge", "ty_name": "link",
            "from": {"added": "titled"}, "to": {"added": "untitled"}},
        {"name": "a_note", "kind": "node", "ty_name": "Note", "payload": {}},
        {"name": "unnamed", "kind": "node", "ty_name": "", "payload": {}},
    ]));
    // 6 and 7: a malformed `Note` declaration and a nameless one.
    apply(json!([
        {"name": "broken_note", "kind": "node", "ty_name": "Type", "payload": broken_note},
        {"name": "nameless", "kind": "node", "ty_name": "Type", "payload": nameless_declaration()},
    ]));
    // 8 and 9: a second `Task` declaration and a malformed third.
    let before = apply(json!([
        {"name": "task_again", "kind": "node", "ty_name": "Type",
            "payload": task(FieldStatus::Optional).to_body()},
        {"name": "broken_task", "kind": "node", "ty_name": "Type", "payload": broken_task},
    ]));
    // 10 to 12: the repairs.
    let ids = apply(json!([
        {"name": "note_type", "kind": "node", "ty_name": "Type", "predecessor": existing(before[6]),
            "intent": "revise", "payload": note.to_body()},
        {"name": "other", "kind": "node", "ty_name": "Type", "predecessor": existing(before[8]),
            "intent": "revise", "payload": Type::new("Other", &[], &[], &[], &[]).to_body()},
        {"name": "spare", "kind": "node", "ty_name": "Type", "predecessor": existing(before[9]),
            "intent": "revise", "payload": Type::new("Spare", &[], &[], &[], &[]).to_body()},
    ]));
    let graph = storage::read(&scratch.state()).unwrap().graph;
    let note_error = SchemaError {
        path: vec![PathPart::Field("fields".into())],
        kind: SchemaErrorKind::WrongShape {
            expected: lifecore::PayloadKind::Record,
            actual: lifecore::PayloadKind::Text,
        },
    };
    let task_error = schema_error(json!({"name": "Task", "outgoing": "x"}));
    let declared = |at: usize, ty: &Type| TypeResolution::Declared {
        declaration: ids[at],
        schema: ty.clone(),
    };
    let cases = [
        (
            6,
            vec![
                ("", TypeResolution::Missing),
                ("Note", TypeResolution::Missing),
                ("Task", declared(0, &task(FieldStatus::Mandatory))),
            ],
        ),
        // A malformed `Note` leaves `Task` declared, and the nameless
        // declaration leaves the empty name missing.
        (
            8,
            vec![
                ("", TypeResolution::Missing),
                (
                    "Note",
                    TypeResolution::Malformed {
                        declaration: ids[6],
                        error: note_error.clone(),
                    },
                ),
                ("Task", declared(0, &task(FieldStatus::Mandatory))),
            ],
        ),
        (
            10,
            vec![
                ("", TypeResolution::Missing),
                (
                    "Task",
                    TypeResolution::Ambiguous {
                        claimants: vec![
                            Declaration::Parsed {
                                declaration: ids[0],
                                schema: task(FieldStatus::Mandatory),
                            },
                            Declaration::Parsed {
                                declaration: ids[8],
                                schema: task(FieldStatus::Optional),
                            },
                            Declaration::Malformed {
                                declaration: ids[9],
                                error: task_error.clone(),
                            },
                        ],
                    },
                ),
            ],
        ),
        (
            13,
            vec![
                ("Note", declared(10, &note)),
                ("Task", declared(0, &task(FieldStatus::Mandatory))),
            ],
        ),
    ];
    for (end, expected) in cases {
        let as_of = Some(end);
        let listed = service.list_types(TypeListRequest { as_of }).unwrap();
        let view = graph.view().prefix(end).unwrap();
        let report = TypeCheck::default()
            .analyze_inner(view, &Default::default())
            .unwrap();
        assert!(listed.types.iter().all(
            |summary| !summary.name.is_empty() || summary.resolution == TypeResolution::Missing
        ));
        for (name, resolution) in expected {
            assert_eq!(
                summary(&listed, name).resolution,
                resolution,
                "{name:?} at {end}"
            );
        }
        for summary in &listed.types {
            assert_eq!(summary.resolution, view.resolve_type(&summary.name));
            match &summary.resolution {
            TypeResolution::Malformed { declaration, error } => {
                assert!(report.contains(&TypeCheckMessage::MalformedDeclaration {
                    subject: *declaration,
                    error: error.clone(),
                }));
            }
            TypeResolution::Ambiguous { claimants } => {
                for (i, claimant) in claimants.iter().enumerate() {
                    let duplicate = TypeCheckMessage::DuplicateType {
                        subject: claimant.id(),
                        name: summary.name.clone(),
                    };
                    assert_eq!(report.contains(&duplicate), i > 0, "{claimant:?}");
                    if let Err(error) = claimant.schema() {
                        assert!(report.contains(&TypeCheckMessage::MalformedDeclaration {
                            subject: claimant.id(),
                            error: error.clone(),
                        }));
                    }
                }
            }
            _ => {}
            }
        }
        for at in [1, 2, 4, 5] {
            let ty = node_type(&service, ids[at], as_of);
            let node = view.node(ids[at]).unwrap();
            assert_eq!(ty, summary(&listed, &node.ty_name).resolution);
            let about = |f: &&TypeCheckMessage| match f {
            TypeCheckMessage::MissingType { subject, .. }
            | TypeCheckMessage::Field { subject, .. } => *subject == ids[at],
            _ => false,
            };
            let findings = report.iter().filter(about).collect::<Vec<_>>();
            match ty {
            TypeResolution::Missing => assert!(matches!(
                findings[..],
                [TypeCheckMessage::MissingType { .. }]
            )),
            TypeResolution::Declared { .. } => assert!(
                findings
                    .iter()
                    .all(|f| matches!(f, TypeCheckMessage::Field { .. }))
            ),
            _ => assert!(findings.is_empty(), "{at} at {end}: {findings:?}"),
            }
        }
        // The untitled task misses its title only while `Task` is declared,
        // and the edge between tasks is judged only then.
        let untitled = report
            .iter()
            .any(|f| matches!(f, TypeCheckMessage::Field { subject, .. } if *subject == ids[1]));
        assert_eq!(untitled, end != 10, "boundary {end}");
        assert!(
            !report
                .iter()
                .any(|f| matches!(f, TypeCheckMessage::BadEdgeEndpoint { .. }))
        );
    }
}

// The declaration of CORE-ROUND 2.4 and a node using every new kind, in TOML
// with non-canonical spellings.
const KINDS_TOML: &str = r#"
[[entries]]
name = "stance_type"
kind = "node"
ty_name = "Type"

[entries.payload]
name = "Stance"
fields = { kind = "record", value = {
  cost     = ["optional", "duration"],
  deadline = ["optional", "date"],
  at       = ["mandatory", "offset_timestamp"],
  per      = ["mandatory", "enum", ["window", "day", "week"]],
  decision = ["mandatory", "variant", { kind = "record", value = {
    committed = { kind = "record", value = { cost = ["mandatory", "duration"] } },
    deferred  = { kind = "record", value = { horizon = ["mandatory", "integer"] } },
    resumed   = { kind = "record", value = {} },
  } }],
} }

[[entries]]
name = "stance"
kind = "node"
ty_name = "Stance"

[entries.payload]
cost = { kind = "duration", value = "90m" }
deadline = { kind = "date", value = "2026-11-06" }
at = { kind = "offset_timestamp", value = "2026-10-29t08:15:00.000+01:00" }
per = "day"
decision = { kind = "variant", tag = "committed", value = { cost = { kind = "duration", value = "1h 30m" } } }
"#;

#[test]
fn change_files_carry_the_new_kinds_in_both_formats_and_export_them_canonically() {
    let from_toml = proposal::decode(KINDS_TOML, Format::Toml).unwrap();
    let json_text = proposal::encode(&from_toml, Format::Json).unwrap();
    assert_eq!(
        proposal::decode(&json_text, Format::Json).unwrap(),
        from_toml
    );
    let exported: Value = serde_json::from_str(&json_text).unwrap();
    let payload = &exported["entries"][1]["payload"];
    assert_eq!(
        *payload,
        json!({
            "cost": {"kind": "duration", "value": "PT1H30M"},
            "deadline": {"kind": "date", "value": "2026-11-06"},
            "at": {"kind": "offset_timestamp", "value": "2026-10-29T08:15:00+01:00"},
            "per": "day",
            "decision": {"kind": "variant", "tag": "committed",
                "value": {"cost": {"kind": "duration", "value": "PT1H30M"}}},
        })
    );
    assert_eq!(
        exported["entries"][0]["payload"]["fields"]["value"]["per"],
        json!(["mandatory", "enum", ["window", "day", "week"]])
    );
    let toml_text = proposal::encode(&from_toml, Format::Toml).unwrap();
    assert_eq!(
        proposal::decode(&toml_text, Format::Toml).unwrap(),
        from_toml
    );
    assert!(toml_text.contains("PT1H30M"), "{toml_text}");

    // TypeCheck accepts the node, then rejects a member outside the enum,
    // an unknown tag, and a tag's missing field, each with its path.
    let scratch = Scratch::new();
    scratch.service().init(&typecheck()).unwrap();
    let service = scratch.service();
    let check = |proposal: Proposal| service.check(request(proposal)).unwrap();
    assert!(check(from_toml.clone()).accepted);
    for (field, replacement, message) in [
        (
            "per",
            json!("month"),
            r#"field per is "month", which its enum does not list"#,
        ),
        (
            "decision",
            json!({"kind": "variant", "tag": "dropped", "value": {}}),
            r#"field decision has tag "dropped", which its variant does not declare"#,
        ),
        (
            "decision",
            json!({"kind": "variant", "tag": "deferred", "value": {}}),
            "mandatory field decision(deferred).horizon is missing",
        ),
    ] {
        let mut changed = serde_json::to_value(&from_toml).unwrap();
        changed["entries"][1]["payload"][field] = replacement;
        let checked = check(serde_json::from_value(changed).unwrap());
        assert!(!checked.accepted, "{field}");
        assert!(
            checked
                .report
                .analysis
                .observations()
                .any(|observation| observation.diagnostic.message == message),
            "{message}"
        );
    }
    // Native TOML dates are not date payloads, and time text is refused
    // where Jiff's or LifeCore's rules do.
    for (field, text) in [
        ("deadline", "deadline = 2026-11-06"),
        (
            "deadline",
            r#"deadline = { kind = "date", value = "20261106" }"#,
        ),
        ("cost", r#"cost = { kind = "duration", value = "1d" }"#),
        (
            "at",
            r#"at = { kind = "offset_timestamp", value = "2026-10-29T07:15:00Z" }"#,
        ),
        ("decision", r#"decision = { kind = "variant", value = {} }"#),
    ] {
        let line = KINDS_TOML
            .lines()
            .find(|line| line.starts_with(&format!("{field} ")))
            .unwrap();
        let file = KINDS_TOML.replace(line, text);
        assert!(proposal::decode(&file, Format::Toml).is_err(), "{text}");
    }
}

#[test]
fn now_takes_the_offset_of_a_known_zone_and_refuses_an_unknown_one() {
    use crate::service::capture_in;
    use jiff::tz::{Offset, TimeZone};
    use lifecore::Timestamp;
    let instant: Timestamp = "2026-07-01T10:00:00Z".parse().unwrap();
    let winter: Timestamp = "2026-01-01T10:00:00Z".parse().unwrap();
    // A rule with daylight saving: the offset is the one in effect at the
    // captured instant.
    let berlin = || TimeZone::posix("CET-1CEST,M3.5.0,M10.5.0/3");
    let summer = capture_in(berlin(), instant).unwrap();
    assert_eq!(summer.instant(), instant);
    assert_eq!(summer.offset(), Offset::constant(2));
    assert_eq!(summer.to_string(), "2026-07-01T12:00:00+02:00");
    assert_eq!(
        capture_in(berlin(), winter).unwrap().offset(),
        Offset::constant(1)
    );
    let india = TimeZone::fixed(Offset::from_seconds(5 * 3600 + 30 * 60).unwrap());
    assert_eq!(
        capture_in(Ok(india), instant).unwrap().to_string(),
        "2026-07-01T15:30:00+05:30"
    );
    assert_eq!(
        capture_in(Ok(TimeZone::UTC), instant).unwrap().to_string(),
        "2026-07-01T10:00:00+00:00"
    );
    // A zone the host cannot determine is not read as UTC, nor is an offset
    // RFC 3339 cannot write rounded.
    for zone in [
        TimeZone::posix("not a rule"),
        Ok(TimeZone::unknown()),
        Ok(TimeZone::fixed(Offset::from_seconds(3600 + 30).unwrap())),
    ] {
        let error = capture_in(zone, instant).unwrap_err();
        assert!(
            matches!(error, AppError::LocalTimeUnknown { .. }),
            "{error}"
        );
        let object = error.to_object();
        assert_eq!(object.code, 1009);
        assert_eq!(object.data.as_ref().unwrap()["kind"], "local_time_unknown");
        assert!(object.data.unwrap()["message"].is_string());
    }
}
