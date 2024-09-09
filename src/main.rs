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
        fmt::{crashln, str, string},
        fs::file_exists,
    };
}

use crate::prelude::*;
use clap::Subcommand;
use cli::verbose::{InfoLevel, Verbosity};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::prelude::*;

#[derive(Parser)]
#[command(version = str!(cli::get_version(false)))]
pub struct Cli {
    /// Config path
    #[arg(short, long, default_value = "config.toml")]
    pub config: String,
    #[arg(short, long)]
    /// Override config address
    pub address: Option<String>,
    #[arg(short, long)]
    /// Override cache directory
    pub cache: Option<String>,
    /// Override config port
    #[arg(short, long)]
    pub port: Option<u16>,
    #[command(subcommand)]
    command: Option<Commands>,
    #[clap(flatten)]
    verbose: Verbosity<InfoLevel>,
}

#[derive(Subcommand)]
enum Cache {
    /// Delete all cached routes
    #[command(visible_alias = "purge")]
    Clean,

    /// View all cached routes
    #[command(visible_alias = "ls")]
    List,

    /// Rebuild the cache
    #[command(visible_alias = "save")]
    Build,

    /// Do a dry-run cache with verbose logging
    #[command(visible_alias = "test")]
    Debug,

    /// View info about a cached route
    #[command(visible_alias = "info")]
    View {
        /// Route name
        route: String,
    },

    /// Remove a cached route
    #[command(visible_alias = "rm", visible_alias = "delete")]
    Remove {
        /// Route name
        route: String,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Cache management
    Cache {
        #[command(subcommand)]
        command: Cache,
    },
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cli = Cli::parse();
    let config = globals::init(&cli);

    let formatting_layer_config = BunyanFormattingLayer::new("server".into(), std::io::stdout)
        .skip_fields(vec!["file", "line"].into_iter())
        .expect("Unable to create logger");

    tracing_subscriber::registry()
        .with(cli.verbose.log_level_filter())
        .with(JsonStorageLayer)
        .with(formatting_layer_config)
        .init();

    Ok(match &cli.command {
        Some(Commands::Cache { command }) => match command {
            Cache::List => cli::cache::list(config).await,
            Cache::Clean => cli::cache::clean(config),
            Cache::Build => cli::cache::build(config).await,
            Cache::Debug => {}
            Cache::View { route } => {}
            Cache::Remove { route } => {}
        },
        None => http::start(config)?.await.unwrap_or_else(|err| {
            crashln!("Failed to start server!\n{:?}", err);
        }),
    })
}
