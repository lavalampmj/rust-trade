//! Data Manager CLI
//!
//! Provides commands for:
//! - `serve`: Start the data manager service
//! - `fetch`: Fetch historical data on-demand
//! - `import`: Import data from third-party files
//! - `symbol`: Symbol management commands
//! - `db`: Database operations

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use data_manager::cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("data_manager=info".parse()?))
        .init();

    // Load environment variables from .env file
    dotenv::dotenv().ok();

    // Parse CLI arguments
    let cli = Cli::parse();

    // Execute command
    match cli.command {
        Commands::Serve(args) => {
            data_manager::cli::serve::execute(args).await?;
        }
        Commands::Fetch(args) => {
            data_manager::cli::fetch::execute(args).await?;
        }
        Commands::Import(args) => {
            data_manager::cli::import::execute(args).await?;
        }
        Commands::Symbol(cmd) => {
            data_manager::cli::symbol::execute(cmd).await?;
        }
        Commands::Db(cmd) => {
            data_manager::cli::db::execute(cmd).await?;
        }
        Commands::Backfill(cmd) => {
            data_manager::cli::backfill::execute(cmd).await?;
        }
    }

    Ok(())
}
