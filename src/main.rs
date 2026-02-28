use anyhow::Context;
use axum::{
    Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use axum_server::tls_rustls::RustlsConfig;
use chrono::Local;
use clap::Parser;
use colored::Colorize;
use std::{env, net::SocketAddr, path::PathBuf};
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

async fn handle_upload(State(upload_dir): State<PathBuf>, mut multipart: Multipart) -> impl IntoResponse {
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

/// Easily share files and messages on a local network.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Port to run on.
    #[arg(default_value_t = 8080)]
    port: u16,

    /// Do not use TLS (run in HTTP mode).
    #[arg(long)]
    no_tls: bool,

    /// Save files in the specified directory.
    #[arg(short, long, default_value = "dropzone-uploads")]
    output: PathBuf,

    /// Limit the maximum body size of uploaded files in bytes.
    #[arg(long, env = "DROPZONE_MAX_BODY_SIZE")]
    max_body_size: Option<usize>,
}

fn print_entry(args: &Cli) {
    let local_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "<your-ip>".to_string());

    let protocol = if args.no_tls { "http" } else { "https" };
    let local_address = format!("{}://localhost:{}", protocol, args.port);
    let network_address = format!("{}://{}:{}", protocol, local_ip, args.port);

    println!("{}", "╔══════════════════════════════════╗".purple());
    println!("{}", "║             DropZone 🚀          ║".purple());
    println!("{}", "╚══════════════════════════════════╝".purple());

    if args.no_tls {
        println!("{}", "  Running in insecure mode".red().bold());
    }

    println!("  {} {}", "Local:".bold(), local_address);
    println!("  {} {}", "Network:".bold(), network_address);
    println!("  {} {}", "Uploads:".bold(), args.output.display());
    println!("{}", "  Waiting for connections...".dimmed());
    println!();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

    fs::create_dir_all(&args.output)
        .await
        .with_context(|| format!("could not create output directory `{}`", args.output.display()))?;

    let cors = CorsLayer::new().allow_origin(Any);

    let app = Router::new()
        .route("/", get(index))
        .route("/favicon.svg", get(favicon))
        .route("/upload", post(handle_upload))
        .route("/message", post(handle_message))
        .layer(DefaultBodyLimit::disable())
        .pipe(|a| match args.max_body_size {
            Some(size) => a.layer(RequestBodyLimitLayer::new(size)),
            None => a,
        })
        .layer(cors)
        .with_state(args.output.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));

    print_entry(&args);

    if args.no_tls {
        let listener = TcpListener::bind(addr)
            .await
            .with_context(|| format!("could not bind address `{}`", addr))?;

        axum::serve(listener, app).await.with_context(|| "error in server")?;
    } else {
        let cert_path = env::var("DROPZONE_CERT_PATH").unwrap_or_else(|_| "./cert.crt".to_string());
        let cert_key_path = env::var("DROPZONE_CERT_KEY_PATH").unwrap_or_else(|_| "./cert.key".to_string());
        let config = RustlsConfig::from_pem_file(cert_path, cert_key_path)
            .await
            .with_context(|| "could not read cert data")?;

        axum_server::bind_rustls(addr, config)
            .serve(app.into_make_service())
            .await
            .with_context(|| "error in server")?;
    }

    Ok(())
}
