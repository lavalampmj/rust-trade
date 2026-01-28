//! Multi-symbol paper trading processor.
//!
//! Manages one strategy instance per symbol, each with its own:
//! - OHLC generator
//! - BarsContext
//! - Position and cash tracking
//! - State lifecycle management
//!
//! # Example
//!
//! ```ignore
//! use trading_common::backtest::strategy::StrategyInstanceConfig;
//!
//! let configs = vec![
//!     StrategyInstanceConfig::new("rsi", "BTCUSD"),
//!     StrategyInstanceConfig::new("rsi", "ETHUSD"),
//!     StrategyInstanceConfig::new("rsi", "SOLUSD"),
//! ];
//!
//! let processor = MultiSymbolProcessor::new(configs, repository).await?;
//! processor.process_tick(&tick).await?;
//! ```

use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::live_trading::RealtimeOHLCGenerator;
use crate::metrics;
use trading_common::backtest::strategy::{create_strategy, Strategy, StrategyInstanceConfig};
use trading_common::data::repository::TickDataRepository;
use trading_common::data::types::{LiveStrategyLog, TickData, TradeSide};
use trading_common::orders::{Order, OrderSide};
use trading_common::series::bars_context::BarsContext;
use trading_common::state::{ComponentId, StateCoordinator};

/// State for a single strategy-symbol instance.
struct StrategyInstance {
    /// The strategy implementation.
    strategy: Box<dyn Strategy + Send>,

    /// Configuration for this instance.
    config: StrategyInstanceConfig,

    /// OHLC bar generator for this symbol.
    ohlc_generator: RealtimeOHLCGenerator,

    /// BarsContext for strategy access to historical series.
    bars_context: BarsContext,

    /// Current cash balance.
    cash: Decimal,

    /// Current position size.
    position: Decimal,

    /// Average cost basis.
    avg_cost: Decimal,

    /// Total number of trades executed.
    total_trades: u64,

    /// State lifecycle component ID.
    component_id: Option<ComponentId>,

    /// Whether first tick has been received.
    first_tick_received: bool,

    /// Whether warmup transition has occurred.
    warmup_transitioned: bool,
}

impl StrategyInstance {
    /// Create a new strategy instance from configuration.
    fn new(config: StrategyInstanceConfig) -> Result<Self, String> {
        // Create strategy from registry
        let strategy = create_strategy(&config.strategy_id)?;

        // Get bar type from config or strategy preference
        let bar_type = config.bar_spec.bar_type.clone();

        // Create OHLC generator for this symbol
        let ohlc_generator = RealtimeOHLCGenerator::new(config.symbol.clone(), bar_type);

        // Initialize BarsContext with strategy's max lookback
        let max_lookback = strategy.max_bars_lookback();
        let bars_context = BarsContext::with_lookback(&config.symbol, max_lookback);

        Ok(Self {
            strategy,
            config: config.clone(),
            ohlc_generator,
            bars_context,
            cash: config.starting_cash,
            position: Decimal::ZERO,
            avg_cost: Decimal::ZERO,
            total_trades: 0,
            component_id: None,
            first_tick_received: false,
            warmup_transitioned: false,
        })
    }

    /// Get the instance ID.
    #[allow(dead_code)]
    fn instance_id(&self) -> String {
        self.config.instance_id()
    }

    /// Calculate portfolio value at current price.
    fn portfolio_value(&self, current_price: Decimal) -> Decimal {
        self.cash + (self.position * current_price)
    }

    /// Calculate P&L from initial capital.
    fn pnl(&self, current_price: Decimal) -> Decimal {
        self.portfolio_value(current_price) - self.config.starting_cash
    }
}

/// Multi-symbol paper trading processor.
///
/// Manages multiple strategy instances, one per symbol.
pub struct MultiSymbolProcessor {
    /// Strategy instances indexed by symbol.
    instances: HashMap<String, StrategyInstance>,

    /// Shared repository for database operations.
    repository: Arc<TickDataRepository>,

