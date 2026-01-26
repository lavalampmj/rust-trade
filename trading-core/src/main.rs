use rust_decimal::Decimal;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tracing::{debug, error, info, warn};

// State management for lifecycle tracking
use trading_common::state::StateCoordinator;

// CLI-specific modules
mod alerting;
mod config;
mod data_source;
mod exchange;
mod live_trading;
mod metrics;
mod service;

// Import from trading-common
use trading_common::backtest;
use trading_common::data;

use config::Settings;
use data::{cache::TieredCache, repository::TickDataRepository};
use exchange::Exchange;
use live_trading::PaperTradingProcessor;
use service::MarketDataService;

use data::cache::TickDataCache;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("backtest") => run_backtest_mode().await,
        Some("live") => {
            // Check if paper trading is enabled
            if args.contains(&"--paper-trading".to_string()) {
                run_live_with_paper_trading().await
            } else {
                run_live_mode().await
            }
        }
        Some("hash-strategy") => {
            run_hash_strategy_command(&args);
            Ok(())
        }
        None => run_live_mode().await,
        Some("--help") | Some("-h") => {
            print_usage();
            Ok(())
        }
        _ => {
            eprintln!("❌ Unknown command: {}", args[1]);
            print_usage();
            std::process::exit(1);
        }
    }
}

fn print_usage() {
    println!("Trading Core - Cryptocurrency Data Collection & Backtesting System");
    println!();
    println!("Usage:");
    println!("  cargo run                              # Run live data collection");
    println!("  cargo run live                         # Run live data collection");
    println!("  cargo run live --paper-trading         # Run live with paper trading");
    println!("  cargo run backtest                     # Run backtesting mode");
    println!("  cargo run hash-strategy <file_path>    # Calculate SHA256 hash of strategy file");
    println!("  cargo run --help                       # Show this help message");
    println!();
    println!("Note: Live data is received from data-manager via IPC.");
    println!(
        "      Ensure data-manager is running: cd data-manager && cargo run serve --live --ipc"
    );
    println!();
}

