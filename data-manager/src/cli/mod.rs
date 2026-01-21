//! Command-line interface
//!
//! Provides CLI commands for the data manager.

pub mod serve;
pub mod fetch;
pub mod import;
pub mod symbol;
pub mod db;

use clap::{Parser, Subcommand};

/// Data Manager CLI
#[derive(Parser)]
#[command(name = "data-manager")]
#[command(about = "Centralized data infrastructure for market data")]
#[command(version)]
pub struct Cli {
    /// Subcommand to run
    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands
#[derive(Subcommand)]
pub enum Commands {
    /// Start the data manager service
    Serve(serve::ServeArgs),
    /// Fetch historical data on-demand
    Fetch(fetch::FetchArgs),
    /// Import data from third-party files
    Import(import::ImportArgs),
    /// Symbol management commands
    #[command(subcommand)]
    Symbol(symbol::SymbolCommands),
    /// Database operations
    #[command(subcommand)]
    Db(db::DbCommands),
}
