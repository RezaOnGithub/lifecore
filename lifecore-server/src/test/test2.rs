use crate::{
    BODY_LIMIT, Options, Shared, Ui,
    access::{Access, Token},
    respond, router, run,
};
use axum::{
    Router,
    body::{Body, Bytes},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
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
    AppError, Service,
    rpc::{self, Method},
    service::{ApplyResult, DismissalList, Published},
    storage::{DEFAULT_STATE_FILE, StatePath},
};
use lifecore_dag_check::DagCheckFactory;
use lifecore_typecheck::TypeCheckFactory;
use semver::Version;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::{self, File},
    io,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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

// A private directory per test, removed afterwards.
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
    // A database with TypeCheck, as `lifecore init` creates it.
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
        configuration: "{}".into(),
    }
}

// Test analyzers: one that fails, one that panics, and one that suggests.
// The first two misbehave only on a nonempty prefix, so `init` succeeds.
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
    // The suggesting analyzer also speaks at initialization, so recorded
    // outputs include an `Initialization` observation.
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

// Suggests revising the first current, unfinished Task to done, and making a
// second one depend on the revision.
fn suggest_done(view: View<'_>) -> Vec<Message> {
    let mut open = view.fresh_nodes().filter(|node| {
        node.ty_name == "Task" && node.payload.get("status") != Some(&"done".into())
    });
    let Some(task) = open.next() else {
        return vec![];
    };
    let mut payload = task.payload.clone();
    payload.insert("status".into(), "done".into());
    let mut actions = vec![EntryChange::Node(NewNode {
        ty_name: "Task".into(),
        payload,
        predecessor: Some(Reference::Existing(task.id)),
        intent: Some(RevisionIntent::Revise),
    })];
    if let Some(other) = open.next() {
        actions.push(EntryChange::Edge(NewEdge {
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

// A one-node proposal that TypeCheck accepts: a new type declaration.
fn declare(name: &str) -> String {
    call_text(
        "declare",
        "proposal.apply",
        json!({"proposal": {"entries": [
            {"name": "declaration", "kind": "node", "ty_name": "Type", "payload": {"name": name}},
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

// Replaces, adds, or (with `None`) removes one header.
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

// Holds the writer lock as a competing process would, until dropped.
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
        json!({"ty_name": "Type", "currentness": "all"}),
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

// Each request would append a type declaration if dispatched; the stored
// snapshot proves none was. The same request with valid headers then applies.
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
        // A rebinding name with a foreign origin fails on the host first.
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
    // An absolute-form target names its own authority, which must agree.
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
    // Accepted spellings: either loopback name with its own origin, a
    // lowercase scheme, and media type parameters.
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
    // A client directory is read on every request, so a rebuild shows on refresh.
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

// HTTP framing allows formatted bodies; every envelope or body failure past
// the access checks is a JSON-RPC error with HTTP 200.
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

// A busy writer lock and a panicking call both come back as structured
// JSON-RPC errors carrying the caller's ID, and neither mutates the state.
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
    // The server keeps answering afterwards.
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

// Dropping the response future is what the server does when a connection
// closes mid-call. The blocking task still publishes, and nothing reports a
// rollback.
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

// Shutdown stops accepting connections, lets the running call finish and
// answer its client, and only then returns.
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

// A body that fails when read, as malformed chunking or an early close does.
fn failing_body() -> Body {
    Body::from_stream(futures_util::stream::once(async {
        Err::<Bytes, io::Error>(io::Error::new(
            io::ErrorKind::ConnectionReset,
            "connection reset by peer",
        ))
    }))
}

// A body of `length` bytes, produced only when something reads it.
fn watched_body(length: usize) -> (Body, Arc<AtomicBool>) {
    let read = Arc::new(AtomicBool::new(false));
    let flag = read.clone();
    let body = Body::from_stream(futures_util::stream::once(async move {
        flag.store(true, Ordering::SeqCst);
        Ok::<_, io::Error>(Bytes::from(vec![b' '; length]))
    }));
    (body, read)
}

// Each step repairs the earliest failing check, so the next one in the
// documented order answers: Host, Origin, token, content type, then the body,
// which none of the earlier checks reads.
#[tokio::test]
async fn access_checks_run_in_order_before_the_body_is_read() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let router = router(shared(service(state), PORT, Ui::Embedded));
    let stored = scratch.bytes();
    let mut headers: Headers = vec![
        ("host", "rebound.example:4242".into()),
        ("origin", "http://evil.example".into()),
        ("content-type", "text/plain".into()),
    ];
    let steps = [
        (StatusCode::MISDIRECTED_REQUEST, "host"),
        (StatusCode::FORBIDDEN, "origin"),
        (StatusCode::UNAUTHORIZED, "token"),
        (StatusCode::UNSUPPORTED_MEDIA_TYPE, "content_type"),
        (StatusCode::PAYLOAD_TOO_LARGE, "body"),
    ];
    for (step, (status, kind)) in steps.into_iter().enumerate() {
        let (body, read) = watched_body(BODY_LIMIT + 1);
        let (actual, _, response) = send(&router, request("POST", "/rpc", &headers, body)).await;
        assert_eq!(
            (actual, json_body(&response)["kind"].clone()),
            (status, json!(kind))
        );
        assert_eq!(read.load(Ordering::SeqCst), kind == "body", "{kind}");
        headers = match step {
        0 => with(headers, "host", Some(&format!("127.0.0.1:{PORT}"))),
        1 => with(headers, "origin", None),
        2 => with(headers, "authorization", Some(&format!("Bearer {TOKEN}"))),
        _ => with(headers, "content-type", Some("application/json")),
        };
    }
    let no_token = with(valid_headers(PORT), "authorization", None);
    for (headers, status, kind) in [
        (
            with(no_token.clone(), "content-type", Some("text/plain")),
            StatusCode::UNAUTHORIZED,
            "token",
        ),
        (
            with(no_token.clone(), "origin", Some("http://evil.example")),
            StatusCode::FORBIDDEN,
            "origin",
        ),
        (no_token, StatusCode::UNAUTHORIZED, "token"),
    ] {
        let (body, read) = watched_body(BODY_LIMIT + 1);
        let (actual, _, response) = send(&router, request("POST", "/rpc", &headers, body)).await;
        assert_eq!(
            (actual, json_body(&response)["kind"].clone()),
            (status, json!(kind))
        );
        assert!(!read.load(Ordering::SeqCst), "{headers:?}");
    }
    assert_eq!(scratch.bytes(), stored);
}

// A repeated header is refused even when every copy would be accepted alone.
#[tokio::test]
async fn repeated_host_origin_and_content_type_headers_are_rejected() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let router = router(shared(service(state), PORT, Ui::Embedded));
    let stored = scratch.bytes();
    let origin = format!("http://127.0.0.1:{PORT}");
    let headers = with(valid_headers(PORT), "origin", Some(&origin));
    let (status, _, _) = send(&router, request("POST", "/rpc", &headers, declare("Note"))).await;
    assert_eq!(status, StatusCode::OK);
    let stored_after_valid = scratch.bytes();
    assert_ne!(stored_after_valid, stored);
    for (name, value, status, kind) in [
        (
            header::HOST,
            format!("127.0.0.1:{PORT}"),
            StatusCode::MISDIRECTED_REQUEST,
            "host",
        ),
        (
            header::ORIGIN,
            origin.clone(),
            StatusCode::FORBIDDEN,
            "origin",
        ),
        (
            header::CONTENT_TYPE,
            "application/json".into(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "content_type",
        ),
    ] {
        let mut repeated = request("POST", "/rpc", &headers, declare("Other"));
        repeated
            .headers_mut()
            .append(name.clone(), HeaderValue::from_str(&value).unwrap());
        assert_eq!(repeated.headers().get_all(&name).iter().count(), 2);
        let (actual, _, body) = send(&router, repeated).await;
        assert_eq!(
            (actual, json_body(&body)["kind"].clone()),
            (status, json!(kind))
        );
        assert_eq!(scratch.bytes(), stored_after_valid, "{name}");
    }
}

// The adapter never approves a CORS preflight, whatever it carries.
#[tokio::test]
async fn preflight_requests_are_never_approved() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let router = router(shared(service(state), PORT, Ui::Embedded));
    let preflight = vec![
        ("host", format!("127.0.0.1:{PORT}")),
        ("access-control-request-method", "POST".into()),
        (
            "access-control-request-headers",
            "authorization, content-type".into(),
        ),
    ];
    for headers in [
        with(preflight.clone(), "origin", Some("http://evil.example")),
        with(
            preflight.clone(),
            "origin",
            Some(&format!("http://127.0.0.1:{PORT}")),
        ),
        preflight.clone(),
        with(
            with(preflight, "authorization", Some(&format!("Bearer {TOKEN}"))),
            "content-type",
            Some("application/json"),
        ),
    ] {
        let (status, response_headers, _) =
            send(&router, request("OPTIONS", "/rpc", &headers, Body::empty())).await;
        assert!(status.is_client_error(), "{status} {headers:?}");
        assert!(
            response_headers
                .keys()
                .all(|name| !name.as_str().starts_with("access-control-")),
            "{response_headers:?}"
        );
    }
}

// Starts a real server on a loopback port; dropping the sender stops it.
async fn serving(
    state: &StatePath,
) -> (
    u16,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<io::Result<()>>,
) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = shared(service(state.clone()), port, Ui::Embedded);
    let tasks = shared.tasks.clone();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(run(listener, router(shared), tasks, async {
        stopped.await.ok();
    }));
    (port, stop, server)
}

// Sends raw bytes, optionally closing the write half, and returns the answer.
async fn exchange(port: u16, bytes: &[u8], half_close: bool) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    stream.write_all(bytes).await.unwrap();
    if half_close {
        stream.shutdown().await.unwrap();
    }
    let mut response = String::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        stream.read_to_string(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    response
}

// The size limit answers 413 and any other unreadable body 400, both with
// the `body` kind and without dispatch, in process and over a real
// connection with malformed chunking or a body cut short.
#[tokio::test(flavor = "multi_thread")]
async fn unreadable_bodies_answer_400_and_oversized_ones_413() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let router = router(shared(service(state.clone()), PORT, Ui::Embedded));
    let stored = scratch.bytes();
    for (body, status) in [
        (failing_body(), StatusCode::BAD_REQUEST),
        (
            Body::from(vec![b' '; BODY_LIMIT + 1]),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let (actual, headers, response) =
            send(&router, request("POST", "/rpc", &valid_headers(PORT), body)).await;
        assert_eq!(actual, status);
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        assert_eq!(json_body(&response)["kind"], "body");
    }
    let (port, stop, server) = serving(&state).await;
    let head = format!(
        "POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nConnection: close\r\n"
    );
    let body = declare("Note");
    let cut_short = format!(
        "{head}Content-Length: {}\r\n\r\n{}",
        body.len(),
        &body[..10]
    );
    for (bytes, half_close) in [
        (
            format!("{head}Transfer-Encoding: chunked\r\n\r\nzz\r\n"),
            false,
        ),
        (cut_short, true),
    ] {
        let response = exchange(port, bytes.as_bytes(), half_close).await;
        assert!(
            response.starts_with("HTTP/1.1 400 Bad Request"),
            "{response}"
        );
        let json: Value = serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(json["kind"], "body", "{response}");
    }
    drop(stop);
    server.await.unwrap().unwrap();
    assert_eq!(scratch.bytes(), stored);
}

#[test]
fn port_80_is_refused_before_anything_is_bound() {
    let scratch = Scratch::new();
    let state = scratch.initialized();
    let options = Options {
        port: 80,
        ui: Ui::Embedded,
    };
    let error = crate::serve(service(state), options, |url| {
        panic!("port 80 printed {url}")
    })
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("port 80"), "{error}");
}

// Fixtures replace random UUIDs, in order of first appearance, and the
// scratch directory with stable stand-ins, so an unchanged Rust shape writes
// byte-identical files.
struct Normalizer {
    scratch: String,
    uuids: HashMap<String, String>,
}

impl Normalizer {
    fn value(&mut self, value: Value) -> Value {
        match value {
        Value::String(text) => Value::String(self.text(&text)),
        Value::Array(items) => Value::Array(items.into_iter().map(|v| self.value(v)).collect()),
        Value::Object(members) => Value::Object(
            members
                .into_iter()
                .map(|(name, member)| (name, self.value(member)))
                .collect(),
        ),
        other => other,
        }
    }
    fn text(&mut self, text: &str) -> String {
        let text = text.replace(&self.scratch, "/scratch");
        let mut normalized = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(c) = rest.chars().next() {
            match rest.get(..36).filter(|candidate| is_uuid(candidate)) {
            Some(uuid) => {
                let next = self.uuids.len() + 1;
                let stand_in = self
                    .uuids
                    .entry(uuid.into())
                    .or_insert_with(|| format!("00000000-0000-4000-8000-{next:012x}"));
                normalized.push_str(stand_in);
                rest = &rest[36..];
            }
            None => {
                normalized.push(c);
                rest = &rest[c.len_utf8()..];
            }
            }
        }
        normalized
    }
}

fn is_uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| match i {
        8 | 13 | 18 | 23 => b == b'-',
        _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

// One generated TypeScript module: each export is a Rust-produced JSON value
// that `satisfies` its declared contract type, so tsc checks literal tags
// and rejects members the contract does not declare.
struct Module {
    file: String,
    text: String,
}

const GENERATED: &str = "// Generated by lifecore-server's fixture test from Rust values; do not edit.\n// `cargo test -p lifecore-server` rewrites stale fixtures and then fails.\n";

fn module(file: String, imports: &[&str], exports: Vec<(&str, String, Value)>) -> Module {
    let mut text = format!(
        "{GENERATED}import type {{ {} }} from \"../contract.ts\";\n",
        imports.join(", ")
    );
    for (name, ty, value) in exports {
        text.push_str(&format!(
            "\nexport const {name} = {} satisfies {ty};\n",
            serde_json::to_string_pretty(&value).unwrap()
        ));
    }
    Module { file, text }
}

// Maps every method to its module, so tsc requires a fixture for each method
// the contract declares and rejects one it does not.
fn index_module() -> Module {
    let mut text =
        format!("{GENERATED}import type {{ Method, MethodFixture }} from \"../contract.ts\";\n");
    for method in Method::ALL {
        text.push_str(&format!(
            "import * as {} from \"./{}.ts\";\n",
            method.name().replace('.', "_"),
            method.name()
        ));
    }
    text.push_str("\nexport const methods = {\n");
    for method in Method::ALL {
        text.push_str(&format!(
            "  {:?}: {},\n",
            method.name(),
            method.name().replace('.', "_")
        ));
    }
    text.push_str("} satisfies { [M in Method]: MethodFixture<M> };\n");
    Module {
        file: "index.ts".into(),
        text,
    }
}

// The fixture service: the built-in analyzers and the scripted ones, over a
// database whose suite also suggests, so outputs include suggested actions.
struct Recorder {
    service: Service,
    normalizer: Normalizer,
}

impl Recorder {
    fn call(&mut self, id: &str, method: &str, params: Value) -> (Value, Value) {
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let response = self.raw(&request.to_string());
        (self.normalizer.value(request), response)
    }
    // Exactly the server's path from body to response.
    fn raw(&mut self, body: &str) -> Value {
        let response = serde_json::to_value(respond(&self.service, body.as_bytes())).unwrap();
        self.normalizer.value(response)
    }
    // An unrecorded call's raw result, for identities a later request needs.
    fn raw_result(&self, method: &str, params: Value) -> Value {
        let line = call_text("setup", method, params);
        let response = serde_json::to_value(rpc::handle(&self.service, &line)).unwrap();
        assert!(response["result"].is_object(), "{response}");
        response["result"].clone()
    }
    fn error(&mut self, id: &str, error: &AppError) -> Value {
        let response = serde_json::to_value(rpc::Response::error(json!(id), error)).unwrap();
        self.normalizer.value(response)
    }
}

fn method_module(method: &str, pairs: Vec<(&str, &str, (Value, Value))>) -> Module {
    let mut exports = vec![];
    for (request, response, (request_value, response_value)) in pairs {
        assert!(
            response_value["result"].is_object(),
            "{method}: {response_value}"
        );
        exports.push((request, format!("RpcRequest<{method:?}>"), request_value));
        exports.push((response, format!("RpcSuccess<{method:?}>"), response_value));
    }
    module(
        format!("{method}.ts"),
        &["RpcRequest", "RpcSuccess"],
        exports,
    )
}

const EVALUATION_TIME: &str = "2026-10-15T08:30:00.123456789Z";

fn task_declaration() -> Value {
    json!({
        "name": "Task",
        "fields": {"kind": "record", "value": {
            "title": ["mandatory", "text"],
            "notes": ["encouraged", "text"],
            "status": ["optional", "text"],
            "priority": ["optional", "integer"],
            "due": ["optional", "timestamp"],
            "done": ["optional", "boolean"],
            "tags": ["optional", "list", "text"],
            "details": ["optional", "record", {"kind": "record", "value": {"owner": ["mandatory", "text"]}}],
        }},
        "outgoing": ["DependsOn"],
        "incoming": ["DependsOn"],
        "successors": [],
    })
}

// Every application error kind; the recorded fixtures must match it exactly.
const ERROR_KINDS: [&str; 25] = [
    "state_path_empty",
    "working_directory",
    "directory_missing",
    "database_missing",
    "database_unreadable",
    "database_exists",
    "lock_failed",
    "busy",
    "publication_failed",
    "publication_uncertain",
    "malformed_proposal",
    "invalid_proposal",
    "acknowledgment_unsupported",
    "foreign_database",
    "stale_base",
    "rejected",
    "analyzer_failure",
    "entry_not_found",
    "beyond_boundary",
    "cannot_dismiss_inconsistency",
    "graphviz_unavailable",
    "graphviz_failed",
    "invalid_params",
    "backend_failure",
    "internal",
];

// Has no wildcard, so a new `AppError` variant fails to compile here; add its
// kind to `ERROR_KINDS`, which then requires a recorded sample.
fn listed(error: &AppError) {
    match error {
    AppError::EmptyStatePath
    | AppError::WorkingDirectory { .. }
    | AppError::DirectoryMissing { .. }
    | AppError::DatabaseMissing { .. }
    | AppError::DatabaseUnreadable { .. }
    | AppError::DatabaseExists { .. }
    | AppError::Lock { .. }
    | AppError::Busy { .. }
    | AppError::PublicationFailed { .. }
    | AppError::PublicationUncertain { .. }
    | AppError::MalformedProposal(_)
    | AppError::InvalidProposal { .. }
    | AppError::AcknowledgmentUnsupported
    | AppError::ForeignDatabase { .. }
    | AppError::StaleBase { .. }
    | AppError::Rejected(_)
    | AppError::Analyzer(_)
    | AppError::EntryNotFound { .. }
    | AppError::BeyondBoundary { .. }
    | AppError::CannotDismissInconsistency { .. }
    | AppError::GraphvizUnavailable { .. }
    | AppError::GraphvizFailed { .. }
    | AppError::InvalidParams { .. }
    | AppError::Backend { .. }
    | AppError::Internal { .. } => {}
    }
}

// Representative requests and responses of every method, application error
// kind, protocol error, and HTTP rejection, recorded from real calls where a
// call can produce them.
async fn fixtures(scratch: &Scratch) -> Vec<Module> {
    let state = scratch.state();
    let mut recorder = Recorder {
        service: service(state.clone()),
        normalizer: Normalizer {
            scratch: scratch.0.to_string_lossy().into_owned(),
            uuids: HashMap::new(),
        },
    };
    recorder
        .service
        .init(&[
            configured(TypeCheckFactory::name()),
            configured(Scripted::Suggesting.name()),
        ])
        .unwrap();
    let mut modules = vec![];

    // A request may omit `params`.
    let mint_request = json!({"jsonrpc": "2.0", "id": 1, "method": "plan_id.mint"});
    let mint = (
        mint_request.clone(),
        recorder.raw(&mint_request.to_string()),
    );
    modules.push(method_module(
        "plan_id.mint",
        vec![("request", "response", mint)],
    ));
    // Later requests carry raw identities; recording normalizes them.
    let first_plan = recorder.raw_result("plan_id.mint", json!({}))["plan_id"].clone();

    let declared = recorder.call(
        "apply",
        "proposal.apply",
        json!({"proposal": {"plan_id": first_plan, "entries": [
            {"name": "task_type", "kind": "node", "ty_name": "Type", "payload": task_declaration()},
            {"name": "report", "kind": "node", "ty_name": "Task",
                "payload": {"title": "Write the report", "priority": {"kind": "integer", "value": "2"}}},
            {"name": "review", "kind": "node", "ty_name": "Task",
                "payload": {"title": "Review the report", "notes": "Ask Teto"}},
            {"name": "review_after_report", "kind": "edge", "ty_name": "DependsOn",
                "from": {"added": "review"}, "to": {"added": "report"}},
        ]}, "evaluation_time": EVALUATION_TIME}),
    );
    let applied: ApplyResult = serde_json::from_value(declared.1["result"].clone()).unwrap();
    modules.push(method_module(
        "proposal.apply",
        vec![("request", "response", declared)],
    ));
    // Raw identities for later requests; recorded requests normalize them.
    let raw_entries =
        recorder.raw_result("entries.list", json!({"currentness": "all"}))["entries"].clone();
    let report_id = raw_entries[1]["id"].clone();
    let edge_id = raw_entries[3]["id"].clone();

    let all = recorder.call("all", "entries.list", json!({"currentness": "all"}));
    let filtered = recorder.call(
        "filtered",
        "entries.list",
        json!({"kind": "node", "ty_name": "Task", "currentness": "current", "text": "report"}),
    );
    modules.push(method_module(
        "entries.list",
        vec![
            ("request", "response", all),
            ("filteredRequest", "filteredResponse", filtered),
        ],
    ));
    let got = recorder.call("get", "entries.get", json!({"id": edge_id}));
    modules.push(method_module(
        "entries.get",
        vec![("request", "response", got)],
    ));

    // Revise the review and restore its dependency, so inspection shows a
    // revision tree and a historical incident edge beside a current one.
    let review_id = raw_entries[2]["id"].clone();
    let revised = recorder.raw_result(
        "proposal.apply",
        json!({"proposal": {"entries": [
            {"name": "review_again", "kind": "node", "ty_name": "Task",
                "predecessor": {"existing": review_id}, "intent": "revise",
                "payload": {"title": "Review the report", "notes": "Ask Teto", "status": "in progress"}},
            {"name": "review_again_after_report", "kind": "edge", "ty_name": "DependsOn",
                "from": {"added": "review_again"}, "to": {"existing": report_id}},
        ]}}),
    );
    let review_again_id = revised["report"]["operations"][0]["entry"].clone();
    let inspected = recorder.call("inspect", "entries.inspect", json!({"id": report_id}));
    let revision = recorder.call(
        "revision",
        "entries.inspect",
        json!({"id": review_again_id}),
    );
    let inspected_edge = recorder.call("edge", "entries.inspect", json!({"id": edge_id}));
    modules.push(method_module(
        "entries.inspect",
        vec![
            ("request", "response", inspected),
            ("revisionRequest", "revisionResponse", revision),
            ("edgeRequest", "edgeResponse", inspected_edge),
        ],
    ));

    let types = recorder.call("types", "types.list", json!({}));
    let types_as_of = recorder.call("types_as_of", "types.list", json!({"as_of": "1"}));
    // Resolutions TypeCheck refuses, from an unchecked database in the same
    // directory: two current `Note` declarations, a malformed `Tag`, and a
    // `Ghost` node with no declaration.
    let unchecked = service(StatePath::new(scratch.0.join("unchecked.json")));
    unchecked.init(&[]).unwrap();
    let checked = std::mem::replace(&mut recorder.service, unchecked);
    recorder.raw_result(
        "proposal.apply",
        json!({"proposal": {"entries": [
            {"name": "note", "kind": "node", "ty_name": "Type", "payload": {"name": "Note",
                "fields": {"kind": "record", "value": {"text": ["mandatory", "text"]}}}},
            {"name": "note_again", "kind": "node", "ty_name": "Type", "payload": {"name": "Note"}},
            {"name": "tag", "kind": "node", "ty_name": "Type", "payload": {"name": "Tag", "fields": "text"}},
            {"name": "ghost", "kind": "node", "ty_name": "Ghost", "payload": {"title": "Undeclared"}},
        ]}}),
    );
    let unresolved = recorder.call("unresolved", "types.list", json!({}));
    recorder.service = checked;
    modules.push(method_module(
        "types.list",
        vec![
            ("request", "response", types),
            ("asOfRequest", "asOfResponse", types_as_of),
            ("unresolvedRequest", "unresolvedResponse", unresolved),
        ],
    ));

    let dependents = recorder.call(
        "dependents",
        "views.query",
        json!({
            "seeds": [{"kind": "ids", "ids": [report_id]}],
            "expand": [{"direction": "incoming", "currentness": "all"}],
        }),
    );
    // Before the revision, two hops from the review over all edges.
    let around_review = recorder.call(
        "around_review",
        "views.query",
        json!({
            "seeds": [{"kind": "nodes", "ty_name": "Task", "text": "Review"}],
            "expand": [{"hops": 2}],
            "edges": {"ty_names": ["DependsOn"], "currentness": "all"},
            "as_of": "4",
        }),
    );
    modules.push(method_module(
        "views.query",
        vec![
            ("request", "response", dependents),
            ("asOfRequest", "asOfResponse", around_review),
        ],
    ));
    // Only DOT is recorded: SVG needs Graphviz on the recording machine, and
    // its text varies with the installed version.
    let rendered = recorder.call(
        "render",
        "views.render",
        json!({
            "seeds": [{"kind": "ids", "ids": [report_id]}],
            "expand": [{"direction": "incoming", "currentness": "all"}],
            "edges": {"currentness": "all"},
            "format": "dot",
        }),
    );
    modules.push(method_module(
        "views.render",
        vec![("request", "response", rendered)],
    ));

    // The report has no `notes`, so TypeCheck warns about it at every prefix
    // from its append, and the suite also suggests.
    let analysis = recorder.call(
        "analysis",
        "analysis.run",
        json!({"evaluation_time": EVALUATION_TIME}),
    );
    let analysis_as_of = recorder.call("analysis_as_of", "analysis.run", json!({"as_of": "2"}));
    modules.push(method_module(
        "analysis.run",
        vec![
            ("request", "response", analysis),
            ("asOfRequest", "asOfResponse", analysis_as_of),
        ],
    ));
    // The TypeCheck warning's complete key, raw, for the dismissal calls.
    let warning_key = recorder.raw_result("analysis.run", json!({}))["analysis"]["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|observation| {
            observation["key"]["kind"] == "warning"
                && observation["key"]["analyzer"]["name"] == TypeCheckFactory::name()
        })
        .expect("the report task warns")["key"]
        .clone();
    let dismissed = recorder.call(
        "dismiss",
        "findings.dismiss",
        json!({"key": warning_key, "reason": "false_positive", "note": "Notes come with the draft."}),
    );
    let dismissal_list: DismissalList =
        serde_json::from_value(dismissed.1["result"].clone()).unwrap();
    modules.push(method_module(
        "findings.dismiss",
        vec![("request", "response", dismissed)],
    ));
    let restored = recorder.call("restore", "findings.restore", json!({"key": warning_key}));
    assert_eq!(restored.1["result"]["restored"], true, "{}", restored.1);
    modules.push(method_module(
        "findings.restore",
        vec![("request", "response", restored)],
    ));

    let plan = recorder.raw_result("plan_id.mint", json!({}))["plan_id"].clone();
    let accepted = recorder.call(
        "check",
        "proposal.check",
        json!({"proposal": {"plan_id": plan, "entries": [
            {"name": "report_done", "kind": "node", "ty_name": "Task",
                "predecessor": {"existing": report_id}, "intent": "revise",
                "payload": {
                    "title": "Write the report",
                    "notes": "Sent on time",
                    "status": "done",
                    "priority": {"kind": "integer", "value": "-9223372036854775808"},
                    "due": {"kind": "timestamp", "value": "2026-10-31T17:00:00.000000001Z"},
                    "done": {"kind": "boolean", "value": true},
                    "tags": ["work", "writing"],
                    "details": {"kind": "record", "value": {"owner": "Teto"}},
                }},
            {"name": "follow_up", "kind": "node", "ty_name": "Task",
                "payload": {"title": "Follow up", "notes": "Later",
                    "priority": {"kind": "integer", "value": "9223372036854775807"}}},
            {"name": "follow_up_after_report", "kind": "edge", "ty_name": "DependsOn",
                "from": {"added": "follow_up"}, "to": {"added": "report_done"}},
        ]}, "evaluation_time": EVALUATION_TIME}),
    );
    assert_eq!(accepted.1["result"]["accepted"], true, "{}", accepted.1);
    let undeclared = json!({"entries": [
        {"name": "stray", "kind": "node", "ty_name": "Undeclared", "payload": {"title": "Stray"}},
    ]});
    let rejected = recorder.call(
        "rejected",
        "proposal.check",
        json!({"proposal": undeclared}),
    );
    assert_eq!(rejected.1["result"]["accepted"], false, "{}", rejected.1);
    modules.push(method_module(
        "proposal.check",
        vec![
            ("request", "response", accepted),
            ("rejectedRequest", "rejectedResponse", rejected),
        ],
    ));

    let toml = "plan_id = \"lc1:aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa:bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb:cccccccc-cccc-4ccc-8ccc-cccccccccccc\"\n\n[[entries]]\nname = \"extremes\"\nkind = \"node\"\nty_name = \"Task\"\n\n[entries.payload]\ntitle = \"Bare integers decode in Rust\"\nmaximum = 9223372036854775807\nminimum = -9223372036854775808\nnested = [1, { kind = \"record\", value = { at = { kind = \"timestamp\", value = \"2026-10-15T08:30:00.000000001Z\" } } }]\n\n[[entries]]\nname = \"extremes_link\"\nkind = \"edge\"\nty_name = \"DependsOn\"\nfrom = { added = \"extremes\" }\nto = { existing = { origin = \"11111111-1111-4111-8111-111111111111\", position = \"18446744073709551615\" } }\n";
    let decoded = recorder.call(
        "decode",
        "proposal.decode",
        json!({"text": toml, "format": "toml"}),
    );
    modules.push(method_module(
        "proposal.decode",
        vec![("request", "response", decoded.clone())],
    ));
    let encoded = recorder.call(
        "encode",
        "proposal.encode",
        json!({"proposal": decoded.1["result"], "format": "json"}),
    );
    modules.push(method_module(
        "proposal.encode",
        vec![("request", "response", encoded)],
    ));

    let added = recorder.call(
        "add",
        "analyzers.add",
        json!({"name": DagCheckFactory::name(), "configuration": r#"{"on_cycle":"inconsistency"}"#,
            "evaluation_time": EVALUATION_TIME}),
    );
    modules.push(method_module(
        "analyzers.add",
        vec![("request", "response", added)],
    ));
    let described = recorder.call("describe", "database.describe", json!({}));
    modules.push(method_module(
        "database.describe",
        vec![("request", "response", described)],
    ));

    let covered = modules
        .iter()
        .map(|module| module.file.trim_end_matches(".ts").to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        covered,
        Method::ALL.iter().map(|m| m.name().to_string()).collect()
    );
    modules.push(index_module());
    modules.push(application_errors(
        &mut recorder,
        scratch,
        applied,
        dismissal_list,
    ));
    modules.push(protocol_errors(&mut recorder));
    modules.push(http_rejections(scratch, &mut recorder.normalizer).await);
    modules
}

fn application_errors(
    recorder: &mut Recorder,
    scratch: &Scratch,
    applied: ApplyResult,
    dismissed: DismissalList,
) -> Module {
    let mut samples: Vec<Value> = vec![];
    let call = |recorder: &mut Recorder, method: &str, params: Value| {
        let response = recorder.raw(&call_text("call", method, params).to_string());
        assert!(response["error"].is_object(), "{method}: {response}");
        response
    };
    let task = |name: &str| json!({"name": name, "kind": "node", "ty_name": "Task", "payload": {"title": name}});
    // Decoding raw text keeps the line and column; an embedded `proposal`
    // parameter drops them.
    samples.push(call(
        recorder,
        "proposal.decode",
        json!({"text": "[[entries]]\nname = \"n\"\nkind = \"node\"\nty_name = \"Task\"\npayload = { n = { kind = \"integer\", value = \"01\" } }\n", "format": "toml"}),
    ));
    samples.push(call(recorder, "proposal.check", json!({"proposal": {"entries": [
        {"name": "n", "kind": "node", "ty_name": "Task", "payload": {"n": {"kind": "integer", "value": "01"}}},
    ]}})));
    samples.push(call(
        recorder,
        "proposal.check",
        json!({"proposal": {"entries": [task("twice"), task("twice")]}}),
    ));
    samples.push(recorder.error(
        "invalid_without_location",
        &AppError::InvalidProposal {
            location: None,
            message: "plan reference 3 does not name an earlier entry operation".into(),
        },
    ));
    let key = json!({"database": Uuid::nil(), "analyzer": {"name": "Fixture_Suggestions", "version": "1.0.0", "configuration": "{}"},
        "kind": "suggestion", "analyzer_certificate": "done:1"});
    samples.push(call(
        recorder,
        "proposal.apply",
        json!({"proposal": {"acknowledges": key, "entries": []}}),
    ));
    let current = recorder.raw_result("plan_id.mint", json!({}))["plan_id"]
        .as_str()
        .unwrap()
        .to_string();
    let parts = current.split(':').collect::<Vec<_>>();
    let foreign = format!("lc1:{}:{}:{}", Uuid::new_v4(), parts[2], parts[3]);
    samples.push(call(
        recorder,
        "proposal.check",
        json!({"proposal": {"plan_id": foreign, "entries": []}}),
    ));
    let stale = format!("lc1:{}:{}:{}", parts[1], Uuid::new_v4(), parts[3]);
    samples.push(call(
        recorder,
        "proposal.check",
        json!({"proposal": {"plan_id": stale, "entries": []}}),
    ));
    samples.push(call(
        recorder,
        "proposal.apply",
        json!({"proposal": {"entries": [
            {"name": "stray", "kind": "node", "ty_name": "Undeclared", "payload": {"title": "Stray"}},
        ]}}),
    ));
    samples.push(call(
        recorder,
        "analyzers.add",
        json!({"name": "Unregistered"}),
    ));
    samples.push(call(
        recorder,
        "analyzers.add",
        json!({"name": Scripted::Failing.name()}),
    ));
    samples.push(call(
        recorder,
        "entries.get",
        json!({"id": {"origin": Uuid::new_v4(), "position": usize::MAX.to_string()}}),
    ));
    samples.push(call(recorder, "entries.list", json!({"as_of": "99"})));
    samples.push(call(recorder, "entries.list", json!({"currentness": null})));
    samples.push(call(
        recorder,
        "views.query",
        json!({"seeds": [], "expand": [{"hops": 0}]}),
    ));
    samples.push(call(
        recorder,
        "analyzers.add",
        json!({"name": Scripted::Panicking.name()}),
    ));
    samples.push(call(
        recorder,
        "findings.dismiss",
        json!({"key": {"database": Uuid::nil(),
            "analyzer": {"name": TypeCheckFactory::name(), "version": "0.1.0", "configuration": "{}"},
            "kind": "inconsistency", "analyzer_certificate": "{\"UnknownType\":{\"subject\":{\"origin\":\"00000000-0000-0000-0000-000000000000\",\"position\":\"3\"},\"ty_name\":\"Undeclared\"}}"},
            "reason": "acknowledged"}),
    ));
    {
        let lock = hold_writer_lock(&scratch.state());
        let quick = service(scratch.state()).with_busy_retry(Duration::ZERO);
        let response = serde_json::to_value(respond(
            &quick,
            call_text(
                "call",
                "proposal.apply",
                json!({"proposal": {"entries": []}}),
            )
            .as_bytes(),
        ))
        .unwrap();
        drop(lock);
        samples.push(recorder.normalizer.value(response));
    }
    let elsewhere = |name: &str| StatePath::new(scratch.0.join(name));
    let describe = |recorder: &mut Recorder, state: StatePath| {
        let response = serde_json::to_value(respond(
            &service(state),
            call_text("call", "database.describe", json!({})).as_bytes(),
        ))
        .unwrap();
        recorder.normalizer.value(response)
    };
    samples.push(describe(recorder, elsewhere("missing.json")));
    fs::write(scratch.0.join("garbage.json"), "not a snapshot").unwrap();
    samples.push(describe(recorder, elsewhere("garbage.json")));
    fs::copy(scratch.state().path(), scratch.0.join("locked.json")).unwrap();
    fs::create_dir(scratch.0.join("locked.json.lock")).unwrap();
    let response = serde_json::to_value(respond(
        &service(elsewhere("locked.json")),
        call_text(
            "call",
            "proposal.apply",
            json!({"proposal": {"entries": []}}),
        )
        .as_bytes(),
    ))
    .unwrap();
    samples.push(recorder.normalizer.value(response));
    // Errors outside RPC dispatch, or that need a fault this test cannot
    // cause portably, are serialized from constructed values.
    let host_errors = [
        StatePath::resolve_from(Some("".into()), &scratch.0).unwrap_err(),
        AppError::WorkingDirectory {
            message: "No such file or directory (os error 2)".into(),
        },
        service(StatePath::new(
            scratch.0.join("absent").join(DEFAULT_STATE_FILE),
        ))
        .init(&[])
        .unwrap_err(),
        service(scratch.state()).init(&[]).unwrap_err(),
        AppError::PublicationFailed {
            message: "No space left on device (os error 28)".into(),
        },
        AppError::PublicationUncertain {
            message: "Input/output error (os error 5)".into(),
            outcome: Box::new(Published::Applied(applied)),
        },
        AppError::PublicationUncertain {
            message: "Input/output error (os error 5)".into(),
            outcome: Box::new(Published::Dismissed(dismissed)),
        },
        AppError::GraphvizUnavailable {
            message: "No such file or directory (os error 2)".into(),
        },
        AppError::GraphvizFailed {
            message: "dot exited with exit status: 1; stderr: Error: <stdin>: syntax error in line 1 near 'x'".into(),
        },
        AppError::Internal {
            message: "the result could not be serialized".into(),
        },
    ];
    for error in &host_errors {
        listed(error);
        samples.push(recorder.error(error.kind(), error));
    }
    let mut application = BTreeMap::<String, Vec<Value>>::new();
    for sample in samples {
        let kind = sample["error"]["data"]["kind"]
            .as_str()
            .unwrap()
            .to_string();
        application.entry(kind).or_default().push(sample);
    }
    assert_eq!(
        application
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(ERROR_KINDS)
    );
    module(
        "errors.ts".into(),
        &["ApplicationErrorFixtures"],
        vec![(
            "application",
            "ApplicationErrorFixtures".into(),
            serde_json::to_value(application).unwrap(),
        )],
    )
}

fn protocol_errors(recorder: &mut Recorder) -> Module {
    let samples = ["{", "[]", r#"{"jsonrpc":"2.0","id":"x","method":"init"}"#]
        .iter()
        .map(|body| recorder.raw(body))
        .collect::<Vec<_>>();
    let codes = samples
        .iter()
        .map(|s| s["error"]["code"].clone())
        .collect::<Vec<_>>();
    assert_eq!(codes, [json!(-32700), json!(-32600), json!(-32601)]);
    module(
        "protocol.ts".into(),
        &["ProtocolError", "RpcFailure"],
        vec![(
            "protocol",
            "RpcFailure<ProtocolError>[]".into(),
            Value::Array(samples),
        )],
    )
}

async fn http_rejections(scratch: &Scratch, normalizer: &mut Normalizer) -> Module {
    let router = router(shared(service(scratch.state()), PORT, Ui::Embedded));
    let valid = valid_headers(PORT);
    let describe = call_text("describe", "database.describe", json!({}));
    let cases = [
        with(valid.clone(), "host", Some("rebound.example:4242")),
        with(valid.clone(), "origin", Some("http://evil.example")),
        with(valid.clone(), "authorization", None),
        with(valid.clone(), "content-type", Some("text/plain")),
    ];
    let mut rejections = BTreeMap::<String, Vec<Value>>::new();
    let mut record = |status: StatusCode, body: &[u8]| {
        let body = json_body(body);
        rejections
            .entry(body["kind"].as_str().unwrap().into())
            .or_default()
            .push(json!({"status": status.as_u16(), "body": body}));
    };
    for headers in cases {
        let (status, _, body) =
            send(&router, request("POST", "/rpc", &headers, describe.clone())).await;
        record(status, &body);
    }
    let (status, _, body) = send(
        &router,
        request("POST", "/rpc", &valid, vec![b' '; BODY_LIMIT + 1]),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    record(status, &body);
    let (status, _, body) = send(&router, request("POST", "/rpc", &valid, failing_body())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    record(status, &body);
    let rejections = serde_json::to_value(rejections).unwrap();
    module(
        "http.ts".into(),
        &["HttpRejectionFixtures"],
        vec![(
            "rejections",
            "HttpRejectionFixtures".into(),
            normalizer.value(rejections),
        )],
    )
}

fn fixture_directory() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/ui/src/fixtures"))
}

// Writes the fixtures the frontend type-check reads. Files are rewritten
// only when stale, so an unchanged run leaves `ui/src` untouched and does not
// rebundle the client; a stale or extra file fails the test after it was
// refreshed, so the change is reviewed and committed.
#[tokio::test(flavor = "multi_thread")]
async fn typescript_fixtures_match_the_rust_shapes() {
    let scratch = Scratch::new();
    let modules = fixtures(&scratch).await;
    let directory = fixture_directory();
    fs::create_dir_all(directory).unwrap();
    let mut stale = vec![];
    let mut expected = BTreeSet::new();
    for module in &modules {
        let path = directory.join(&module.file);
        expected.insert(module.file.clone());
        if fs::read_to_string(&path).ok().as_deref() != Some(module.text.as_str()) {
            fs::write(&path, &module.text).unwrap();
            stale.push(module.file.clone());
        }
    }
    for entry in fs::read_dir(directory).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if !expected.contains(&name) {
            fs::remove_file(directory.join(&name)).unwrap();
            stale.push(format!("{name} (removed)"));
        }
    }
    assert!(
        stale.is_empty(),
        "stale fixtures in {} were rewritten: {stale:?}; run the frontend type-check, update contract.ts if it fails, and commit them",
        directory.display()
    );
}