    /// Shared state coordinator for lifecycle management.
    coordinator: Arc<StateCoordinator>,

    /// Total initial capital across all instances.
    total_initial_capital: Decimal,
}

impl MultiSymbolProcessor {
    /// Create a new multi-symbol processor from configurations.
    pub fn new(
        configs: Vec<StrategyInstanceConfig>,
        repository: Arc<TickDataRepository>,
    ) -> Result<Self, String> {
        if configs.is_empty() {
            return Err("No strategy configurations provided".to_string());
        }

        let coordinator = Arc::new(StateCoordinator::default());
        let mut instances = HashMap::new();
        let mut total_initial_capital = Decimal::ZERO;

        println!("📊 Multi-Symbol Paper Trading Processor:");
        println!("   Instances: {}", configs.len());
        println!();

        for config in configs {
            if !config.enabled {
                println!("   ⏸️  {} - DISABLED", config.instance_id());
                continue;
            }

            let symbol = config.symbol.clone();
            let instance_id = config.instance_id();

            // Create instance
            let mut instance = StrategyInstance::new(config.clone())?;

            // Register with state coordinator
            let component_id = coordinator
                .register_strategy(instance.strategy.name(), &symbol)
                .map_err(|e| format!("Failed to register {}: {}", instance_id, e))?;

            // Initialize strategy state
            coordinator
                .initialize_strategy(&component_id, instance.strategy.as_mut())
                .map_err(|e| format!("Failed to initialize {}: {}", instance_id, e))?;

            instance.component_id = Some(component_id);
            total_initial_capital += config.starting_cash;

            println!(
                "   ✓ {} | {} | ${} | {} | warmup: {} bars",
                symbol,
                instance.strategy.name(),
                config.starting_cash,
                config.bar_spec.bar_type.as_str(),
                instance.strategy.warmup_period()
            );

            instances.insert(symbol, instance);
        }

        if instances.is_empty() {
            return Err("No enabled strategy instances".to_string());
        }

        println!();
        println!(
            "   Total Capital: ${} across {} symbols",
            total_initial_capital,
            instances.len()
        );
        println!();

        Ok(Self {
            instances,
            repository,
            coordinator,
            total_initial_capital,
        })
    }

    /// Create from a single strategy ID applied to multiple symbols.
    ///
    /// Convenience method for common case of same strategy on multiple symbols.
    #[allow(dead_code)]
    pub fn from_strategy_and_symbols(
        strategy_id: &str,
        symbols: &[String],
        starting_cash_per_symbol: Decimal,
        repository: Arc<TickDataRepository>,
    ) -> Result<Self, String> {
        let configs: Vec<_> = symbols
            .iter()
            .map(|symbol| {
                StrategyInstanceConfig::new(strategy_id, symbol)
                    .with_starting_cash(starting_cash_per_symbol, "USD")
            })
            .collect();

        Self::new(configs, repository)
    }

    /// Get list of subscribed symbols.
    #[allow(dead_code)]
    pub fn symbols(&self) -> Vec<&str> {
        self.instances.keys().map(|s| s.as_str()).collect()
    }

