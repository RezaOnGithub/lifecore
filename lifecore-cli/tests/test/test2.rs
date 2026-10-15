use serde_json::Value;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpStream},
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

// Runs the built binary with stdin on a file holding two request lines. The
// child shares the file offset with this process, so whatever the child did
// not read is still here afterwards.
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
    // The second request was neither read nor executed.
    assert_eq!(fs::read(&state).unwrap(), before);
}

// A preview that is not accepted is a result on stdout, but the dedicated
// command still reports it through its exit status.
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

// Sends one HTTP/1.1 request and returns the status line and body.
fn post(port: u16, headers: &[String], body: &str) -> (String, Value) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let mut request = format!("POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    for header in headers {
        request.push_str(&format!("{header}\r\n"));
    }
    request.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (
        head.lines().next().unwrap().to_string(),
        serde_json::from_str(body).unwrap(),
    )
}

// The URL with its launch token is the only stdout line; the server listens
// on loopback, answers a valid call, refuses one without the token before
// dispatch, and exits cleanly on SIGTERM.
#[test]
fn serve_prints_a_loopback_url_and_stops_on_sigterm() {
    let scratch = Scratch::new();
    let state = scratch.0.join("LifeCoreState.json");
    let init = Command::new(BINARY)
        .arg("init")
        .env("LIFECORE_STATE_FILE_PATH", &state)
        .output()
        .unwrap();
    let database = serde_json::from_slice::<Value>(&init.stdout).unwrap()["database"].clone();
    let mut server = Command::new(BINARY)
        .args(["serve", "--ui-dir"])
        .arg(&scratch.0)
        .env("LIFECORE_STATE_FILE_PATH", &state)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(server.stdout.take().unwrap());
    let mut url = String::new();
    stdout.read_line(&mut url).unwrap();
    let (origin, token) = url.trim().split_once("/#token=").unwrap();
    let port: u16 = origin
        .strip_prefix("http://127.0.0.1:")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(port, 0);
    assert_eq!(token.len(), 64, "{url}");
    let describe = r#"{"jsonrpc":"2.0","id":1,"method":"database.describe"}"#;
    let json = "Content-Type: application/json".to_string();
    let (status, body) = post(
        port,
        &[format!("Authorization: Bearer {token}"), json.clone()],
        describe,
    );
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert_eq!(body["result"]["database"], database);
    let (status, body) = post(port, &[json], describe);
    assert_eq!(status, "HTTP/1.1 401 Unauthorized");
    assert_eq!(body["kind"], "token");
    let killed = Command::new("kill")
        .args(["-TERM", &server.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    assert!(server.wait().unwrap().success());
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert_eq!(rest, "");
}

// The database is resolved and read at startup; without one, `serve` prints
// the error object and no URL.
#[test]
fn serve_refuses_to_start_without_a_database() {
    let scratch = Scratch::new();
    let output = Command::new(BINARY)
        .arg("serve")
        .env("LIFECORE_STATE_FILE_PATH", scratch.0.join("missing.json"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["code"], 1002);
    assert_eq!(error["data"]["kind"], "database_missing");
}
