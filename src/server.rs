use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

use anyhow::Context;
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use colored::Colorize;
use qrcode::QrCode;

use crate::{local_ip::best_local_ip, qr, tls::CertificateSource};

pub enum Entry {
    Serve { output: PathBuf },
    Share { file: PathBuf, path: String },
}

impl Entry {
    fn print(&self, no_tls: bool, addr: SocketAddr) {
        let path = match self {
            Self::Serve { .. } => "/",
            Self::Share { path, .. } => path,
        };
        let (local_url, network_url) = urls(no_tls, addr, path);

        println!("{}", "╔══════════════════════════════════╗".purple());
        println!("{}", "║             DropZone 🚀          ║".purple());
        println!("{}", "╚══════════════════════════════════╝".purple());
        if no_tls {
            println!("  {}", "Running in insecure HTTP-only mode".red().bold());
        }
        println!("  {} {}", "Local:  ".bold(), local_url);
        println!("  {} {}", "Network:".bold(), network_url);
        match self {
            Self::Serve { output } => println!("  {} {}", "Uploads:".bold(), output.display()),
            Self::Share { file, .. } => println!("  {} {}", "Sharing:".bold(), file.display()),
        }
        if let Ok(code) = QrCode::with_error_correction_level(network_url, qrcode::EcLevel::L) {
            println!("  {}", "QR code for network address:".bold());
            println!();

            for line in qr::render_to_string(&code).lines() {
                println!("  {}", line.black().on_bright_white());
            }
            println!();
        }

        let waiting = match self {
            Self::Serve { .. } => "Waiting for connections...",
            Self::Share { .. } => "Waiting for one download...",
        };
        println!("  {}", waiting.dimmed());
        println!();
    }
}

fn urls(no_tls: bool, addr: SocketAddr, path: &str) -> (String, String) {
    let protocol = if no_tls { "http" } else { "https" };
    (
        format!("{protocol}://127.0.0.1:{}{path}", addr.port()),
        format!("{protocol}://{addr}{path}"),
    )
}

async fn tls_config(no_tls: bool, local_ip: IpAddr) -> anyhow::Result<Option<RustlsConfig>> {
    if no_tls {
        return Ok(None);
    }

    let source =
        CertificateSource::from_paths(env::var_os("DROPZONE_CERT_PATH"), env::var_os("DROPZONE_CERT_KEY_PATH"))?;
    let config = source.config(Some(local_ip)).await?;

    match source {
        CertificateSource::Generated => println!(
            "{}",
            "Using a temporary self-signed TLS certificate. Browsers will show a certificate warning.\n\
             The certificate changes each startup; bypassing the warning does not verify the server's identity."
                .yellow()
        ),
        CertificateSource::Files { .. } => println!("{}", "Using the specified TLS certificate.".dimmed()),
    }

    Ok(Some(config))
}

pub async fn run(
    app: Router,
    port: u16,
    no_tls: bool,
    handle: axum_server::Handle<SocketAddr>,
    entry: Entry,
) -> Result<(), Box<dyn std::error::Error>> {
    let local_ip = best_local_ip().context("Could not find suitable local IP")?;
    let config = tls_config(no_tls, local_ip).await?;
    let display_handle = handle.clone();

    tokio::spawn(async move {
        if let Some(addr) = display_handle.listening().await {
            entry.print(no_tls, SocketAddr::new(local_ip, addr.port()));
        }
    });

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    match config {
        Some(config) => {
            axum_server::bind_rustls(addr, config)
                .handle(handle)
                .serve(app.into_make_service())
                .await?
        }
        None => {
            axum_server::bind(addr)
                .handle(handle)
                .serve(app.into_make_service())
                .await?
        }
    }

    Ok(())
}