fn run_hash_strategy_command(args: &[String]) {
    use std::path::Path;
    use trading_common::backtest::strategy::calculate_file_hash;

    if args.len() < 3 {
        eprintln!("❌ Error: Missing file path argument");
        eprintln!();
        eprintln!("Usage:");
        eprintln!("  cargo run --bin trading-core -- hash-strategy <file_path>");
        eprintln!();
        eprintln!("Example:");
        eprintln!(
            "  cargo run --bin trading-core -- hash-strategy strategies/examples/example_sma.py"
        );
        std::process::exit(1);
    }

    let file_path = &args[2];
    let path = Path::new(file_path);

    if !path.exists() {
        eprintln!("❌ Error: File not found: {}", file_path);
        std::process::exit(1);
    }

    match calculate_file_hash(path) {
        Ok(hash) => {
            println!("✅ SHA256 hash calculated successfully");
            println!();
            println!("File: {}", file_path);
            println!("SHA256: {}", hash);
            println!();
            println!("Add this to your config file:");
            println!();
            println!("[[strategies.python]]");
            println!("id = \"your_strategy_id\"");
            println!("file = \"{}\"", path.file_name().unwrap().to_str().unwrap());
            println!("class_name = \"YourStrategyClassName\"");
            println!("enabled = true");
            println!("sha256 = \"{}\"", hash);
        }
        Err(e) => {
            eprintln!("❌ Error calculating hash: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run_live_with_paper_trading() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize application environment
    init_application().await?;

    info!("🎯 Starting Trading Core Application (Live Mode + Paper Trading)");
    info!("📡 Data source: IPC (from data-manager)");

    // Load configuration
    let settings = Settings::new()?;

    // Check if paper trading is enabled
    if !settings.paper_trading.enabled {
        warn!("⚠️ Paper trading is disabled in config. Set paper_trading.enabled = true");
        warn!("⚠️ Falling back to live data collection only...");
        return run_live_mode().await;
    }

    info!("📋 Configuration loaded successfully");
    info!("📊 Monitoring symbols: {:?}", settings.symbols);
    info!(
        "🎯 Paper Trading Strategy: {}",
        settings.paper_trading.strategy
    );
    info!(
        "💰 Initial Capital: ${}",
        settings.paper_trading.initial_capital
    );
    info!(
        "🗄️  Database: {} connections",
        settings.database.max_connections
    );
    info!(
        "💾 Cache: Memory({} ticks/{}s) + Redis({} ticks/{}s)",
        settings.cache.memory.max_ticks_per_symbol,
        settings.cache.memory.ttl_seconds,
        settings.cache.redis.max_ticks_per_symbol,
        settings.cache.redis.ttl_seconds
    );

    // Verify strategy exists
    if backtest::strategy::get_strategy_info(&settings.paper_trading.strategy).is_none() {
        error!("❌ Unknown strategy: {}", settings.paper_trading.strategy);
        error!("💡 Available strategies: rsi, sma");
        std::process::exit(1);
    }

    // Create database connection pool
    info!("🔌 Connecting to database...");
    let pool = create_database_pool(&settings).await?;
    test_database_connection(&pool).await?;
    info!("✅ Database connection established");

    // Create shared cache
    info!("💾 Initializing cache...");
    let cache = create_cache(&settings).await?;
    let cache: Arc<dyn TickDataCache> = Arc::new(cache);
    info!("✅ Cache initialized");

    // Create repository (needed for paper trading log writes)
    // Repository shares the same cache instance
    let repository = Arc::new(TickDataRepository::new(pool, Arc::clone(&cache)));

    // Create IPC exchange to receive data from data-manager
    info!("📡 Initializing IPC data source from data-manager...");
    let exchange: Arc<dyn Exchange> = Arc::new(data_source::IpcExchange::binance());
    info!("✅ Data source ready");

    // Create strategy
    info!(
        "🧠 Initializing strategy: {}",
        settings.paper_trading.strategy
    );
    let strategy = backtest::strategy::create_strategy(&settings.paper_trading.strategy)?;
    info!("✅ Strategy initialized: {}", strategy.name());

    // Create paper trading processor with state tracking
    let initial_capital = Decimal::try_from(settings.paper_trading.initial_capital)
        .map_err(|e| format!("Invalid initial capital: {}", e))?;

    // Create shared state coordinator for lifecycle management
    let coordinator = Arc::new(StateCoordinator::default());

    // Subscribe to state changes for monitoring/logging
    let mut state_rx = coordinator.subscribe();
    tokio::spawn(async move {
        while let Ok(event) = state_rx.recv().await {
            info!(
                "STATE CHANGE: {} | {:?} → {:?} | reason: {:?}",
                event.component_id, event.old_state, event.new_state, event.reason
            );
        }
        debug!("State change subscriber task ended");
    });

    // Get the first symbol for state tracking
    let primary_symbol = settings
        .symbols
        .first()
        .map(|s| s.as_str())
        .unwrap_or("BTCUSDT");

    // Create processor with state tracking enabled
    let processor = PaperTradingProcessor::new(strategy, Arc::clone(&repository), initial_capital)
        .with_coordinator(Arc::clone(&coordinator), primary_symbol)
        .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

    let paper_trading = Arc::new(tokio::sync::Mutex::new(processor));
    let paper_trading_handle = Arc::clone(&paper_trading); // Keep handle for shutdown

    // Create market data service (cache only - no tick persistence, handled by data-manager)
    // Note: Tick validation happens at data-manager ingestion point, not here
    let service = MarketDataService::new(exchange, cache, settings.symbols.clone())
        .with_paper_trading(paper_trading);

    info!(
        "🎯 Starting market data collection with paper trading for {} symbols",
        settings.symbols.len()
    );
    println!("🚀 Paper trading is now active! Watch for trading signals below...");
    println!(
        "📈 Strategy: {} | Initial Capital: ${}",
        settings.paper_trading.strategy, settings.paper_trading.initial_capital
    );
    println!("{}", "=".repeat(80));

    // Start service with shutdown handling that terminates state lifecycle
    let service_shutdown_tx = service.get_shutdown_tx();

    // Signal forwarding task with state termination
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("Failed to listen for ctrl-c");
        println!("\nReceived Ctrl+C signal, shutting down...");
        info!("Received Ctrl+C signal, initiating shutdown");

        // Terminate strategy state lifecycle
        {
            let mut processor = paper_trading_handle.lock().await;
            processor.shutdown();
        }

        // Forward shutdown to service
        let _ = service_shutdown_tx.send(());
    });

    // Wait for service to complete
    match service.start().await {
        Ok(()) => {
            info!("Service stopped successfully");
        }
        Err(e) => {
            error!("Service stopped with error: {}", e);
            return Err(Box::new(e));
        }
    }

    info!("✅ Application stopped gracefully");
    Ok(())
}

/// Real-time mode entry
async fn run_live_mode() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize environment and logging
    init_application().await?;

    info!("🚀 Starting Trading Core Application (Live Mode)");
    info!("📡 Data source: IPC (from data-manager)");

    // Load configuration
    let settings = Settings::new()?;

    info!("📋 Configuration loaded successfully");
    info!("📊 Monitoring symbols: {:?}", settings.symbols);
    info!(
        "🗄️  Database: {} connections",
        settings.database.max_connections
    );
    info!(
        "💾 Cache: Memory({} ticks/{}s) + Redis({} ticks/{}s)",
        settings.cache.memory.max_ticks_per_symbol,
        settings.cache.memory.ttl_seconds,
        settings.cache.redis.max_ticks_per_symbol,
        settings.cache.redis.ttl_seconds
    );

    // Create and start the application
    run_live_application(settings).await?;

    info!("✅ Application stopped gracefully");
    Ok(())
}

