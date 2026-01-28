//! Fetch command - fetch historical data on-demand
//!
//! Uses the routing system to automatically select the appropriate provider
//! based on asset type and exchange.

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use clap::Args;
use tracing::{error, info, warn};

use crate::config::{AssetType, Settings};
use crate::provider::{infer_asset_type, HistoricalRequest, ProviderFactory};
use crate::storage::MarketDataRepository;
use crate::symbol::SymbolSpec;

/// Arguments for the fetch command
#[derive(Args)]
pub struct FetchArgs {
    /// Symbols to fetch (comma-separated, DBT canonical format e.g., ESH5, BTCUSD)
    #[arg(long, short, value_delimiter = ',')]
    pub symbols: Vec<String>,

    /// Exchange for the symbols (e.g., CME, NASDAQ, KRAKEN)
    #[arg(long, short)]
    pub exchange: String,

    /// Asset type for routing (futures, equity, crypto, fx)
    /// If not specified, will be inferred from symbol/exchange
    #[arg(long, short)]
    pub asset_type: Option<String>,

    /// Override: Data provider to use (databento, kraken)
    /// If not specified, automatically determined by routing rules
    #[arg(long, short)]
    pub provider: Option<String>,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    pub start: String,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    pub end: String,

    /// Provider-specific dataset override
    #[arg(long)]
    pub dataset: Option<String>,

    /// Dry run (don't actually fetch)
    #[arg(long)]
    pub dry_run: bool,

    /// Show routing resolution details
    #[arg(long)]
    pub verbose: bool,
}

/// Execute the fetch command
pub async fn execute(args: FetchArgs) -> Result<()> {
    // Parse dates
    let start_date = NaiveDate::parse_from_str(&args.start, "%Y-%m-%d")?;
    let end_date = NaiveDate::parse_from_str(&args.end, "%Y-%m-%d")?;

    let start: DateTime<Utc> = start_date.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end: DateTime<Utc> = end_date.and_hms_opt(23, 59, 59).unwrap().and_utc();

    // Parse asset type if provided
    let asset_type: Option<AssetType> = if let Some(ref at) = args.asset_type {
        Some(at.parse().map_err(|e: String| anyhow::anyhow!(e))?)
    } else {
        // Infer from first symbol and exchange
        args.symbols
            .first()
            .and_then(|s| infer_asset_type(s, &args.exchange))
    };

    // Create symbol specs
    let symbols: Vec<SymbolSpec> = args
        .symbols
        .iter()
        .map(|s| SymbolSpec::new(s, &args.exchange))
        .collect();

    // Load settings and create factory
    let settings = Settings::load().unwrap_or_else(|_| Settings::default_settings());
    let factory = ProviderFactory::with_routing(settings.routing.clone());

    // Resolve routing for the first symbol (all symbols assumed same asset class)
    let first_symbol = symbols.first().ok_or_else(|| anyhow::anyhow!("No symbols specified"))?;

    // Check if provider is manually overridden
    let route = if let Some(ref provider_override) = args.provider {
        // Create a manual route with the override
        crate::config::ResolvedRoute::new(
            provider_override.clone(),
            args.dataset.clone(),
            first_symbol.symbol.clone(),
            crate::config::RouteResolutionSource::GlobalDefault, // Mark as override
        )
    } else {
        factory.resolve_historical(first_symbol, asset_type)
    };

    // Apply dataset override if specified
    let dataset = args.dataset.clone().or(route.dataset.clone());

    // Display routing info
    info!("=== Fetch Request ===");
    info!("Symbols:      {:?}", args.symbols);
    info!("Exchange:     {}", args.exchange);
    info!("Asset Type:   {}", asset_type.map(|a| a.to_string()).unwrap_or_else(|| "unknown".to_string()));
    info!("Date Range:   {} to {}", args.start, args.end);
    info!("");
    info!("=== Routing Resolution ===");
    info!("Provider:     {}", route.provider);
    if let Some(ref ds) = dataset {
        info!("Dataset:      {}", ds);
    }
    info!("Resolution:   {}", route.resolution_source);
    if args.provider.is_some() {
        info!("              (manually overridden via --provider)");
    }

    if args.verbose {
        info!("");
        info!("Provider Symbol: {}", route.provider_symbol);
    }

    if args.dry_run {
        info!("");
        info!("Dry run - not actually fetching data");
        return Ok(());
    }

    // Validate provider supports historical data
    if route.provider != "databento" {
        error!("");
        error!("Provider '{}' does not support historical data fetching.", route.provider);
        error!("Historical data is currently only available via Databento.");
        error!("");
        error!("Supported asset types for historical fetch:");
        error!("  - Futures (CME, CBOT, NYMEX, COMEX)");
        error!("  - Equities (NASDAQ, NYSE)");
        error!("");
        error!("For crypto historical data, use the backfill command with appropriate settings.");
        return Ok(());
    }

    // Create provider via factory
    info!("");
    info!("Creating {} provider...", route.provider);
    let provider = factory.create_historical_provider(&route).await?;

    // Create request with resolved dataset
    let mut request = HistoricalRequest::trades(symbols, start, end);
    if let Some(ds) = dataset {
        request = request.with_dataset(ds);
    }

    // Connect to database
    info!("Connecting to database...");
    let repository = MarketDataRepository::from_settings(&settings.database).await?;

    // Fetch data
    info!("Fetching data from {}...", route.provider);
    let ticks: Vec<_> = provider
        .fetch_ticks(&request)
        .await?
        .filter_map(|r| r.ok())
        .collect();

    info!("Received {} ticks", ticks.len());

    if ticks.is_empty() {
        warn!("No data received for the specified range");
        return Ok(());
    }

    // Insert into database
    info!("Inserting into database...");
    let inserted = repository.batch_insert_ticks(&ticks).await?;
    info!("Inserted {} ticks", inserted);

    info!("");
    info!("Fetch completed successfully");
    Ok(())
}
