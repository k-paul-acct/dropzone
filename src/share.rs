use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, anyhow};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use percent_encoding::utf8_percent_encode;
use tokio::{fs::File, sync::Mutex};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::{cli::ShareArgs, server, web};

struct SharedFile {
    file: Mutex<Option<File>>,
    headers: HeaderMap,
    page: String,
    handle: axum_server::Handle<SocketAddr>,
}

fn download_headers(name: &str, length: u64) -> HeaderMap {
    let mut headers = HeaderMap::new();

    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(header::CONTENT_LENGTH, length.into());
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"download\"; filename*=UTF-8''{}",
            utf8_percent_encode(name, percent_encoding::NON_ALPHANUMERIC),
        ))
        .unwrap(),
    );

    headers
}

fn render_page(name: &str, length: u64, download_url: &str) -> String {
    fn escape_html(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }

    fn file_size(length: u64) -> String {
        if length < 1024 {
            return format!("{length} bytes");
        }
        let mut size = length as f64;
        for unit in ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"] {
            size /= 1024.0;
            if size < 1024.0 || unit == "EiB" {
                return format!("{size:.1} {unit} ({length} bytes)");
            }
        }
        unreachable!()
    }

    include_str!("share.html")
        .replace("{{file_name}}", &escape_html(name))
        .replace("{{file_size}}", &file_size(length))
        .replace("{{download_url}}", &escape_html(download_url))
}

async fn page(State(state): State<Arc<SharedFile>>) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        state.page.clone(),
    )
}

async fn head(State(state): State<Arc<SharedFile>>) -> impl IntoResponse {
    state.headers.clone()
}

async fn download(State(state): State<Arc<SharedFile>>) -> Response {
    let Some(file) = state.file.lock().await.take() else {
        return StatusCode::GONE.into_response();
    };

    state.handle.graceful_shutdown(None);
    (state.headers.clone(), Body::from_stream(ReaderStream::new(file))).into_response()
}

pub async fn run(args: ShareArgs) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(&args.path)
        .await
        .with_context(|| format!("Could not open file '{}'", args.path.display()))?;
    let metadata = file.metadata().await.context("Could not read shared file metadata")?;
    if !metadata.is_file() {
        return Err(anyhow!("'{}' is not a regular file", args.path.display()).into());
    }
    let name = args.path.file_name().unwrap_or_default().to_string_lossy();
    let path = format!("/{}", Uuid::new_v4());
    let handle = axum_server::Handle::new();

    let app = Router::new()
        .route(&path, get(page))
        .route(&format!("{path}/download"), get(download).head(head))
        .with_state(Arc::new(SharedFile {
            file: Mutex::new(Some(file)),
            headers: download_headers(&name, metadata.len()),
            page: render_page(&name, metadata.len(), &format!("{path}/download")),
            handle: handle.clone(),
        }))
        .merge(web::assets());

    server::run(
        app,
        args.port,
        args.no_tls,
        handle,
        server::Entry::Share { file: args.path, path },
    )
    .await?;

    println!("Share consumed. Server stopped.");

    Ok(())
}
