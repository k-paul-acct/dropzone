mod local_ip;
mod qr;
mod tls;

use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::Context;
use axum::{
    Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::Local;
use clap::{Args, Parser, Subcommand};
use colored::Colorize;
use qrcode::QrCode;
use tokio::fs;
use tower_http::{
    cors::{Any, CorsLayer},
    limit::RequestBodyLimitLayer,
};
use uuid::Uuid;

use crate::local_ip::best_local_ip;

const INDEX_HTML: &'static str = include_str!("index.html");
const FAVICON_SVG: &'static str = include_str!("favicon.svg");

async fn index() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, mime::TEXT_HTML_UTF_8.as_ref())], INDEX_HTML)
}

async fn favicon() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, mime::IMAGE_SVG.as_ref())], FAVICON_SVG)
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

        if let Err(err) = fs::create_dir_all(&upload_dir).await {
            eprintln!("Failed to create directory at '{}': {err}", upload_dir.display());
            return StatusCode::INTERNAL_SERVER_ERROR;
        }

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
                    println!("{} {}", format!("[{ts}]").dimmed(), "MESSAGE".yellow().bold());
                    println!("{}", trimmed);
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
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start DropZone server.
    Serve(ServeArgs),
}

#[derive(Args)]
struct ServeArgs {
    /// Port to listen to.
    #[arg(default_value_t = 8443)]
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

async fn run_serve(args: ServeArgs) -> Result<(), Box<dyn std::error::Error>> {
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
        .layer(CorsLayer::new().allow_origin(Any))
        .with_state(args.output.clone());

    let local_ip = best_local_ip().context("Could not find suitable local IP")?;

    if args.no_tls {
        let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
        let handle = axum_server::Handle::new();

        tokio::spawn(listening(
            handle.clone(),
            true,
            SocketAddr::new(local_ip, args.port),
            args.output.clone(),
        ));

        axum_server::bind(addr)
            .handle(handle)
            .serve(app.into_make_service())
            .await?
    } else {
        let source = tls::CertificateSource::from_paths(
            env::var_os("DROPZONE_CERT_PATH"),
            env::var_os("DROPZONE_CERT_KEY_PATH"),
        )?;
        let config = source.config(Some(local_ip)).await?;

        match &source {
            tls::CertificateSource::Generated => {
                println!(
                    "{}",
                    "Using a temporary self-signed TLS certificate. Browsers will show a certificate warning.\n\
                     The certificate changes each startup; bypassing the warning does not verify the server's identity."
                        .yellow()
                );
            }
            tls::CertificateSource::Files { cert: _, key: _ } => {
                println!("{}", "Using the specified TLS certificate.".dimmed());
            }
        }

        let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
        let handle = axum_server::Handle::new();

        tokio::spawn(listening(
            handle.clone(),
            false,
            SocketAddr::new(local_ip, args.port),
            args.output.clone(),
        ));

        axum_server::bind_rustls(addr, config)
            .handle(handle)
            .serve(app.into_make_service())
            .await?
    }

    Ok(())
}

fn print_entry(is_http: bool, addr: SocketAddr, output: PathBuf) {
    let protocol = if is_http { "http" } else { "https" };
    let local_addr = format!("{}://127.0.0.1:{}/", protocol, addr.port());
    let network_addr = format!("{}://{}:{}/", protocol, addr.ip(), addr.port());

    println!("{}", "╔══════════════════════════════════╗".purple());
    println!("{}", "║             DropZone 🚀          ║".purple());
    println!("{}", "╚══════════════════════════════════╝".purple());

    if is_http {
        println!("  {}", "Running in insecure HTTP-only mode".red().bold());
    }

    println!("  {} {}", "Local:  ".bold(), local_addr);
    println!("  {} {}", "Network:".bold(), network_addr);
    println!("  {} {}", "Uploads:".bold(), output.display());

    if let Ok(qr) = QrCode::with_error_correction_level(network_addr, qrcode::EcLevel::L) {
        println!("  {}", "QR code for network address:".bold());
        println!();
        for line in qr::render_to_string(&qr).lines() {
            println!("  {}", line.black().on_bright_white());
        }
        println!();
    }

    println!("  {}", "Waiting for connections...".dimmed());
    println!();
}

async fn listening(handle: axum_server::Handle<SocketAddr>, is_http: bool, addr: SocketAddr, output: PathBuf) {
    if handle.listening().await.is_some() {
        print_entry(is_http, addr, output);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Serve(args) => run_serve(args),
    }
    .await
}
