mod cli;
mod local_ip;
mod qr;
mod serve;
mod server;
mod share;
mod tls;
mod upload;

use clap::Parser;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Commands::Serve(args) => serve::run(args).await,
        cli::Commands::Share(args) => share::run(args).await,
    }
}
