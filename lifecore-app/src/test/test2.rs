use crate::proposal::{self, Format, PlanId, Proposal};
use lifecore::{Body, Payload, Timestamp};
use serde_json::{Value, json};

const FILE_JSON: &str = r#"{
  "format_version": 1,
  "plan_id": "lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc",
  "entries": [
    {"name": "task_done", "kind": "node", "type": "Task", "intent": "revise",
     "predecessor": {"existing": {"origin": "11111111-1111-4111-8111-111111111111", "position": "42"}},
     "payload": {"title": "Write proposal", "status": "done", "priority": 9223372036854775807,
       "deadline": {"kind": "timestamp", "value": "2026-09-30T17:00:00.000000001Z"},
       "tags": [], "details": {"kind": "record", "value": {"kind": "integer", "value": "1"}},
       "rows": [{"kind": "record", "value": {}}, [], -9223372036854775808],
       "done": {"kind": "boolean", "value": true}}},
    {"name": "explanation_link", "kind": "edge", "type": "Explains",
     "from": {"added": "task_done"}, "to": {"added": "task_done"}}
  ]
}"#;

const FILE_TOML: &str = r#"
format_version = 1
plan_id = "lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc"

[[entries]]
name = "task_done"
kind = "node"
type = "Task"
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
type = "Explains"
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
    //A A record whose fields look like an envelope stays a record.
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
        //A Exports always use integer envelopes, never the bare shorthand.
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
        "kind": "Suggestion",
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
        //A A native TOML datetime is not timestamp payload syntax.
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\npayload = { at = 2026-09-30T17:00:00Z }\n",
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\npayload = { n = 9223372036854775808 }\n",
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\npayload = { n = 1.5 }\n",
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\npayload = { n = { kind = \"integer\", value = 1 } }\n",
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\npayload = { n = { kind = \"float\", value = \"1\" } }\n",
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\npayload = {}\ncolor = \"red\"\n",
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\n",
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"edge\"\ntype = \"T\"\nfrom = { added = \"x\" }\nto = { added = \"y\" }\npayload = {}\n",
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"delete\"\ntype = \"T\"\n",
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\npayload = {}\nintent = \"replace\"\n",
        "format_version = 2\nentries = []\n",
        "format_version = 1\n",
        "format_version = 1\nentries = []\ndatabase = \"x\"\n",
        "format_version = 1\nplan_id = \"lc2:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc\"\nentries = []\n",
        "format_version = 1\nplan_id = \"lc1:AAAAAAAA-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc\"\nentries = []\n",
        "format_version = 1\nplan_id = \"lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb\"\nentries = []\n",
        "format_version = 1\nplan_id = \"\"\nentries = []\n",
        "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\npayload = {}\npredecessor = { existing = { origin = \"11111111-1111-4111-8111-111111111111\", position = 42 } }\n",
    ];
    for text in toml_cases {
        let error = proposal::decode(text, Format::Toml).unwrap_err();
        assert!(error.line.is_some(), "no location for {text}: {error}");
    }
    let error = proposal::decode(toml_cases[0], Format::Toml).unwrap_err();
    assert_eq!(error.line, Some(6));
    //A Plain timestamp-like and digit strings remain text.
    let text = "format_version = 1\n[[entries]]\nname = \"a\"\nkind = \"node\"\ntype = \"T\"\npayload = { at = \"2026-09-30T17:00:00Z\", n = \"42\" }\n";
    let decoded = proposal::decode(text, Format::Toml).unwrap();
    let proposal::ProposalEntry::Node { payload, .. } = &decoded.entries[0] else {
        panic!("expected a node")
    };
    assert_eq!(payload["at"], Payload::Text("2026-09-30T17:00:00Z".into()));
    assert_eq!(payload["n"], Payload::Text("42".into()));
    let error =
        proposal::decode("{\"format_version\": 1,\n \"entries\": [}", Format::Json).unwrap_err();
    assert_eq!(error.line, Some(2));
    for text in [
        r#"{"format_version":1,"entries":[{"name":"a","kind":"node","type":"T","payload":{"n":18446744073709551615}}]}"#,
        r#"{"format_version":1,"entries":[{"name":"a","kind":"node","type":"T","payload":{"n":null}}]}"#,
        r#"{"format_version":1,"entries":[{"name":"a","kind":"node","type":"T","payload":{"n":1,"n":2}}]}"#,
        r#"{"format_version":1,"plan_id":null,"entries":[]}"#,
        r#"{"format_version":1,"entries":[{"name":"a","kind":"node","type":"T","payload":{},"predecessor":null}]}"#,
    ] {
        assert!(
            proposal::decode(text, Format::Json).is_err(),
            "accepted {text}"
        );
    }
}