/// Backtesting mode entry
async fn run_backtest_mode() -> Result<(), Box<dyn std::error::Error>> {
    init_application().await?;

    info!("🔬 Starting Trading Core Application (Backtest Mode)");

    let settings = Settings::new()?;
    info!("📋 Configuration loaded successfully");

    let pool = create_database_pool(&settings).await?;
    test_database_connection(&pool).await?;
    info!("✅ Database connection established");

    let cache = create_backtest_cache(&settings).await?;
    let cache: Arc<dyn TickDataCache> = Arc::new(cache);
    info!("✅ Cache initialized for backtest");

    let repository = TickDataRepository::new(pool, cache);

    run_backtest_interactive(repository).await?;

    info!("✅ Backtest completed successfully");
    Ok(())
}

/// Backtesting interactive interface
async fn run_backtest_interactive(
    repository: TickDataRepository,
) -> Result<(), Box<dyn std::error::Error>> {
    use backtest::{
        engine::{BacktestConfig, BacktestData, BacktestEngine},
        strategy::{create_strategy, list_strategies},
    };
    use rust_decimal::Decimal;
    use std::io::{self, Write};
    use std::str::FromStr;

    println!("{}", "=".repeat(60));
    println!("🎯 TRADING CORE BACKTESTING SYSTEM");
    println!("{}", "=".repeat(60));

    // Display statistics
    println!("📊 Loading data statistics...");
    let data_info = repository.get_backtest_data_info().await?;

    println!("\n📈 Available Data:");
    println!("  Total Records: {}", data_info.total_records);
    println!("  Available Symbols: {}", data_info.symbols_count);

    if let Some(earliest) = data_info.earliest_time {
        println!(
            "  Earliest Data: {}",
            earliest.format("%Y-%m-%d %H:%M:%S UTC")
        );
    }
    if let Some(latest) = data_info.latest_time {
        println!("  Latest Data: {}", latest.format("%Y-%m-%d %H:%M:%S UTC"));
    }

    println!("\n📋 Symbol Details:");
    for (i, symbol_info) in data_info.symbol_info.iter().take(10).enumerate() {
        println!(
            "  {}: {} ({} records)",
            i + 1,
            symbol_info.symbol,
            symbol_info.records_count
        );
    }

    if data_info.symbol_info.len() > 10 {
        println!(
            "  ... and {} more symbols",
            data_info.symbol_info.len() - 10
        );
    }

    // Strategy Selection
    println!("\n🎯 Available Strategies:");
    let strategies = list_strategies();
    for (i, strategy) in strategies.iter().enumerate() {
        println!("  {}) {} - {}", i + 1, strategy.name, strategy.description);
    }

    print!("\nSelect strategy (1-{}): ", strategies.len());
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let choice: usize = input.trim().parse().unwrap_or(0);

    if choice == 0 || choice > strategies.len() {
        println!("❌ Invalid selection");
        return Ok(());
    }

    let selected_strategy = &strategies[choice - 1];
    println!("✅ Selected Strategy: {}", selected_strategy.name);

    // Trading pair selection
    println!("\n📊 Symbol Selection:");
    let available_symbols = data_info.get_available_symbols();

    // Display the first 10 symbols for quick selection
    for (i, symbol) in available_symbols.iter().take(10).enumerate() {
        let symbol_info = data_info.get_symbol_info(symbol).unwrap();
        println!(
            "  {}) {} ({} records)",
            i + 1,
            symbol,
            symbol_info.records_count
        );
    }

    print!(
        "\nSelect symbol (1-{}) or enter custom symbol: ",
        available_symbols.len().min(10)
    );
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    let symbol = if let Ok(choice) = input.parse::<usize>() {
        if choice > 0 && choice <= available_symbols.len().min(10) {
            available_symbols[choice - 1].clone()
        } else {
            println!("❌ Invalid selection");
            return Ok(());
        }
    } else if input.is_empty() {
        "BTCUSDT".to_string()
    } else {
        input.to_uppercase()
    };

    // Verify whether the selected transaction pair has data
    if !data_info.has_sufficient_data(&symbol, 100) {
        println!(
            "❌ Insufficient data for symbol: {} (minimum 100 records required)",
            symbol
        );
        return Ok(());
    }

    let symbol_info = data_info.get_symbol_info(&symbol).unwrap();
    println!(
        "✅ Selected Symbol: {} ({} records available)",
        symbol, symbol_info.records_count
    );

    // Data quantity selection
    print!(
        "\nEnter number of records to backtest (default: 10000, max: {}): ",
        symbol_info.records_count
    );
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let data_count: i64 = if input.trim().is_empty() {
        10000.min(symbol_info.records_count as i64)
    } else {
        input
            .trim()
            .parse()
            .unwrap_or(10000)
            .min(symbol_info.records_count as i64)
    };

    // Initial Funding Setup
    print!("\nEnter initial capital (default: $10000): $");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let initial_capital = if input.trim().is_empty() {
        Decimal::from(10000)
    } else {
        Decimal::from_str(input.trim()).unwrap_or(Decimal::from(10000))
    };

    // Commission rate setting
    print!("\nEnter commission rate % (default: 0.1%): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let commission_rate = if input.trim().is_empty() {
        Decimal::from_str("0.001").unwrap() // 0.1%
    } else {
        let rate = input.trim().parse::<f64>().unwrap_or(0.1);
        Decimal::from_str(&format!("{}", rate / 100.0))
            .unwrap_or(Decimal::from_str("0.001").unwrap())
    };

    // Load tick data for backtest
    println!(
        "\n🔍 Loading historical tick data: {} latest {} records...",
        symbol, data_count
    );

    let data = repository
        .get_recent_ticks_for_backtest(&symbol, data_count)
        .await?;

    if data.is_empty() {
        println!("❌ No historical data found for symbol: {}", symbol);
        return Ok(());
    }

    println!("✅ Loaded {} tick data points", data.len());
    println!(
        "📅 Data range: {} to {}",
        data.first().unwrap().timestamp.format("%Y-%m-%d %H:%M:%S"),
        data.last().unwrap().timestamp.format("%Y-%m-%d %H:%M:%S")
    );

    let config = BacktestConfig::new(initial_capital).with_commission_rate(commission_rate);

    let strategy = create_strategy(&selected_strategy.id)?;

    // Create shared state coordinator for lifecycle monitoring
    let coordinator = Arc::new(trading_common::state::StateCoordinator::default());

    // Subscribe to state changes for backtest monitoring
    let mut state_rx = coordinator.subscribe();
    let state_monitor = tokio::spawn(async move {
        while let Ok(event) = state_rx.recv().await {
            println!(
                "📊 STATE: {} | {:?} → {:?}{}",
                event.component_id,
                event.old_state,
                event.new_state,
                event.reason.map(|r| format!(" | {}", r)).unwrap_or_default()
            );
        }
    });

    println!("\n{}", "=".repeat(60));
    let mut engine = BacktestEngine::new(strategy, config)?
        .with_coordinator(Arc::clone(&coordinator), &symbol)
        .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
    let result = engine.run_unified(BacktestData::Ticks(data));

    // Stop state monitor task
    state_monitor.abort();

    // Show results
    println!("\n");
    result.print_summary();

    // Ask whether to display detailed transaction analysis
    print!("\nShow detailed trade analysis? (y/N): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if input.trim().to_lowercase() == "y" || input.trim().to_lowercase() == "yes" {
        result.print_trade_analysis();
    }

    println!("\n🎉 Backtest completed successfully!");

    Ok(())
}

