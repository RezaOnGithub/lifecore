use crate::Shared;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::{fmt, io};
use subtle::ConstantTimeEq;

// The per-launch secret. Debug never prints it; only the startup URL does.
pub(crate) struct Token(String);

impl Token {
    // 256 bits from the operating system's CSPRNG, as lowercase hex.
    pub(crate) fn generate() -> io::Result<Self> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        Ok(Self(
            bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        ))
    }
    pub(crate) fn reveal(&self) -> &str {
        &self.0
    }
    #[cfg(test)]
    pub(crate) fn new(token: &str) -> Self {
        Self(token.into())
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

// What the adapter accepts before dispatch (INTERFACES §4): the bound
// loopback authority under either name, and the launch token.
#[derive(Debug)]
pub(crate) struct Access {
    hosts: [String; 2],
    token: Token,
}

impl Access {
    pub(crate) fn new(port: u16, token: Token) -> Self {
        Self {
            hosts: [format!("127.0.0.1:{port}"), format!("localhost:{port}")],
            token,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reason {
    Host,
    Origin,
    Token,
    ContentType,
    Body,
}

// The body of every HTTP rejection, so the client can tell an access failure
// from a JSON-RPC error without parsing prose.
#[derive(Serialize)]
pub(crate) struct Rejection<'a> {
    pub(crate) kind: Reason,
    pub(crate) message: &'a str,
}

pub(crate) fn reject(status: StatusCode, kind: Reason, message: &str) -> Response {
    let body = serde_json::to_vec(&Rejection { kind, message }).expect("rejection JSON");
    let mut response = (
        status,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response();
    if kind == Reason::Token {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    response
}

// A repeated header is ambiguous, so it is treated like a wrong one.
fn single(headers: &HeaderMap, name: HeaderName) -> Result<Option<&HeaderValue>, ()> {
    let mut values = headers.get_all(name).iter();
    match (values.next(), values.next()) {
    (value, None) => Ok(value),
    _ => Err(()),
    }
}

// Every route, assets included: a rebinding host name or a foreign page never
// reaches an asset or the dispatcher. The `Host` check defeats DNS rebinding;
// an `Origin`, when sent, must be the page's own origin under that host.
pub(crate) async fn require_local_origin(
    State(shared): State<Shared>,
    request: Request,
    next: Next,
) -> Response {
    let access = &shared.access;
    let host = single(request.headers(), header::HOST)
        .ok()
        .flatten()
        .and_then(|host| host.to_str().ok())
        .filter(|host| access.hosts.iter().any(|allowed| allowed == host));
    let authority = request
        .uri()
        .authority()
        .map(|authority| authority.as_str());
    let Some(host) = host.filter(|host| authority.is_none_or(|authority| authority == *host))
    else {
        return reject(
            StatusCode::MISDIRECTED_REQUEST,
            Reason::Host,
            &format!(
                "the Host must be {} or {}",
                access.hosts[0], access.hosts[1]
            ),
        );
    };
    let origin = format!("http://{host}");
    match single(request.headers(), header::ORIGIN) {
    Ok(None) => {}
    Ok(Some(value)) if value.as_bytes() == origin.as_bytes() => {}
    _ => {
        return reject(
            StatusCode::FORBIDDEN,
            Reason::Origin,
            &format!("requests must come from {origin}"),
        );
    }
    }
    next.run(request).await
}

// `/rpc` only, after the Host and Origin checks and before the body is read.
// The token is compared in constant time. A JSON content type keeps HTML
// forms out, and a cross-origin fetch that sets it, or the Authorization
// header, needs a CORS preflight that is never approved.
pub(crate) async fn require_token_and_json(
    State(shared): State<Shared>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let presented = single(headers, header::AUTHORIZATION)
        .ok()
        .flatten()
        .and_then(|value| bearer(value.as_bytes()));
    let expected = shared.access.token.reveal().as_bytes();
    if !presented.is_some_and(|token| bool::from(token.ct_eq(expected))) {
        return reject(
            StatusCode::UNAUTHORIZED,
            Reason::Token,
            "missing or wrong launch token; open the URL that `lifecore serve` printed at its latest start",
        );
    }
    let json = single(headers, header::CONTENT_TYPE)
        .ok()
        .flatten()
        .and_then(|value| value.to_str().ok())
        .is_some_and(is_json);
    if !json {
        return reject(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Reason::ContentType,
            "the request body must be declared as application/json",
        );
    }
    next.run(request).await
}

// `Bearer <token>`; the scheme is case-insensitive (RFC 9110 11.1).
fn bearer(value: &[u8]) -> Option<&[u8]> {
    let (scheme, token) = value.split_at_checked(7)?;
    scheme.eq_ignore_ascii_case(b"bearer ").then_some(token)
}

// The media type's essence, ignoring parameters such as `charset`.
fn is_json(value: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|essence| essence.trim().eq_ignore_ascii_case("application/json"))
}
