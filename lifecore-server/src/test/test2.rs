use crate::{
    Shared, Ui,
    access::{Access, Token},
    router, run,
};
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
};
use lifecore::{
    LifeCoreError, View,
    analysis::{
        Analyzer, AnalyzerCertificate, AnalyzerFactory, Config, Diagnostic, EvaluationInputs,
        Message, MessageKind,
    },
    transaction::{EntryChange, NewEdge, NewNode, Reference, RevisionIntent},
};
use lifecore_app::{
    Service, rpc,
    storage::{DEFAULT_STATE_FILE, StatePath},
};
use lifecore_dag_check::DagCheckFactory;
use lifecore_typecheck::TypeCheckFactory;
use semver::Version;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    net::Ipv4Addr,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::task::TaskTracker;
use tower::ServiceExt;
use uuid::Uuid;

const PORT: u16 = 4242;
const TOKEN: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

//A A private directory per test, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("lifecore-server-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn state(&self) -> StatePath {
        StatePath::new(self.0.join(DEFAULT_STATE_FILE))
    }
    //A A database with TypeCheck, as `lifecore init` creates it.
    fn initialized(&self) -> StatePath {
        let state = self.state();
        service(state.clone())
            .init(&[configured(TypeCheckFactory::name())])
            .unwrap();
        state
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

fn configured(name: &str) -> Config {
    Config {
        name: name.into(),
        config: "{}".into(),
    }
}

//A Test analyzers: one that fails, one that panics, and one that suggests.
//A The first two misbehave only on a nonempty prefix, so `init` succeeds.
#[derive(Clone, Copy)]
enum Scripted {
    Failing,
    Panicking,
    Suggesting,
}

impl Scripted {
    const ALL: [Self; 3] = [Self::Failing, Self::Panicking, Self::Suggesting];
    fn name(self) -> &'static str {
        match self {
        Self::Failing => "Fixture_Failing",
        Self::Panicking => "Fixture_Panicking",
        Self::Suggesting => "Fixture_Suggestions",
        }
    }
}

impl Analyzer for Scripted {
    fn version(&self) -> Version {
        Version::new(1, 0, 0)
    }
    //A The suggesting analyzer also speaks at initialization, so recorded
    //A outputs include an `Initialization` observation.
    fn initialize(&mut self, _: &EvaluationInputs) -> Result<Vec<Message>, String> {
        let Self::Suggesting = self else {
            return Ok(vec![]);
        };
        Ok(vec![Message {
            analyzer_certificate: AnalyzerCertificate("scope".into()),
            diagnostic: Diagnostic {
                subjects: BTreeSet::new(),
                focus: None,
                message: "suggestions consider current tasks only".into(),
                kind: MessageKind::Warning,
                sticky: false,
            },
        }])
    }
    fn analyze(&self, view: View<'_>, _: &EvaluationInputs) -> Result<Vec<Message>, String> {
        match self {
        _ if view.is_empty() => Ok(vec![]),
        Self::Failing => Err(format!("cannot judge boundary {}", view.len())),
        Self::Panicking => panic!("fixture analyzer panicked at boundary {}", view.len()),
        Self::Suggesting => Ok(suggest_done(view)),
        }
    }
}

//A Suggests revising the first current, unfinished Task to done, and making a
//A second one depend on the revision.
fn suggest_done(view: View<'_>) -> Vec<Message> {
    let mut open = view
        .fresh_nodes()
        .filter(|node| node.ty_name == "Task" && node.body.get("status") != Some(&"done".into()));
    let Some(task) = open.next() else {
        return vec![];
    };
    let mut body = task.body.clone();
    body.insert("status".into(), "done".into());
    let mut actions = vec![EntryChange::AddNode(NewNode {
        ty_name: "Task".into(),
        body,
        parent: Some(Reference::Existing(task.id)),
        intent: Some(RevisionIntent::Revise),
    })];
    if let Some(other) = open.next() {
        actions.push(EntryChange::AddEdge(NewEdge {
            ty_name: "DependsOn".into(),
            from: Reference::Existing(other.id),
            to: Reference::Added(0),
        }));
    }
    vec![Message {
        analyzer_certificate: AnalyzerCertificate(format!("done:{}", task.id.position())),
        diagnostic: Diagnostic {
            subjects: BTreeSet::from([task.id]),
            focus: Some(task.id),
            message: "this task may be done".into(),
            kind: MessageKind::Suggestion(actions),
            sticky: false,
        },
    }]
}