/// Initialize application environment and logging
async fn init_application() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file
    dotenv::dotenv().ok();

    // Initialize tracing/logging
    init_tracing()?;

    // Initialize Prometheus metrics
    metrics::register_metrics()?;
    metrics::start_uptime_monitor();
    info!("✓ Prometheus metrics initialized");

    // Start metrics HTTP server on port 9090
    tokio::spawn(async {
        if let Err(e) = metrics::start_metrics_server(9090).await {
            error!("Metrics server error: {}", e);
        }
    });
    info!("📊 Metrics server started on http://0.0.0.0:9090/metrics");

    // Initialize alerting system
    init_alerting_system().await?;

    // Initialize Python strategy system (optional - graceful degradation)
    let config_path = "../config/development.toml";
    match trading_common::backtest::strategy::initialize_python_strategies(config_path) {
        Ok(_) => info!("✓ Python strategy system initialized"),
        Err(e) => {
            warn!("⚠ Python strategies unavailable: {}", e);
            warn!("  Rust strategies will still work normally");
        }
    }

    info!("🔧 Application environment initialized");
    Ok(())
}

/// Initialize alerting system with configured rules
async fn init_alerting_system() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let settings = Settings::new()?;

    if !settings.alerting.enabled {
        info!("⚠️ Alerting system disabled in configuration");
        return Ok(());
    }

    use alerting::{AlertEvaluator, AlertRule, LogAlertHandler};
    use std::time::Duration;

    let handler = LogAlertHandler::new();
    let mut evaluator = AlertEvaluator::new(handler);

    // Configure cooldown
    evaluator.set_cooldown(Duration::from_secs(settings.alerting.cooldown_secs));

    // Add alert rules based on configuration
    evaluator.add_rule(AlertRule::ipc_disconnected());

    evaluator.add_rule(AlertRule::ipc_reconnection_storm(
        settings.alerting.reconnection_storm_threshold,
    ));

    evaluator.add_rule(AlertRule::channel_backpressure(
        settings.alerting.channel_backpressure_threshold,
    ));

    evaluator.add_rule(AlertRule::cache_failure_rate(
        settings.alerting.cache_failure_threshold.unwrap_or(0.1),
    ));

    // Start background monitoring
    evaluator.start_monitoring(settings.alerting.interval_secs);

    info!(
        "🚨 Alerting system started (interval: {}s, cooldown: {}s)",
        settings.alerting.interval_secs, settings.alerting.cooldown_secs
    );
    info!(
        "  📋 Rules: IPC disconnected, channel backpressure ({:.0}%), reconnection storm (>{} attempts)",
        settings.alerting.channel_backpressure_threshold,
        settings.alerting.reconnection_storm_threshold
    );

    Ok(())
}

