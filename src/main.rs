use anyhow::Context;
use axum::{
    Router,
    extract::{DefaultBodyLimit, Multipart, State},
    handler::HandlerWithoutStateExt,
    http::{StatusCode, Uri, header, uri},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use axum_server::tls_rustls::RustlsConfig;
use chrono::Local;
use clap::Parser;
use colored::Colorize;
use std::{env, net::SocketAddr, path::PathBuf};
use tap::Pipe;
use tokio::fs;
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
    /// HTTP port to listen to.
    #[arg(default_value_t = 8080)]
    port: u16,

    /// HTTPS port to listen to.
    #[arg(long, default_value_t = 8443)]
    https_port: u16,

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

struct Ports {
    http: u16,
    https: u16,
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

    let ports = Ports {
        http: args.port,
        https: args.https_port,
    };

    if args.no_tls {
        let addr = SocketAddr::from(([0, 0, 0, 0], ports.http));
        let handle = axum_server::Handle::new();

        tokio::spawn(listening_http(handle.clone(), args.output.clone()));

        axum_server::bind(addr)
            .handle(handle)
            .serve(app.into_make_service())
            .await
            .with_context(|| "error in server")?;
    } else {
        let cert_path = env::var("DROPZONE_CERT_PATH").unwrap_or_else(|_| "./cert.pem".to_string());
        let cert_key_path = env::var("DROPZONE_CERT_KEY_PATH").unwrap_or_else(|_| "./key.pem".to_string());
        let config = RustlsConfig::from_pem_file(cert_path, cert_key_path)
            .await
            .with_context(|| "could not read cert data")?;

        let addr = SocketAddr::from(([0, 0, 0, 0], ports.https));
        let http_handle = axum_server::Handle::new();
        let https_handle = axum_server::Handle::new();

        tokio::spawn(listening_https(
            http_handle.clone(),
            https_handle.clone(),
            args.output.clone(),
        ));

        tokio::spawn(redirect_to_https(ports, http_handle));

        axum_server::bind_rustls(addr, config)
            .handle(https_handle)
            .serve(app.into_make_service())
            .await
            .with_context(|| "error in server")?;
    }

    Ok(())
}

fn print_entry(http_addr: SocketAddr, https_addr: Option<SocketAddr>, output: PathBuf) {
    let local_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "<your-ip>".to_string());

    let local_addr = match https_addr {
        Some(https_addr) => format!(
            "http://localhost:{}, https://localhost:{}",
            http_addr.port(),
            https_addr.port()
        ),
        None => format!("http://localhost:{}", http_addr.port()),
    };

    let network_addr = match https_addr {
        Some(https_addr) => format!(
            "http://{}:{}, https://{}:{}",
            local_ip,
            http_addr.port(),
            local_ip,
            https_addr.port()
        ),
        None => format!("http://{}:{}", local_ip, http_addr.port()),
    };

    println!("{}", "╔══════════════════════════════════╗".purple());
    println!("{}", "║             DropZone 🚀          ║".purple());
    println!("{}", "╚══════════════════════════════════╝".purple());

    if https_addr.is_some() {
        println!("  {}", "Running in secure HTTPS mode".green().bold());
    } else {
        println!("  {}", "Running in insecure HTTP-only mode".red().bold());
    }

    println!("  {} {}", "Local:".bold(), local_addr);
    println!("  {} {}", "Network:".bold(), network_addr);
    println!("  {} {}", "Uploads:".bold(), output.display());
    println!("  {}", "Waiting for connections...".dimmed());
    println!();
}

async fn listening_http(handle: axum_server::Handle<SocketAddr>, output: PathBuf) {
    if let Some(addr) = handle.listening().await {
        print_entry(addr, None, output);
    }
}

async fn listening_https(
    http_handle: axum_server::Handle<SocketAddr>,
    https_handle: axum_server::Handle<SocketAddr>,
    output: PathBuf,
) {
    match tokio::join!(http_handle.listening(), https_handle.listening()) {
        (Some(http_addr), Some(https_addr)) => {
            print_entry(http_addr, Some(https_addr), output);
        }
        _ => {}
    }
}

async fn redirect_to_https(ports: Ports, handle: axum_server::Handle<SocketAddr>) -> anyhow::Result<()> {
    fn make_https(uri: Uri, authority: uri::Authority) -> Option<Uri> {
        let mut parts = uri.into_parts();

        parts.scheme = Some(uri::Scheme::HTTPS);
        parts.authority = Some(authority);

        if parts.path_and_query.is_none() {
            parts.path_and_query = Some("/".parse().unwrap());
        }

        Uri::from_parts(parts).ok()
    }

    let authority = format!("127.0.0.1:{}", ports.https).parse().unwrap();
    let redirect = move |uri| async move {
        match make_https(uri, authority) {
            Some(uri) => Ok(Redirect::permanent(&uri.to_string())),
            None => Err(StatusCode::BAD_REQUEST),
        }
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], ports.http));

    axum_server::bind(addr)
        .handle(handle)
        .serve(redirect.into_make_service())
        .await
        .with_context(|| "error in server")?;

    Ok(())
}