    /// Get number of active instances.
    #[allow(dead_code)]
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// Process a tick, routing to the correct symbol instance.
    pub async fn process_tick(&mut self, tick: &TickData) -> Result<(), String> {
        // Check if symbol is tracked
        if !self.instances.contains_key(&tick.symbol) {
            debug!("Tick for untracked symbol: {}", tick.symbol);
            return Ok(());
        }

        // Start timer for paper trading tick processing
        let timer = metrics::PAPER_TICK_DURATION.start_timer();
        let start_time = Instant::now();

        // 1. Get data from cache (doesn't need instance borrow)
        let recent_ticks = self
            .repository
            .get_cache()
            .get_recent_ticks(&tick.symbol, 20)
            .await
            .map_err(|e| format!("Cache error: {}", e))?;
        let cache_hit = !recent_ticks.is_empty();

        // Update cache metrics
        if cache_hit {
            metrics::CACHE_HITS_TOTAL.inc();
        } else {
            metrics::CACHE_MISSES_TOTAL.inc();
        }

        // Process the tick and collect data for logging (scoped to release borrow)
        let (portfolio_value, total_pnl, instance_id, starting_cash, position, cash, total_trades, last_action_type) = {
            let instance = self.instances.get_mut(&tick.symbol).unwrap();

            // Handle first tick state transition
            if !instance.first_tick_received {
                instance.first_tick_received = true;
                if let Some(ref id) = instance.component_id {
                    if let Err(e) = self
                        .coordinator
                        .enter_data_processing(id, instance.strategy.as_mut())
                    {
                        warn!("State transition warning for {}: {}", tick.symbol, e);
                    }
                }
            }

            // 2. Generate BarData events from tick
            let bar_events = instance.ohlc_generator.process_tick(tick);

            // 3. Process each bar event through strategy
            let mut last_action_type = "HOLD".to_string();
            for bar_data in bar_events {
                // Update BarsContext BEFORE calling strategy
                instance.bars_context.on_bar_update(&bar_data);

                // Execute strategy with BarData and BarsContext
                instance
                    .strategy
                    .on_bar_data(&bar_data, &mut instance.bars_context);

                // Skip order execution during warmup
                if !instance.strategy.is_ready(&instance.bars_context) {
                    continue;
                }

                // Check for warmup completion transition to Realtime
                if !instance.warmup_transitioned && instance.strategy.is_ready(&instance.bars_context) {
                    instance.warmup_transitioned = true;
                    if let Some(ref id) = instance.component_id {
                        if let Err(e) = self.coordinator.enter_realtime(id, instance.strategy.as_mut())
                        {
                            warn!("State transition warning for {}: {}", tick.symbol, e);
                        } else {
                            info!(
                                "{} ({}) entering REALTIME mode",
                                tick.symbol,
                                instance.strategy.name()
                            );
                        }
                    }
                }

                // Get orders from strategy
                let orders = instance
                    .strategy
                    .get_orders(&bar_data, &mut instance.bars_context);

                // Execute orders
                for order in orders {
                    let action_type = Self::execute_order(instance, &order, tick)?;
                    if action_type != "HOLD" {
                        last_action_type = action_type;
                    }
                }
            }

            // 4. Calculate Portfolio Value for this instance
            let portfolio_value = instance.portfolio_value(tick.price);
            let total_pnl = instance.pnl(tick.price);

            // Collect data needed for logging
            (
                portfolio_value,
                total_pnl,
                instance.config.instance_id(),
                instance.config.starting_cash,
                instance.position,
                instance.cash,
                instance.total_trades,
                last_action_type,
            )
        }; // Mutable borrow ends here

        // Update portfolio metrics (aggregate across all instances)
        let total_portfolio = self.total_portfolio_value(tick.price);
        let total_pnl_all = total_portfolio - self.total_initial_capital;
        metrics::PAPER_PORTFOLIO_VALUE.set(total_portfolio.to_string().parse::<f64>().unwrap_or(0.0));
        metrics::PAPER_PNL.set(total_pnl_all.to_string().parse::<f64>().unwrap_or(0.0));

        // 5. Record to database
        let processing_time = start_time.elapsed().as_micros() as u64;
        let log = LiveStrategyLog {
            timestamp: tick.timestamp,
            strategy_id: instance_id,
            symbol: tick.symbol.clone(),
            current_price: tick.price,
            signal_type: last_action_type.clone(),
            portfolio_value,
            total_pnl,
            cache_hit,
            processing_time_us: processing_time,
        };

        self.repository
            .insert_live_strategy_log(&log)
            .await
            .map_err(|e| format!("Database error: {}", e))?;

        // 6. Real-time output
        Self::log_activity_static(
            &tick.symbol,
            &last_action_type,
            tick,
            portfolio_value,
            total_pnl,
            starting_cash,
            position,
            cash,
            total_trades,
            processing_time,
        );

        // Stop timer
        timer.observe_duration();

        Ok(())
    }