impl AnalyzerFactory for Scripted {
    fn analyzer_with_config(&mut self, _: &str) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        Ok(Box::new(*self))
    }
    fn analyzer_name(&self) -> &'static str {
        self.name()
    }
}

fn factories() -> Vec<Box<dyn AnalyzerFactory>> {
    let mut factories: Vec<Box<dyn AnalyzerFactory>> =
        vec![Box::new(TypeCheckFactory), Box::new(DagCheckFactory)];
    factories.extend(
        Scripted::ALL
            .into_iter()
            .map(|scripted| Box::new(scripted) as Box<dyn AnalyzerFactory>),
    );
    factories
}

fn service(state: StatePath) -> Service {
    Service::new(state, Box::new(factories))
}

fn shared(service: Service, port: u16, ui: Ui) -> Shared {
    Shared {
        service: Arc::new(service),
        access: Arc::new(Access::new(port, Token::new(TOKEN))),
        ui: Arc::new(ui),
        tasks: TaskTracker::new(),
    }
}

fn call_text(id: &str, method: &str, params: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

//A A one-node proposal that TypeCheck accepts: a new type declaration.
fn declare(name: &str) -> String {
    call_text(
        "declare",
        "proposal.apply",
        json!({"proposal": {"entries": [
            {"name": "declaration", "kind": "node", "type": "Type", "payload": {"name": name}},
        ]}}),
    )
}

type Headers = Vec<(&'static str, String)>;

fn valid_headers(port: u16) -> Headers {
    vec![
        ("host", format!("127.0.0.1:{port}")),
        ("authorization", format!("Bearer {TOKEN}")),
        ("content-type", "application/json".into()),
    ]
}

//A Replaces, adds, or (with `None`) removes one header.
fn with(mut headers: Headers, name: &'static str, value: Option<&str>) -> Headers {
    headers.retain(|(existing, _)| *existing != name);
    if let Some(value) = value {
        headers.push((name, value.into()));
    }
    headers
}

fn request(method: &str, uri: &str, headers: &Headers, body: impl Into<Body>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, value);
    }
    builder.body(body.into()).unwrap()
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = router.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    (parts.status, parts.headers, bytes.to_vec())
}