/// Initialize tracing subscriber for logging
///
/// Uses standardized logging from trading_common::logging.
/// Configure via environment variables:
/// - RUST_LOG: Log filter (e.g., "trading_core=debug,sqlx=info")
/// - LOG_FORMAT: Output format ("pretty", "compact", "json")
/// - LOG_TIMESTAMPS: Timestamp format ("local", "utc", "none")
fn init_tracing() -> Result<(), Box<dyn std::error::Error>> {
    use trading_common::logging::{init_logging, LogConfig};

    // Use standardized logging configuration from environment
    let config = LogConfig::from_env()
        .with_app_name("trading-core")
        .with_default_level("trading_core=info,sqlx=info,tokio=info,hyper=info");

    init_logging(config).map_err(|e| -> Box<dyn std::error::Error> { e })?;

    Ok(())
}

/// Main application runtime (original live mode)
async fn run_live_application(settings: Settings) -> Result<(), Box<dyn std::error::Error>> {
    // Validate basic configuration
    if settings.symbols.is_empty() {
        error!("❌ No symbols configured for monitoring");
        std::process::exit(1);
    }

    if settings.database.max_connections == 0 {
        error!("❌ Database max_connections must be greater than 0");
        std::process::exit(1);
    }

    // Create database connection pool
    info!("🔌 Connecting to database...");
    let pool = create_database_pool(&settings).await?;

    // Test database connectivity
    test_database_connection(&pool).await?;
    info!("✅ Database connection established");

    // Create cache
    info!("💾 Initializing cache...");
    let cache = create_cache(&settings).await?;
    let cache: Arc<dyn TickDataCache> = Arc::new(cache);
    info!("✅ Cache initialized");

    // Create IPC exchange to receive data from data-manager
    info!("📡 Initializing IPC data source from data-manager...");
    let exchange: Arc<dyn Exchange> = Arc::new(data_source::IpcExchange::binance());
    info!("✅ Data source ready");

    // Create market data service (cache only - no tick persistence, handled by data-manager)
    // Note: Tick validation happens at data-manager ingestion point, not here
    let service = MarketDataService::new(exchange, cache, settings.symbols.clone());

    info!(
        "🎯 Starting market data collection for {} symbols",
        settings.symbols.len()
    );

    // Setup signal forwarding to service
    let service_shutdown_tx = service.get_shutdown_tx();
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("Failed to listen for ctrl-c");
        println!("\nReceived Ctrl+C signal, forwarding to service...");
        info!("Received Ctrl+C signal, forwarding to service");
        let _ = service_shutdown_tx.send(());
    });

    // Start service and wait for completion
    match service.start().await {
        Ok(()) => {
            info!("✅ Service stopped successfully");
            Ok(())
        }
        Err(e) => {
            error!("❌ Service stopped with error: {}", e);
            Err(Box::new(e))
        }
    }
}

