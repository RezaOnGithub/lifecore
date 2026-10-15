use crate::Shared;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::path::PathBuf;

//A Where the client comes from: the copy `build.rs` embedded, or a directory
//A read on every request, so a refresh picks up an `esbuild --watch` rebuild.
#[derive(Clone, Debug)]
pub enum Ui {
    Embedded,
    Directory(PathBuf),
}

pub(crate) const INDEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/index.html"));
pub(crate) const SCRIPT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.js"));

//A Scripts load only from this origin; inline styles in index.html are allowed.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Clone, Copy)]
enum Asset {
    Index,
    Script,
}

impl Asset {
    fn file(self) -> &'static str {
        match self {
        Self::Index => "index.html",
        Self::Script => "app.js",
        }
    }
    fn content_type(self) -> &'static str {
        match self {
        Self::Index => "text/html; charset=utf-8",
        Self::Script => "text/javascript; charset=utf-8",
        }
    }
    fn embedded(self) -> &'static [u8] {
        match self {
        Self::Index => INDEX,
        Self::Script => SCRIPT,
        }
    }
}

pub(crate) async fn index(State(shared): State<Shared>) -> Response {
    serve(&shared.ui, Asset::Index).await
}

pub(crate) async fn script(State(shared): State<Shared>) -> Response {
    serve(&shared.ui, Asset::Script).await
}

pub(crate) async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

async fn serve(ui: &Ui, asset: Asset) -> Response {
    let body = match ui {
    Ui::Embedded => Bytes::from_static(asset.embedded()),
    Ui::Directory(directory) => {
        let path = directory.join(asset.file());
        match tokio::fs::read(&path).await {
        Ok(bytes) => bytes.into(),
        Err(error) => {
            return (
                StatusCode::NOT_FOUND,
                format!("cannot read {}: {error}", path.display()),
            )
                .into_response();
        }
        }
    }
    };
    let mut response = (
        [
            (header::CONTENT_TYPE, asset.content_type()),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    response
}
