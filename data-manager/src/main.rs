//! Data Manager CLI
//!
//! Provides commands for:
//! - `serve`: Start the data manager service
//! - `fetch`: Fetch historical data on-demand
//! - `import`: Import data from third-party files
//! - `symbol`: Symbol management commands
//! - `db`: Database operations
//!
//! # Logging Configuration
//!
//! Configure via environment variables:
//! - `RUST_LOG`: Log filter (e.g., "data_manager=debug,sqlx=info")
//! - `LOG_FORMAT`: Output format ("pretty", "compact", "json")
//! - `LOG_TIMESTAMPS`: Timestamp format ("local", "utc", "none")

use anyhow::Result;
use clap::Parser;
use trading_common::logging::{init_logging, LogConfig};

use data_manager::cli::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging with standardized configuration
    let log_config = LogConfig::from_env()
        .with_app_name("data-manager")
        .with_default_level("data_manager=info,sqlx=warn");

    init_logging(log_config).map_err(|e| anyhow::anyhow!("Failed to initialize logging: {}", e))?;

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