fn json_body(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

//A Holds the writer lock as a competing process would, until dropped.
fn hold_writer_lock(state: &StatePath) -> File {
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(state.lock_path())
        .unwrap();
    file.lock().unwrap();
    file
}

fn declared_types(service: &Service) -> Vec<String> {
    let line = call_text(
        "types",
        "entries.list",
        json!({"type": "Type", "currentness": "all"}),
    );
    let listed = serde_json::to_value(rpc::handle(service, &line)).unwrap();
    listed["result"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["payload"]["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn launch_tokens_are_256_random_bits_that_debug_never_prints() {
    let first = Token::generate().unwrap();
    let second = Token::generate().unwrap();
    let revealed = first.reveal().to_string();
    assert_eq!(revealed.len(), 64);
    assert!(
        revealed
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert_ne!(revealed, second.reveal());
    assert_eq!(format!("{first:?}"), "Token(..)");
    let printed = format!("{:?}", Access::new(PORT, first));
    assert!(printed.contains("Token(..)"), "{printed}");
    assert!(!printed.contains(&revealed), "{printed}");
}

//A Each request would append a type declaration if dispatched; the stored
//A snapshot proves none was. The same request with valid headers then applies.
#[tokio::test]
async fn requests_failing_an_access_check_are_rejected_before_dispatch() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let router = router(shared(service(state.clone()), PORT, Ui::Embedded));
    let valid = valid_headers(PORT);
    let wrong_token = "f".repeat(64);
    let cases = [
        (
            with(valid.clone(), "authorization", None),
            StatusCode::UNAUTHORIZED,
            "token",
        ),
        (
            with(
                valid.clone(),
                "authorization",
                Some(&format!("Bearer {wrong_token}")),
            ),
            StatusCode::UNAUTHORIZED,
            "token",
        ),
        (
            with(
                valid.clone(),
                "authorization",
                Some(&format!("Bearer {}", &TOKEN[..63])),
            ),
            StatusCode::UNAUTHORIZED,
            "token",
        ),
        (
            with(
                valid.clone(),
                "authorization",
                Some(&format!("Bearer {TOKEN}0")),
            ),
            StatusCode::UNAUTHORIZED,
            "token",
        ),
        (
            with(
                valid.clone(),
                "authorization",
                Some(&format!("Basic {TOKEN}")),
            ),
            StatusCode::UNAUTHORIZED,
            "token",
        ),
        (
            with(valid.clone(), "authorization", Some(TOKEN)),
            StatusCode::UNAUTHORIZED,
            "token",
        ),
        (
            {
                let mut twice = valid.clone();
                twice.push(("authorization", format!("Bearer {TOKEN}")));
                twice
            },
            StatusCode::UNAUTHORIZED,
            "token",
        ),
        (
            with(valid.clone(), "content-type", None),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "content_type",
        ),
        (
            with(valid.clone(), "content-type", Some("text/plain")),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "content_type",
        ),
        (
            with(
                valid.clone(),
                "content-type",
                Some("application/x-www-form-urlencoded"),
            ),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "content_type",
        ),
        (
            with(
                valid.clone(),
                "content-type",
                Some("multipart/form-data; boundary=x"),
            ),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "content_type",
        ),
        (
            with(valid.clone(), "content-type", Some("application/jsonp")),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "content_type",
        ),
        (
            with(valid.clone(), "origin", Some("http://evil.example")),
            StatusCode::FORBIDDEN,
            "origin",
        ),
        (
            with(valid.clone(), "origin", Some("null")),
            StatusCode::FORBIDDEN,
            "origin",
        ),
        (
            with(
                valid.clone(),
                "origin",
                Some(&format!("https://127.0.0.1:{PORT}")),
            ),
            StatusCode::FORBIDDEN,
            "origin",
        ),
        (
            with(
                valid.clone(),
                "origin",
                Some(&format!("http://localhost:{PORT}")),
            ),
            StatusCode::FORBIDDEN,
            "origin",
        ),
        (
            with(valid.clone(), "host", Some(&format!("evil.example:{PORT}"))),
            StatusCode::MISDIRECTED_REQUEST,
            "host",
        ),
        (
            with(valid.clone(), "host", Some("127.0.0.1:4243")),
            StatusCode::MISDIRECTED_REQUEST,
            "host",
        ),
        (
            with(valid.clone(), "host", Some("localhost")),
            StatusCode::MISDIRECTED_REQUEST,
            "host",
        ),
        (
            with(valid.clone(), "host", None),
            StatusCode::MISDIRECTED_REQUEST,
            "host",
        ),
        //A A rebinding name with a foreign origin fails on the host first.
        (
            with(
                with(valid.clone(), "host", Some("rebound.example:4242")),
                "origin",
                Some("http://rebound.example:4242"),
            ),
            StatusCode::MISDIRECTED_REQUEST,
            "host",
        ),
    ];
    let stored = scratch.bytes();
    for (headers, status, kind) in cases {
        let (actual, response_headers, body) =
            send(&router, request("POST", "/rpc", &headers, declare("Note"))).await;
        assert_eq!(actual, status, "{headers:?}");
        let body = json_body(&body);
        assert_eq!(body["kind"], kind, "{headers:?}");
        assert!(body["message"].is_string());
        assert_eq!(response_headers[header::CONTENT_TYPE], "application/json");
        assert_eq!(
            response_headers.get(header::WWW_AUTHENTICATE).is_some(),
            kind == "token"
        );
        assert_eq!(scratch.bytes(), stored, "{headers:?}");
    }
    //A An absolute-form target names its own authority, which must agree.
    let (status, _, body) = send(
        &router,
        request("POST", "http://evil.example/rpc", &valid, declare("Note")),
    )
    .await;
    assert_eq!(
        (status, json_body(&body)["kind"].clone()),
        (StatusCode::MISDIRECTED_REQUEST, json!("host"))
    );
    assert_eq!(scratch.bytes(), stored);
    //A Accepted spellings: either loopback name with its own origin, a
    //A lowercase scheme, and media type parameters.
    let accepted = with(
        with(
            with(valid.clone(), "host", Some(&format!("localhost:{PORT}"))),
            "origin",
            Some(&format!("http://localhost:{PORT}")),
        ),
        "content-type",
        Some("Application/JSON; charset=utf-8"),
    );
    let accepted = with(accepted, "authorization", Some(&format!("bearer {TOKEN}")));
    let (status, response_headers, body) =
        send(&router, request("POST", "/rpc", &accepted, declare("Note"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response_headers[header::CACHE_CONTROL], "no-store");
    let body = json_body(&body);
    assert_eq!(body["id"], "declare");
    assert!(body["result"]["graph_revision"].is_string(), "{body}");
    assert_ne!(scratch.bytes(), stored);
    let (status, _, body) = send(
        &router,
        request(
            "POST",
            "/rpc",
            &with(valid, "origin", Some(&format!("http://127.0.0.1:{PORT}"))),
            declare("Other"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(json_body(&body)["result"].is_object());
    assert_eq!(declared_types(&service(state)), ["Note", "Other"]);
}

#[tokio::test]
async fn static_assets_need_no_token_but_pass_the_host_and_origin_checks() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let embedded = router(shared(service(state.clone()), PORT, Ui::Embedded));
    let host = vec![("host", format!("127.0.0.1:{PORT}"))];
    let (status, headers, body) = send(&embedded, request("GET", "/", &host, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "text/html; charset=utf-8");
    assert!(
        headers[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("default-src 'self'")
    );
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(body, crate::assets::INDEX);
    let (status, headers, body) =
        send(&embedded, request("GET", "/app.js", &host, Body::empty())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    assert_eq!(body, crate::assets::SCRIPT);
    let (status, _, _) = send(&embedded, request("GET", "/missing", &host, Body::empty())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(
        &embedded,
        request("GET", "/rpc", &valid_headers(PORT), Body::empty()),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    for (headers, status) in [
        (
            with(host.clone(), "host", Some("evil.example:4242")),
            StatusCode::MISDIRECTED_REQUEST,
        ),
        (
            with(host.clone(), "origin", Some("http://evil.example")),
            StatusCode::FORBIDDEN,
        ),
    ] {
        for path in ["/", "/app.js", "/missing"] {
            let (actual, _, _) =
                send(&embedded, request("GET", path, &headers, Body::empty())).await;
            assert_eq!(actual, status, "{path} {headers:?}");
        }
    }
    //A A client directory is read on every request, so a rebuild shows on refresh.
    let directory = scratch.0.join("ui");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("index.html"), "<p>first</p>").unwrap();
    let disk = router(shared(
        service(state),
        PORT,
        Ui::Directory(directory.clone()),
    ));
    let (status, _, body) = send(&disk, request("GET", "/", &host, Body::empty())).await;
    assert_eq!((status, body), (StatusCode::OK, b"<p>first</p>".to_vec()));
    fs::write(directory.join("index.html"), "<p>second</p>").unwrap();
    let (_, _, body) = send(&disk, request("GET", "/", &host, Body::empty())).await;
    assert_eq!(body, b"<p>second</p>");
    let (status, _, _) = send(&disk, request("GET", "/app.js", &host, Body::empty())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

//A HTTP framing allows formatted bodies; every envelope or body failure past
//A the access checks is a JSON-RPC error with HTTP 200.
#[tokio::test]
async fn rpc_bodies_may_span_lines_and_their_failures_stay_json_rpc() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let router = router(shared(service(state), PORT, Ui::Embedded));
    let pretty = serde_json::to_string_pretty(
        &json!({"jsonrpc": "2.0", "id": 7, "method": "database.describe", "params": {}}),
    )
    .unwrap();
    let (status, _, body) = send(
        &router,
        request("POST", "/rpc", &valid_headers(PORT), pretty),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body = json_body(&body);
    assert_eq!(body["id"], 7);
    assert_eq!(body["result"]["view"]["end_exclusive"], "0");
    for (text, code) in [
        (b"{".to_vec(), -32700),
        (vec![0xff, b'{', b'}'], -32700),
        (b"[]".to_vec(), -32600),
        (
            br#"{"jsonrpc":"2.0","method":"database.describe"}"#.to_vec(),
            -32600,
        ),
        (call_text("x", "init", json!({})).into_bytes(), -32601),
    ] {
        let (status, _, body) = send(
            &router,
            request("POST", "/rpc", &valid_headers(PORT), text.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            json_body(&body)["error"]["code"],
            code,
            "{}",
            String::from_utf8_lossy(&text)
        );
    }
}

//A A busy writer lock and a panicking call both come back as structured
//A JSON-RPC errors carrying the caller's ID, and neither mutates the state.
#[tokio::test]
async fn busy_writers_and_failed_tasks_answer_with_structured_errors() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let quick = service(state.clone()).with_busy_retry(Duration::from_millis(50));
    let router = router(shared(quick, PORT, Ui::Embedded));
    let stored = scratch.bytes();
    let lock = hold_writer_lock(&state);
    let (status, _, body) = send(
        &router,
        request("POST", "/rpc", &valid_headers(PORT), declare("Note")),
    )
    .await;
    drop(lock);
    assert_eq!(status, StatusCode::OK);
    let body = json_body(&body);
    assert_eq!(body["id"], "declare");
    assert_eq!(body["error"]["code"], 1006);
    assert_eq!(body["error"]["data"]["kind"], "busy");
    assert_eq!(scratch.bytes(), stored);
    let panicking = call_text(
        "panics",
        "analyzers.add",
        json!({"name": Scripted::Panicking.name()}),
    );
    send(
        &router,
        request("POST", "/rpc", &valid_headers(PORT), declare("Note")),
    )
    .await;
    let stored = scratch.bytes();
    let (status, _, body) = send(
        &router,
        request("POST", "/rpc", &valid_headers(PORT), panicking),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body = json_body(&body);
    assert_eq!(body["id"], "panics");
    assert_eq!(body["error"]["code"], 5001, "{body}");
    assert_eq!(body["error"]["data"]["kind"], "backend_failure");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("fixture analyzer panicked at boundary 1")
    );
    assert_eq!(scratch.bytes(), stored);
    //A The server keeps answering afterwards.
    let (_, _, body) = send(
        &router,
        request(
            "POST",
            "/rpc",
            &valid_headers(PORT),
            call_text("after", "database.describe", json!({})),
        ),
    )
    .await;
    assert!(json_body(&body)["result"].is_object());
}

//A Dropping the response future is what the server does when a connection
//A closes mid-call. The blocking task still publishes, and nothing reports a
//A rollback.
#[tokio::test(flavor = "multi_thread")]
async fn a_client_that_goes_away_does_not_cancel_its_write() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let shared = shared(service(state.clone()), PORT, Ui::Embedded);
    let tasks = shared.tasks.clone();
    let router = router(shared);
    let lock = hold_writer_lock(&state);
    let call = router.oneshot(request(
        "POST",
        "/rpc",
        &valid_headers(PORT),
        declare("Note"),
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(300), call)
            .await
            .is_err()
    );
    assert_eq!(tasks.len(), 1);
    assert!(declared_types(&service(state.clone())).is_empty());
    drop(lock);
    tasks.close();
    tokio::time::timeout(Duration::from_secs(10), tasks.wait())
        .await
        .unwrap();
    assert_eq!(declared_types(&service(state)), ["Note"]);
}

//A Shutdown stops accepting connections, lets the running call finish and
//A answer its client, and only then returns.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_waits_for_running_calls_to_finish_and_answer() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = shared(service(state.clone()), port, Ui::Embedded);
    let tasks = shared.tasks.clone();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(run(listener, router(shared), tasks, async {
        stopped.await.ok();
    }));
    let lock = hold_writer_lock(&state);
    let body = declare("Note");
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    let head = format!(
        "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body.as_bytes()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    stop.send(()).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!server.is_finished());
    assert!(declared_types(&service(state.clone())).is_empty());
    drop(lock);
    let mut response = String::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        stream.read_to_string(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    let json: Value = serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert!(json["result"]["graph_revision"].is_string(), "{json}");
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(declared_types(&service(state)), ["Note"]);
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .is_err()
    );
}
