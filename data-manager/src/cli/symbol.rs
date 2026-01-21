//! Symbol management commands

use anyhow::Result;
use clap::{Args, Subcommand};
use tracing::{error, info};

use crate::config::Settings;
use crate::provider::databento::DatabentoClient;
use crate::provider::DataProvider;
use crate::storage::MarketDataRepository;
use crate::symbol::{SymbolRegistry, SymbolSpec};

/// Symbol subcommands
#[derive(Subcommand)]
pub enum SymbolCommands {
    /// List registered symbols
    List(ListArgs),
    /// Add symbols to registry
    Add(AddArgs),
    /// Remove symbols from registry
    Remove(RemoveArgs),
    /// Discover symbols from provider
    Discover(DiscoverArgs),
}

/// Arguments for list command
#[derive(Args)]
pub struct ListArgs {
    /// Filter by exchange
    #[arg(long, short)]
    pub exchange: Option<String>,

    /// Show detailed information
    #[arg(long, short)]
    pub verbose: bool,
}

/// Arguments for add command
#[derive(Args)]
pub struct AddArgs {
    /// Symbols to add (comma-separated)
    #[arg(long, short, value_delimiter = ',')]
    pub symbols: Vec<String>,

    /// Exchange for the symbols
    #[arg(long, short)]
    pub exchange: String,
}

/// Arguments for remove command
#[derive(Args)]
pub struct RemoveArgs {
    /// Symbols to remove (comma-separated)
    #[arg(long, short, value_delimiter = ',')]
    pub symbols: Vec<String>,

    /// Exchange for the symbols
    #[arg(long, short)]
    pub exchange: String,
}

/// Arguments for discover command
#[derive(Args)]
pub struct DiscoverArgs {
    /// Provider to discover from
    #[arg(long, short, default_value = "databento")]
    pub provider: String,

    /// Exchange to discover (optional)
    #[arg(long, short)]
    pub exchange: Option<String>,

    /// Automatically add discovered symbols
    #[arg(long)]
    pub auto_add: bool,
}

/// Execute symbol commands
pub async fn execute(cmd: SymbolCommands) -> Result<()> {
    match cmd {
        SymbolCommands::List(args) => execute_list(args).await,
        SymbolCommands::Add(args) => execute_add(args).await,
        SymbolCommands::Remove(args) => execute_remove(args).await,
        SymbolCommands::Discover(args) => execute_discover(args).await,
    }
}

async fn execute_list(args: ListArgs) -> Result<()> {
    let settings = Settings::default_settings();
    let repository = MarketDataRepository::from_settings(&settings.database).await?;
    let registry = SymbolRegistry::new(repository.pool().clone());

    let symbols = if let Some(exchange) = args.exchange {
        registry.list_by_exchange(&exchange).await?
    } else {
        registry.list().await?
    };

    if symbols.is_empty() {
        info!("No symbols registered");
        return Ok(());
    }

    info!("Registered symbols ({}):", symbols.len());
    for symbol in symbols {
        if args.verbose {
            info!(
                "  {} ({}) - Status: {:?}, Data: {:?} to {:?}",
                symbol.spec,
                symbol.id,
                symbol.status,
                symbol.data_start,
                symbol.data_end
            );
        } else {
            info!("  {}", symbol.spec);
        }
    }

    Ok(())
}

async fn execute_add(args: AddArgs) -> Result<()> {
    let settings = Settings::default_settings();
    let repository = MarketDataRepository::from_settings(&settings.database).await?;
    let registry = SymbolRegistry::new(repository.pool().clone());

    let symbols: Vec<SymbolSpec> = args
        .symbols
        .iter()
        .map(|s| SymbolSpec::new(s, &args.exchange))
        .collect();

    info!("Adding {} symbols...", symbols.len());
    let count = registry.register_many(&symbols).await?;
    info!("Added {} symbols", count);

    Ok(())
}

async fn execute_remove(args: RemoveArgs) -> Result<()> {
    let settings = Settings::default_settings();
    let repository = MarketDataRepository::from_settings(&settings.database).await?;
    let registry = SymbolRegistry::new(repository.pool().clone());

    let mut removed = 0;
    for symbol in &args.symbols {
        if registry.delete(symbol, &args.exchange).await? {
            removed += 1;
            info!("Removed {}@{}", symbol, args.exchange);
        } else {
            info!("Symbol not found: {}@{}", symbol, args.exchange);
        }
    }

    info!("Removed {} symbols", removed);
    Ok(())
}

async fn execute_discover(args: DiscoverArgs) -> Result<()> {
    if args.provider != "databento" {
        error!("Unknown provider: {}. Only 'databento' is currently supported.", args.provider);
        return Ok(());
    }

    let api_key = std::env::var("DATABENTO_API_KEY")
        .map_err(|_| anyhow::anyhow!("DATABENTO_API_KEY environment variable not set"))?;

    let mut provider = DatabentoClient::from_api_key(api_key);
    provider.connect().await?;

    info!("Discovering symbols from {}...", args.provider);
    let symbols = provider.discover_symbols(args.exchange.as_deref()).await?;

    info!("Discovered {} symbols:", symbols.len());
    for symbol in &symbols {
        info!("  {}", symbol);
    }

    if args.auto_add && !symbols.is_empty() {
        info!("Auto-adding discovered symbols...");
        let settings = Settings::default_settings();
        let repository = MarketDataRepository::from_settings(&settings.database).await?;
        let registry = SymbolRegistry::new(repository.pool().clone());
        let count = registry.register_many(&symbols).await?;
        info!("Added {} symbols to registry", count);
    }

    Ok(())
}
