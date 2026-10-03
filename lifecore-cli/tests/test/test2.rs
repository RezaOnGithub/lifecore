use serde_json::Value;
use std::{
    fs::{self, File},
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
};

const BINARY: &str = env!("CARGO_BIN_EXE_lifecore");

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "lifecore-cli-binary-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

//A Runs the built binary with stdin on a file holding two request lines. The
//A child shares the file offset with this process, so whatever the child did
//A not read is still here afterwards.
#[test]
fn rpc_leaves_the_line_after_its_request_unread() {
    let scratch = Scratch::new();
    let state = scratch.0.join("LifeCoreState.json");
    let init = Command::new(BINARY)
        .arg("init")
        .env("LIFECORE_STATE_FILE_PATH", &state)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let first = r#"{"jsonrpc":"2.0","id":1,"method":"plan_id.mint","params":{}}"#;
    let second = r#"{"jsonrpc":"2.0","id":2,"method":"proposal.apply","params":{"proposal":{"entries":[{"name":"n","kind":"node","type":"Type","payload":{}}]}}}"#;
    let requests = scratch.0.join("requests.jsonl");
    fs::write(&requests, format!("{first}\n{second}\n")).unwrap();
    let before = fs::read(&state).unwrap();
    let mut input = File::open(&requests).unwrap();
    let rpc = Command::new(BINARY)
        .arg("rpc")
        .env("LIFECORE_STATE_FILE_PATH", &state)
        .stdin(Stdio::from(input.try_clone().unwrap()))
        .output()
        .unwrap();
    assert!(rpc.status.success(), "{rpc:?}");
    let output = String::from_utf8(rpc.stdout).unwrap();
    assert_eq!(output.lines().count(), 1);
    let response: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(response["id"], 1);
    assert!(response["result"]["plan_id"].is_string(), "{response}");
    let mut rest = String::new();
    input.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, format!("{second}\n"));
    //A The second request was neither read nor executed.
    assert_eq!(fs::read(&state).unwrap(), before);
}

//A A preview that is not accepted is a result on stdout, but the dedicated
//A command still reports it through its exit status.
#[test]
fn check_exits_nonzero_for_a_proposal_that_is_not_accepted() {
    let scratch = Scratch::new();
    let state = scratch.0.join("LifeCoreState.json");
    let run = |args: &[&str]| {
        Command::new(BINARY)
            .args(args)
            .env("LIFECORE_STATE_FILE_PATH", &state)
            .output()
            .unwrap()
    };
    assert!(run(&["init"]).status.success());
    let empty = scratch.0.join("empty.json");
    fs::write(&empty, r#"{"entries":[]}"#).unwrap();
    let untyped = scratch.0.join("untyped.toml");
    fs::write(
        &untyped,
        "[[entries]]\nname = \"n\"\nkind = \"node\"\ntype = \"Undeclared\"\npayload = {}\n",
    )
    .unwrap();
    for (file, accepted) in [(&empty, true), (&untyped, false)] {
        let output = run(&["check", file.to_str().unwrap()]);
        assert_eq!(output.status.success(), accepted, "{output:?}");
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["accepted"], accepted, "{result}");
        assert!(result["report"]["analysis"].is_object());
    }
}
