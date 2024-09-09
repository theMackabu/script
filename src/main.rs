mod cli;
mod config;
mod database;
mod globals;
mod helpers;
mod http;
mod modules;
mod routes;
mod structs;

pub mod prelude {
    pub use clap::Parser;
    pub use macros_rs::{
        fmt::{crashln, string},
        fs::file_exists,
    };
}

use crate::prelude::*;
use cli::verbose::{InfoLevel, Verbosity};
use structs::config::Config;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::prelude::*;

#[derive(Clone, Parser)]
pub struct Cli {
    /// Config path
    #[arg(short, long, default_value = "config.toml")]
    pub config: String,
    #[arg(short, long)]
    /// Override config address
    pub address: Option<String>,
    /// Override config port
    #[arg(short, long)]
    pub port: Option<u16>,
    #[clap(flatten)]
    verbose: Verbosity<InfoLevel>,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    let formatting_layer_config = BunyanFormattingLayer::new("server".into(), std::io::stdout)
        .skip_fields(vec!["file", "line"].into_iter())
        .expect("Unable to create logger");

    tracing_subscriber::registry()
        .with(cli.verbose.log_level_filter())
        .with(JsonStorageLayer)
        .with(formatting_layer_config)
        .init();

    if !file_exists!(&cli.config) {
        Config::new().set_path(&format!("{}.tmp", cli.config)).write_example()
    }

    Ok(if let Err(err) = http::start(cli)?.await {
        crashln!("Failed to start server!\n{:?}", err)
    })
}