    /// Check timers for all instances (for time-based bar closing).
    pub async fn check_timer(&mut self) -> Result<(), String> {
        let now = Utc::now();

        for (symbol, instance) in self.instances.iter_mut() {
            // Check if bar should close by timer
            let mut bar_events = Vec::new();

            if let Some(closed_bar) = instance.ohlc_generator.check_timer_close(now) {
                debug!("⏰ TIMER_CLOSE: {} bar closed by timer", symbol);
                bar_events.push(closed_bar);
            }

            // Check if synthetic bar needed
            if let Some(synthetic_bar) = instance.ohlc_generator.generate_synthetic_if_needed(now) {
                debug!("🔄 SYNTHETIC: {} generated synthetic bar", symbol);
                bar_events.push(synthetic_bar);
            }

            // Process bar events
            for bar_data in bar_events {
                instance.bars_context.on_bar_update(&bar_data);
                instance
                    .strategy
                    .on_bar_data(&bar_data, &mut instance.bars_context);

                if !instance.strategy.is_ready(&instance.bars_context) {
                    continue;
                }

                // Check for warmup completion
                if !instance.warmup_transitioned && instance.strategy.is_ready(&instance.bars_context)
                {
                    instance.warmup_transitioned = true;
                    if let Some(ref id) = instance.component_id {
                        let _ = self.coordinator.enter_realtime(id, instance.strategy.as_mut());
                        info!(
                            "{} ({}) entering REALTIME mode",
                            symbol,
                            instance.strategy.name()
                        );
                    }
                }

                // Get and execute orders
                let orders = instance
                    .strategy
                    .get_orders(&bar_data, &mut instance.bars_context);

                let execution_price = bar_data.ohlc_bar.close;
                let synthetic_tick = TickData::with_details(
                    now,
                    now,
                    symbol.clone(),
                    "UNKNOWN".to_string(),
                    execution_price,
                    Decimal::ZERO,
                    TradeSide::Buy,
                    "PAPER".to_string(),
                    format!("timer_{}", now.timestamp()),
                    false,
                    0,
                );

                for order in orders {
                    Self::execute_order(instance, &order, &synthetic_tick)?;
                }
            }
        }

        Ok(())
    }

    /// Execute an order for an instance.
    fn execute_order(
        instance: &mut StrategyInstance,
        order: &Order,
        tick: &TickData,
    ) -> Result<String, String> {
        let quantity = order.quantity;

        match order.side {
            OrderSide::Buy => {
                let cost = quantity * tick.price;

                if cost <= instance.cash {
                    if instance.position == Decimal::ZERO {
                        instance.position = quantity;
                        instance.avg_cost = tick.price;
                    } else {
                        let total_cost = (instance.position * instance.avg_cost) + cost;
                        instance.position += quantity;
                        instance.avg_cost = total_cost / instance.position;
                    }

                    instance.cash -= cost;
                    instance.total_trades += 1;
                    metrics::PAPER_TRADES_TOTAL.inc();

                    debug!(
                        "[{}] BUY: {} @ {}, position: {}, cash: {}",
                        tick.symbol, quantity, tick.price, instance.position, instance.cash
                    );
                    return Ok("BUY".to_string());
                } else {
                    debug!(
                        "[{}] BUY ignored: insufficient cash ({} needed, {} available)",
                        tick.symbol, cost, instance.cash
                    );
                }
            }

            OrderSide::Sell => {
                if quantity <= instance.position {
                    let proceeds = quantity * tick.price;
                    instance.cash += proceeds;
                    instance.position -= quantity;
                    instance.total_trades += 1;
                    metrics::PAPER_TRADES_TOTAL.inc();

                    if instance.position == Decimal::ZERO {
                        instance.avg_cost = Decimal::ZERO;
                    }

                    debug!(
                        "[{}] SELL: {} @ {}, position: {}, cash: {}",
                        tick.symbol, quantity, tick.price, instance.position, instance.cash
                    );
                    return Ok("SELL".to_string());
                } else {
                    debug!(
                        "[{}] SELL ignored: insufficient position ({} needed, {} available)",
                        tick.symbol, quantity, instance.position
                    );
                }
            }
        }

        Ok("HOLD".to_string())
    }

