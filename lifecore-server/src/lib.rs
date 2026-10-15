mod access;
mod assets;

use access::{Access, Reason, Token};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State, rejection::BytesRejection},
    http::header,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use lifecore_app::{AppError, Service, rpc};
use serde_json::Value;
use std::{
    any::Any,
    future::Future,
    io,
    net::Ipv4Addr,
    panic::{self, AssertUnwindSafe},
    sync::Arc,
};
use tokio::{
    net::TcpListener,
    signal::unix::{SignalKind, signal},
};
use tokio_util::task::TaskTracker;

pub use assets::Ui;

// Far beyond any plausible change file; a larger body is rejected before
// dispatch.
const BODY_LIMIT: usize = 64 * 1024 * 1024;

pub struct Options {
    // Zero lets the operating system choose a free port.
    pub port: u16,
    pub ui: Ui,
}

// Everything a request can reach. The service keeps no state between calls;
// `tasks` only tracks running calls so shutdown can wait for them.
#[derive(Clone)]
struct Shared {
    service: Arc<Service>,
    access: Arc<Access>,
    ui: Arc<Ui>,
    tasks: TaskTracker,
}

// Binds the loopback interface, reports the client URL with a fresh launch
// token to `ready`, and serves until SIGINT or SIGTERM. Shutdown stops
// accepting connections and returns once every running call has finished.
pub fn serve(service: Service, options: Options, ready: impl FnOnce(&str)) -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        // Signal handlers are in place before the URL is announced, so a
        // launcher may stop the server as soon as it has read the URL.
        let shutdown = shutdown_signal()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, options.port)).await?;
        let port = listener.local_addr()?.port();
        let token = Token::generate()?;
        ready(&format!(
            "http://127.0.0.1:{port}/#token={}",
            token.reveal()
        ));
        let tasks = TaskTracker::new();
        let app = router(Shared {
            service: Arc::new(service),
            access: Arc::new(Access::new(port, token)),
            ui: Arc::new(options.ui),
            tasks: tasks.clone(),
        });
        run(listener, app, tasks, shutdown).await
    })
}

fn router(shared: Shared) -> Router {
    let rpc = post(call).layer(middleware::from_fn_with_state(
        shared.clone(),
        access::require_token_and_json,
    ));
    Router::new()
        .route("/rpc", rpc)
        .route("/", get(assets::index))
        .route("/app.js", get(assets::script))
        .fallback(assets::not_found)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn_with_state(
            shared.clone(),
            access::require_local_origin,
        ))
        .with_state(shared)
}

async fn run(
    listener: TcpListener,
    app: Router,
    tasks: TaskTracker,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;
    // Calls whose clients disconnected are still running; publication must
    // not be cut short by the process exiting.
    tasks.close();
    if !tasks.is_empty() {
        eprintln!(
            "lifecore: waiting for {} running request(s) to finish",
            tasks.len()
        );
    }
    tasks.wait().await;
    Ok(())
}

// The first SIGINT or SIGTERM starts an orderly shutdown; a second one exits
// at once, which atomic publication makes safe for the stored snapshot.
fn shutdown_signal() -> io::Result<impl Future<Output = ()> + Send + 'static> {
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = interrupt.recv() => {}
            _ = terminate.recv() => {}
        }
        eprintln!("lifecore: stopping after running requests finish; interrupt again to stop now");
        tokio::spawn(async move {
            tokio::select! {
                _ = interrupt.recv() => {}
                _ = terminate.recv() => {}
            }
            std::process::exit(130);
        });
    })
}

// Reported when the connection closed before the response was ready. The
// blocking task keeps running and its outcome stands; nothing is rolled back.
struct Pending {
    answered: bool,
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.answered {
            eprintln!(
                "lifecore: a client disconnected before its response; the request runs to completion and its outcome is not reported"
            );
        }
    }
}

// One blocking task per call, running the shared decoder and dispatcher on
// the synchronous service exactly as one CLI invocation would. The task is
// not cancelled when the client goes away.
async fn call(State(shared): State<Shared>, body: Result<Bytes, BytesRejection>) -> Response {
    let body = match body {
    Ok(body) => body,
    Err(rejection) => {
        return access::reject(rejection.status(), Reason::Body, &rejection.body_text());
    }
    };
    let service = shared.service.clone();
    let mut pending = Pending { answered: false };
    let task = shared
        .tasks
        .spawn_blocking(move || encode(&respond(&service, &body)));
    // A join error means the task never produced a response, so its request
    // ID is unknown here.
    let bytes = task.await.unwrap_or_else(|error| {
        encode(&rpc::Response::error(
            Value::Null,
            &AppError::Backend {
                message: error.to_string(),
            },
        ))
    });
    pending.answered = true;
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response()
}

// A panic is caught after the envelope was read, so the structured error
// carries the caller's request ID. Builds with `panic = "abort"`, such as
// this workspace's dev profile, end the process instead.
fn respond(service: &Service, body: &[u8]) -> rpc::Response {
    let text = match std::str::from_utf8(body) {
    Ok(text) => text,
    Err(error) => {
        return rpc::parse_error(format!("parse error: the body is not UTF-8: {error}"));
    }
    };
    let request = match rpc::parse_request(text) {
    Ok(request) => request,
    Err(response) => return response,
    };
    let id = request.id.clone();
    panic::catch_unwind(AssertUnwindSafe(|| request.respond(service))).unwrap_or_else(|panic| {
        rpc::Response::error(
            id,
            &AppError::Backend {
                message: panic_message(panic.as_ref()),
            },
        )
    })
}

fn panic_message(panic: &(dyn Any + Send)) -> String {
    match (panic.downcast_ref::<&str>(), panic.downcast_ref::<String>()) {
    (Some(message), _) => format!("it panicked: {message}"),
    (_, Some(message)) => format!("it panicked: {message}"),
    _ => "it panicked".into(),
    }
}

fn encode(response: &rpc::Response) -> Vec<u8> {
    serde_json::to_vec(response).expect("a JSON-RPC response is JSON")
}

#[cfg(test)]
mod test;
