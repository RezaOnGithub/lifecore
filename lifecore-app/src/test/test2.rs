use crate::{
    AppError, Service,
    proposal::{self, Format, PlanId, Proposal},
    query::{EdgeFilter, EntryInspection, Expansion, Incidence, Provenance, Seed, ViewQuery},
    rpc,
    schema::{self, NodeCounts, TypeResolution},
    service::{
        AddAnalyzerRequest, EntryGetRequest, EntryInspectRequest, EntryListRequest, EntryRecord,
        NoParams, ProposalRequest, Published, TypeListRequest, ViewQueryRequest,
    },
    storage::{self, DEFAULT_STATE_FILE, Publication, Snapshot, StatePath},
};
use lifecore::{
    Body, Entry, EntryId, LifeCoreList, Node, Payload, Timestamp,
    analysis::{AnalysisRunner, AnalyzerFactory, Config, EvaluatedView, MessageType},
    inspection::{Currentness, Direction},
    transaction::{self, Change, EntryChange, NewEdge, NewNode, RevisionIntent},
    typing::{FieldDecl, FieldStatus, Type, ValueType},
};
use lifecore_dag_check::DagCheckFactory;
use lifecore_typecheck::TypeCheckFactory;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    time::Duration,
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

    let bare = Scratch::new();
    let initialized = bare.service().init(&[]).unwrap();
    assert!(initialized.suite.0.is_empty());
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
            "issue": {"path": [{"Field": "notes"}], "kind": "MissingEncouraged"}}})
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
    let owner = ValueType::Record(
        [(
            "owner".into(),
            FieldDecl {
                ty: ValueType::Text,
                status: FieldStatus::Mandatory,
            },
        )]
        .into(),
    );
    let list = |element| ValueType::List(Box::new(element));
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
                "path": [{"Field": "details"}, {"Field": "owner"}], "kind": "MissingMandatory"}}}),
            json!({"Field": {"subject": entry, "issue": {
                "path": [{"Field": "grid"}, {"Index": 0}, {"Index": 0}],
                "kind": {"TypeMismatch": {"expected": "Integer", "actual": "Text"}}}}}),
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
        analyzer: lifecore::analysis::AnalyzerIdentity {
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
    let at: Timestamp = "2026-01-02T03:04:05.000000006Z".parse().unwrap();
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
    let note =
        |fields: Value| json!({"name": "Note", "fields": {"kind": "record", "value": fields}});
    let applied = service
        .apply(request(proposal(json!([
            {"name": "note", "kind": "node", "ty_name": "Type",
                "payload": note(json!({"text": ["mandatory", "text"]}))},
            {"name": "note_again", "kind": "node", "ty_name": "Type",
                "payload": note(json!({"text": ["optional", "list", "text"]}))},
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

fn tag_declaration() -> Value {
    json!({"name": "Tag", "fields": "text"})
}

fn nameless_declaration() -> Value {
    json!({"fields": {"kind": "record", "value": {}}})
}

// The display text a malformed declaration payload fails with.
fn schema_error(payload: Value) -> String {
    let body: Body = serde_json::from_value(payload).unwrap();
    Type::from_body(&body).unwrap_err().to_string()
}

#[test]
fn type_schemas_are_the_structured_form_of_declarations() {
    let list = |element| ValueType::List(Box::new(element));
    let owner = ValueType::Record(
        [(
            "owner".into(),
            FieldDecl {
                ty: ValueType::Text,
                status: FieldStatus::Mandatory,
            },
        )]
        .into(),
    );
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
    let schema = schema::TypeSchema::from(&nested);
    assert_eq!(schema.name, "Nested");
    assert_eq!(
        schema.fields["grid"],
        schema::FieldDecl {
            status: schema::FieldStatus::Optional,
            shape: schema::Shape::List {
                element: Box::new(schema::Shape::List {
                    element: Box::new(schema::Shape::Integer)
                })
            },
        }
    );
    assert_eq!(
        schema.fields["details"],
        schema::FieldDecl {
            status: schema::FieldStatus::Deprecated,
            shape: schema::Shape::Record {
                fields: BTreeMap::from([(
                    "owner".to_string(),
                    schema::FieldDecl {
                        status: schema::FieldStatus::Mandatory,
                        shape: schema::Shape::Text,
                    }
                )])
            },
        }
    );
    assert_eq!(schema.fields["when"].shape, schema::Shape::Timestamp);
    assert_eq!(schema.fields["done"].shape, schema::Shape::Boolean);
    // Permissions keep their set order; shapes and statuses are tagged words.
    assert_eq!(
        serde_json::to_value(&schema).unwrap(),
        json!({
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
        })
    );
    let round_trip: schema::TypeSchema =
        serde_json::from_value(serde_json::to_value(&schema).unwrap()).unwrap();
    assert_eq!(round_trip, schema);
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
    assert_eq!(
        *schema,
        schema::TypeSchema::from(&Type::from_body(&task_type()).unwrap())
    );
    assert_eq!(
        schema.fields["title"],
        schema::FieldDecl {
            status: schema::FieldStatus::Mandatory,
            shape: schema::Shape::Text,
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
    assert_eq!(names, ["", "Ghost", "Note", "Tag", "Type"]);
    let summary = |name: &str| listed.types.iter().find(|t| t.name == name).unwrap();
    let one = NodeCounts {
        current: 1,
        historical: 0,
    };
    assert_eq!(
        summary("").resolution,
        TypeResolution::Malformed {
            declaration: ids[5],
            message: schema_error(nameless_declaration()),
        }
    );
    assert_eq!(summary("").nodes, NodeCounts::default());
    assert_eq!(
        summary("Tag").resolution,
        TypeResolution::Malformed {
            declaration: ids[4],
            message: schema_error(tag_declaration()),
        }
    );
    assert_eq!(
        summary("Note").resolution,
        TypeResolution::Ambiguous {
            declarations: vec![ids[0], ids[1]],
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
    }) => (view, entry, ty, lineage, edges),
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
    let (view, entry, ty, lineage, edges) = node(ids[2], None);
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
                incident.other.entry.id(),
                incident.other.current,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        incident,
        [
            (ids[3], false, Incidence::Incoming, ids[1], false),
            (ids[6], true, Incidence::Incoming, ids[4], true),
            (ids[7], true, Incidence::Outgoing, ids[5], true),
        ]
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
        let (view, _, _, lineage, _) = node(id, as_of);
        assert_eq!(view.end_exclusive, as_of.unwrap_or(8));
        assert_eq!(positions(&lineage), expected, "{as_of:?}");
    }
    // The superseded root and its edge are historical now, current before.
    let (_, entry, _, _, edges) = node(ids[1], None);
    assert!(!entry.current);
    assert_eq!(
        edges
            .iter()
            .map(|e| (e.edge.entry.id(), e.edge.current, e.direction))
            .collect::<Vec<_>>(),
        [(ids[3], false, Incidence::Outgoing)]
    );
    let (view, entry, _, _, edges) = node(ids[1], Some(4));
    assert_eq!(view.end_exclusive, 4);
    assert!(entry.current);
    assert_eq!(
        (
            edges[0].edge.current,
            edges[0].other.entry.id(),
            edges[0].other.current
        ),
        (true, ids[2], true)
    );
    // Just after the revision, A is stale but the later edges are not visible.
    let (_, entry, _, _, edges) = node(ids[2], Some(5));
    assert!(entry.current);
    assert_eq!(
        edges
            .iter()
            .map(|e| (e.edge.entry.id(), e.edge.current))
            .collect::<Vec<_>>(),
        [(ids[3], false)]
    );
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
    assert_eq!(result["edges"][0]["other"]["kind"], "node");
    let line =
        json!({"jsonrpc": "2.0", "id": 2, "method": "entries.inspect", "params": {"id": ids[3]}})
            .to_string();
    let response = serde_json::to_value(rpc::handle(&service, &line)).unwrap();
    assert_eq!(response["result"]["kind"], "edge");
    assert_eq!(response["result"]["from"]["kind"], "node");
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
                declarations: vec![ids[0], ids[1]],
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
            message: schema_error(tag_declaration()),
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
    let old_schema = schema::TypeSchema::from(&Type::from_body(&task_type()).unwrap());
    let new_schema = schema::TypeSchema::from(&revised);
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
        schema: schema::TypeSchema::from(ty),
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
    // Boundaries past the end and seeds the view lacks or that are edges are
    // errors.
    assert!(matches!(
        query(seeds(&[ids[2]]), vec![], EdgeFilter::default(), Some(9)),
        Err(AppError::BeyondBoundary {
            requested: 9,
            visible: 8
        })
    ));
    assert!(matches!(
        query(seeds(&[ids[5]]), vec![], EdgeFilter::default(), Some(5)),
        Err(AppError::EntryNotFound { id }) if id == ids[5]
    ));
    assert!(matches!(
        query(seeds(&[ids[3]]), vec![], EdgeFilter::default(), None),
        Err(AppError::EntryNotFound { id }) if id == ids[3]
    ));
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