    /// Calculate total portfolio value across all instances.
    ///
    /// Note: This requires knowing the current price for each symbol.
    /// For simplicity, uses the last known price from each instance's bars_context.
    fn total_portfolio_value(&self, fallback_price: Decimal) -> Decimal {
        // For accurate total, we'd need current prices for all symbols.
        // For now, just sum cash + position values using last known close prices.
        self.instances
            .values()
            .map(|inst| {
                // Use last bar close as current price estimate
                let price = inst
                    .bars_context
                    .close
                    .get(0)
                    .copied()
                    .unwrap_or(fallback_price);
                inst.portfolio_value(price)
            })
            .sum()
    }

    /// Get summary of all instances.
    #[allow(dead_code)]
    pub fn summary(&self) -> Vec<InstanceSummary> {
        self.instances
            .iter()
            .map(|(symbol, inst)| {
                let last_price = inst
                    .bars_context
                    .close
                    .get(0)
                    .copied()
                    .unwrap_or(Decimal::ZERO);

                InstanceSummary {
                    symbol: symbol.clone(),
                    strategy_id: inst.config.strategy_id.clone(),
                    account_id: inst.config.account_id.clone(),
                    cash: inst.cash,
                    position: inst.position,
                    avg_cost: inst.avg_cost,
                    portfolio_value: inst.portfolio_value(last_price),
                    pnl: inst.pnl(last_price),
                    total_trades: inst.total_trades,
                    is_realtime: inst.warmup_transitioned,
                }
            })
            .collect()
    }

    /// Shutdown all instances.
    pub fn shutdown(&mut self) {
        for (symbol, instance) in self.instances.iter_mut() {
            if let Some(ref id) = instance.component_id {
                let _ = self.coordinator.terminate_strategy(
                    id,
                    instance.strategy.as_mut(),
                    Some("Multi-symbol processor shutdown".to_string()),
                );
                info!("{} ({}) terminated", symbol, instance.strategy.name());
            }
        }
    }

    fn log_activity_static(
        symbol: &str,
        action_type: &str,
        tick: &TickData,
        portfolio_value: Decimal,
        total_pnl: Decimal,
        starting_cash: Decimal,
        position: Decimal,
        cash: Decimal,
        total_trades: u64,
        total_time_us: u64,
    ) {
        if action_type != "HOLD" {
            let return_pct = if starting_cash > Decimal::ZERO {
                total_pnl / starting_cash * Decimal::from(100)
            } else {
                Decimal::ZERO
            };

            println!(
                "🎯 {} {} @ ${} | Portfolio: ${} | P&L: ${} ({:.2}%) | Pos: {} | Cash: ${} | Trades: {} | {}μs",
                action_type,
                symbol,
                tick.price,
                portfolio_value,
                total_pnl,
                return_pct,
                position,
                cash,
                total_trades,
                total_time_us
            );
        }
    }
}

/// Summary of a single instance's state.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct InstanceSummary {
    pub symbol: String,
    pub strategy_id: String,
    pub account_id: String,
    pub cash: Decimal,
    pub position: Decimal,
    pub avg_cost: Decimal,
    pub portfolio_value: Decimal,
    pub pnl: Decimal,
    pub total_trades: u64,
    pub is_realtime: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_strategy_instance_config_creation() {
        let config = StrategyInstanceConfig::new("rsi", "BTCUSD")
            .with_starting_cash(dec!(50000), "USD");

        assert_eq!(config.strategy_id, "rsi");
        assert_eq!(config.symbol, "BTCUSD");
        assert_eq!(config.starting_cash, dec!(50000));
    }

    #[test]
    fn test_instance_id() {
        let config = StrategyInstanceConfig::new("sma", "ETHUSD")
            .with_account("AGGRESSIVE");

        assert_eq!(config.instance_id(), "sma:ETHUSD:AGGRESSIVE");
    }
}
