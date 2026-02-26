use std::{env, net::SocketAddr, path::PathBuf};

use axum::{
    Router,
    extract::{DefaultBodyLimit, Multipart},
    http::{StatusCode, header},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use axum_server::tls_rustls::RustlsConfig;
use chrono::Local;
use colored::Colorize;
use tap::Pipe;
use tokio::{fs, net::TcpListener};
use tower_http::{
    cors::{Any, CorsLayer},
    limit::RequestBodyLimitLayer,
};
use uuid::Uuid;

const INDEX_HTML: &'static str = include_str!("index.html");
const FAVICON_SVG: &'static str = include_str!("favicon.svg");

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn favicon() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/svg+xml")], FAVICON_SVG)
}

async fn handle_upload(
    axum::extract::State(upload_dir): axum::extract::State<PathBuf>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    while let Ok(Some(field)) = multipart.next_field().await {
        let file_name = match field.file_name() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };

        let data = match field.bytes().await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Error reading field: {e}");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
        };

        let stem = PathBuf::from(&file_name);
        let stem = stem.file_stem().unwrap_or_default().to_string_lossy();
        let ext = PathBuf::from(&file_name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();

        let unique = format!("{}-{}{}", stem, &Uuid::new_v4().to_string()[..8], ext);
        let dest = upload_dir.join(&unique);

        match fs::write(&dest, &data).await {
            Ok(_) => {
                let ts = Local::now().format("%H:%M:%S");
                println!(
                    "{} {} {} ({} bytes)",
                    format!("[{ts}]").dimmed(),
                    "FILE".green().bold(),
                    unique,
                    data.len()
                );
            }
            Err(e) => {
                eprintln!("Failed to write file {unique}: {e}");
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
                    let ts = Local::now().format("%H:%M:%S");
                    println!(
                        "{} {}\n  {}",
                        format!("[{ts}]").dimmed(),
                        "MESSAGE".yellow().bold(),
                        trimmed.white(),
                    );
                }
            }
        }
    }
    StatusCode::OK
}

fn print_entry(no_tls: bool, port: u16, upload_dir: &PathBuf) {
    let local_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "<your-ip>".to_string());

    let protocol = if no_tls { "http" } else { "https" };
    let local_address = format!("{}://localhost:{}", protocol, port);
    let network_address = format!("{}://{}:{}", protocol, local_ip, port);

    println!("{}", "╔══════════════════════════════════╗".purple());
    println!("{}", "║             DropZone 🚀          ║".purple());
    println!("{}", "╚══════════════════════════════════╝".purple());

    if no_tls {
        println!("{}", "Running in insecure mode".yellow().bold());
    }

    println!("  {} {}", "Local:".bold(), local_address);
    println!("  {} {}", "Network:".bold(), network_address);
    println!("  {} {}", "Uploads:".bold(), upload_dir.display());
    println!("{}", "  Waiting for connections...".dimmed());
    println!();
}

#[tokio::main]
async fn main() {
    let curr_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let upload_dir = if env::args().any(|a| a == "--flat") {
        curr_dir.clone()
    } else {
        curr_dir.join("dropzone-uploads")
    };

    std::fs::create_dir_all(&upload_dir).expect("Cannot create dropzone directory");

    let cors = CorsLayer::new().allow_origin(Any);
    let max_size = env::var("DROPZONE_MAX_BODY_SIZE")
        .ok()
        .and_then(|a| a.parse().ok());

    let app = Router::new()
        .route("/", get(index))
        .route("/favicon.svg", get(favicon))
        .route("/upload", post(handle_upload))
        .route("/message", post(handle_message))
        .layer(DefaultBodyLimit::disable())
        .pipe(|router| match max_size {
            Some(size) => router.layer(RequestBodyLimitLayer::new(size)),
            None => router,
        })
        .layer(cors)
        .with_state(upload_dir.clone());

    let port: u16 = env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(8080);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let no_tls = env::args().any(|a| a == "--no-tls");

    print_entry(no_tls, port, &upload_dir);

    if no_tls {
        let listener = TcpListener::bind(addr).await.expect("Failed to bind port");
        axum::serve(listener, app).await.unwrap();
    } else {
        let cert_path = env::var("DROPZONE_CERT_PATH")
            .unwrap_or_else(|_| curr_dir.join("cert.crt").to_string_lossy().into_owned());
        let cert_key_path = env::var("DROPZONE_CERT_KEY_PATH")
            .unwrap_or_else(|_| curr_dir.join("cert.key").to_string_lossy().into_owned());

        let config = RustlsConfig::from_pem_file(cert_path, cert_key_path)
            .await
            .expect("Failed to read cert data");

        axum_server::bind_rustls(addr, config)
            .serve(app.into_make_service())
            .await
            .unwrap();
    }
}
