use std::path::PathBuf;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use colored::Colorize;
use tokio::fs;
use tower_http::{
    cors::{self, CorsLayer},
    limit::RequestBodyLimitLayer,
};

use crate::{cli, server, upload};

const INDEX_HTML: &'static str = include_str!("index.html");
const FAVICON_SVG: &'static str = include_str!("favicon.svg");

async fn index() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], INDEX_HTML)
}

async fn favicon() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], FAVICON_SVG)
}

async fn handle_upload(State(upload_dir): State<PathBuf>, mut multipart: Multipart) -> impl IntoResponse {
    while let Ok(Some(field)) = multipart.next_field().await {
        let file_name = match field.file_name() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };

        let data = match field.bytes().await {
            Ok(d) => d,
            Err(err) => {
                eprintln!("Error reading field: {err}");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
        };

        if let Err(err) = fs::create_dir_all(&upload_dir).await {
            eprintln!("Failed to create directory at '{}': {err}", upload_dir.display());
            return StatusCode::INTERNAL_SERVER_ERROR;
        }

        match upload::save_file(&upload_dir, &file_name, &data).await {
            Ok(dest) => {
                let ts = chrono::Local::now().format("%H:%M:%S");
                println!(
                    "{} {} '{}' ({} bytes)",
                    format!("[{ts}]").dimmed(),
                    "FILE".green().bold(),
                    dest.file_name().unwrap_or_default().display(),
                    data.len()
                );
            }
            Err(err) => {
                eprintln!("Failed to write file '{file_name}': {err}");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
        }
    }
    StatusCode::OK
}

async fn handle_message(mut multipart: Multipart) -> impl IntoResponse {
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("message") {
            if let Ok(text) = field.text().await {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let ts = chrono::Local::now().format("%H:%M:%S");
                    println!("{} {}", format!("[{ts}]").dimmed(), "MESSAGE".yellow().bold());
                    println!("{}", trimmed);
                }
            }
        }
    }
    StatusCode::OK
}

pub async fn run(args: cli::ServeArgs) -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/", get(index))
        .route("/favicon.svg", get(favicon))
        .route("/upload", post(handle_upload))
        .route("/message", post(handle_message))
        .layer(DefaultBodyLimit::disable());
    let app = match args.max_body_size {
        Some(size) => app.layer(RequestBodyLimitLayer::new(size)),
        None => app,
    };
    let app = app
        .layer(CorsLayer::new().allow_origin(cors::Any))
        .with_state(args.output.clone());

    server::run(
        app,
        args.port,
        args.no_tls,
        axum_server::Handle::new(),
        server::Entry::Serve { output: args.output },
    )
    .await
}