/// Create database connection pool
async fn create_database_pool(settings: &Settings) -> Result<PgPool, Box<dyn std::error::Error>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(settings.database.max_connections)
        .min_connections(settings.database.min_connections)
        .max_lifetime(Duration::from_secs(settings.database.max_lifetime))
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(600))
        .connect(&settings.database.url)
        .await?;

    Ok(pool)
}

/// Test database connection
async fn test_database_connection(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    // Simple connectivity test
    sqlx::query("SELECT 1").execute(pool).await?;

    // Note: trading-core no longer writes to database - data-manager handles persistence
    // We only verify basic connectivity here
    info!("✅ Database connection verified");
    Ok(())
}

/// Create cache instance (original live mode)
async fn create_cache(settings: &Settings) -> Result<TieredCache, Box<dyn std::error::Error>> {
    let memory_config = (
        settings.cache.memory.max_ticks_per_symbol,
        settings.cache.memory.ttl_seconds,
    );

    let redis_config = (
        settings.cache.redis.url.as_str(),
        settings.cache.redis.max_ticks_per_symbol,
        settings.cache.redis.ttl_seconds,
    );

    let cache = TieredCache::new(memory_config, redis_config).await?;

    // Test cache connectivity
    test_cache_connection(&cache).await?;

    Ok(cache)
}

/// Create simplified cache for backtest mode
async fn create_backtest_cache(
    settings: &Settings,
) -> Result<TieredCache, Box<dyn std::error::Error>> {
    // Creating a minimal cache configuration for backtesting
    let memory_config = (10, 60);
    let redis_config = (settings.cache.redis.url.as_str(), 10, 60);

    let cache = TieredCache::new(memory_config, redis_config).await?;

    // Simple connection test (not required to be completely normal, because backtesting mainly uses the database)
    if let Err(e) = test_cache_connection(&cache).await {
        warn!("⚠️ Cache test failed (this is OK for backtest mode): {}", e);
    }

    Ok(cache)
}

/// Test cache connection
async fn test_cache_connection(cache: &TieredCache) -> Result<(), Box<dyn std::error::Error>> {
    // Test cache by getting symbols (should return empty list initially)
    cache.get_symbols().await?;
    info!("✅ Cache connectivity test passed");
    Ok(())
}
