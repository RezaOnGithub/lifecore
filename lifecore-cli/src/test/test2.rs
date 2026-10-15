use super::super::{factories, run_rpc};
use lifecore_app::{AppError, Service, storage::StatePath};
use serde_json::Value;
use std::{fs, io::Cursor, path::PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("lifecore-cli-{}", uuid()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn service(&self) -> Service {
        Service::new(
            StatePath::new(self.0.join("LifeCoreState.json")),
            Box::new(factories),
        )
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn uuid() -> String {
    format!(
        "{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn call(input: &str, service: impl FnOnce() -> Result<Service, AppError>) -> (bool, String, u64) {
    let mut reader = Cursor::new(input.as_bytes().to_vec());
    let mut output = vec![];
    let success = run_rpc(&mut reader, &mut output, service).unwrap();
    (
        success,
        String::from_utf8(output).unwrap(),
        reader.position(),
    )
}

#[test]
fn rpc_executes_exactly_one_line_and_prints_one_compact_response() {
    let scratch = Scratch::new();
    scratch.service().init(&[]).unwrap();
    let first = r#"{"jsonrpc":"2.0","id":1,"method":"plan_id.mint","params":{}}"#;
    let second = r#"{"jsonrpc":"2.0","id":2,"method":"proposal.apply","params":{"proposal":{"entries":[{"name":"n","kind":"node","ty_name":"T","payload":{}}]}}}"#;
    let before = fs::read(scratch.0.join("LifeCoreState.json")).unwrap();
    let (success, output, consumed) =
        call(&format!("{first}\n{second}\n"), || Ok(scratch.service()));
    assert!(success);
    // The second line is neither read past nor executed.
    assert_eq!(consumed, first.len() as u64 + 1);
    assert_eq!(output.matches('\n').count(), 1);
    assert!(output.ends_with('\n'));
    let response: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(response["id"], 1);
    assert!(
        response["result"]["plan_id"]
            .as_str()
            .unwrap()
            .starts_with("lc1:")
    );
    assert_eq!(
        fs::read(scratch.0.join("LifeCoreState.json")).unwrap(),
        before
    );
    // EOF can terminate the only line.
    let (success, output, _) = call(first, || Ok(scratch.service()));
    assert!(success);
    assert_eq!(output.lines().count(), 1);
}

#[test]
fn rpc_reports_malformed_input_and_selection_failures_as_one_error_line() {
    let (success, output, _) = call("", || unreachable!("no request to dispatch"));
    assert!(!success);
    let response: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(response["error"]["code"], -32700);
    assert_eq!(response["id"], Value::Null);
    let (success, output, _) = call(
        "{\"jsonrpc\":\"2.0\",\"id\":\"sel\",\"method\":\"database.describe\",\"params\":{}}\r\n",
        || Err(AppError::EmptyStatePath),
    );
    assert!(!success);
    let response: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(response["id"], "sel");
    assert_eq!(response["error"]["data"]["kind"], "state_path_empty");
    let missing = Scratch::new();
    let (success, output, _) = call(
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"entries.list\",\"params\":{}}\n",
        || Ok(missing.service()),
    );
    assert!(!success);
    let response: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(response["error"]["data"]["kind"], "database_missing");
    assert!(!missing.0.join("LifeCoreState.json").exists());
}
