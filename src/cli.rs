use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Easily share files and messages on a local network.
#[derive(Parser)]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start DropZone server.
    Serve(ServeArgs),

    /// Share a specific file.
    Share(ShareArgs),
}

#[derive(Args)]
pub struct ServeArgs {
    /// Port to listen on.
    #[arg(default_value_t = 8443)]
    pub port: u16,

    /// Do not use TLS (run in HTTP mode).
    #[arg(long)]
    pub no_tls: bool,

    /// Save files in the specified directory.
    #[arg(short, long, default_value = "dropzone-uploads")]
    pub output: PathBuf,

    /// Limit the maximum body size of uploaded files in bytes.
    #[arg(long, env = "DROPZONE_MAX_BODY_SIZE")]
    pub max_body_size: Option<usize>,
}

#[derive(Args)]
pub struct ShareArgs {
    /// Path to a file to share.
    pub path: PathBuf,

    /// Port to listen on.
    #[arg(short, long, default_value_t = 8443)]
    pub port: u16,

    /// Do not use TLS (run in HTTP mode).
    #[arg(long)]
    pub no_tls: bool,
}
