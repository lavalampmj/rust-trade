// src/live_trading/paper_trading.rs
use chrono::Utc;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info};

use crate::live_trading::RealtimeOHLCGenerator;
use crate::metrics;
use trading_common::backtest::strategy::Strategy;
use trading_common::data::repository::TickDataRepository;
use trading_common::data::types::{BarData, LiveStrategyLog, TickData};
use trading_common::orders::{Order, OrderSide};
use trading_common::series::bars_context::BarsContext;
use trading_common::state::{ComponentId, ComponentState, StateCoordinator, StrategyStateTracker};

pub struct PaperTradingProcessor {
    strategy: Box<dyn Strategy + Send>,
    repository: Arc<TickDataRepository>,
    pub(crate) initial_capital: Decimal,

    // OHLC bar generator for unified strategy interface
    ohlc_generator: Option<RealtimeOHLCGenerator>,

    // BarsContext for strategy access to historical series
    bars_context: BarsContext,

    // Simple status tracking
    pub(crate) cash: Decimal,
    pub(crate) position: Decimal,
    pub(crate) avg_cost: Decimal,
    pub(crate) total_trades: u64,

    // State lifecycle management
    coordinator: Option<Arc<StateCoordinator>>,
    strategy_component_id: Option<ComponentId>,
    first_tick_received: bool,
    warmup_transitioned: bool,
}

impl PaperTradingProcessor {
    pub fn new(
        strategy: Box<dyn Strategy + Send>,
        repository: Arc<TickDataRepository>,
        initial_capital: Decimal,
    ) -> Self {
        // Check if strategy uses unified interface (needs OHLC generator)
        let bar_data_mode = strategy.bar_data_mode();
        let bar_type = strategy.preferred_bar_type();
        let max_lookback = strategy.max_bars_lookback();

        // Create OHLC generator if strategy uses OnCloseBar or OnPriceMove modes
        // For OnEachTick, we still need the generator to track OHLC state
        let ohlc_generator = {
            // Extract symbol from strategy - for now use a default, could be configured
            let symbol = "BTCUSDT".to_string(); // TODO: Make configurable
            Some(RealtimeOHLCGenerator::new(symbol.clone(), bar_type))
        };

        // Initialize BarsContext with strategy's max lookback
        let bars_context = BarsContext::with_lookback("BTCUSDT", max_lookback);

        println!("📊 Paper Trading initialized:");
        println!("   Strategy: {}", strategy.name());
        println!("   Bar Type: {}", bar_type.as_str());
        println!(
            "   Mode: {}",
            match bar_data_mode {
                trading_common::data::types::BarDataMode::OnEachTick => "OnEachTick",
                trading_common::data::types::BarDataMode::OnPriceMove => "OnPriceMove",
                trading_common::data::types::BarDataMode::OnCloseBar => "OnCloseBar",
            }
        );
        println!("   Warmup period: {} bars", strategy.warmup_period());

        Self {
            strategy,
            repository,
            initial_capital,
            ohlc_generator,
            bars_context,
            cash: initial_capital,
            position: Decimal::ZERO,
            avg_cost: Decimal::ZERO,
            total_trades: 0,
            coordinator: None,
            strategy_component_id: None,
            first_tick_received: false,
            warmup_transitioned: false,
        }
    }

    /// Enable state lifecycle tracking for this paper trading session.
    ///
    /// When enabled, the processor will:
    /// - Register the strategy with the state coordinator
    /// - Transition through proper lifecycle states during trading
    /// - Notify the strategy via `on_state_change()` at each transition
    /// - Transition to Realtime when warmup completes (unlike backtest)
    ///
    /// # Arguments
    /// - `symbol`: The trading symbol for this session (e.g., "BTCUSDT")
    ///
    /// # Example
    /// ```ignore
    /// let processor = PaperTradingProcessor::new(strategy, repo, capital)
    ///     .with_state_tracking("BTCUSDT")?;
    /// ```
    #[allow(dead_code)]
    pub fn with_state_tracking(mut self, symbol: &str) -> Result<Self, String> {
        let coordinator = Arc::new(StateCoordinator::default());

        // Register strategy
        let component_id = coordinator
            .register_strategy(self.strategy.name(), symbol)
            .map_err(|e| format!("Failed to register strategy: {}", e))?;

        // Perform initial transitions: Undefined → SetDefaults → Configure
        coordinator
            .initialize_strategy(&component_id, self.strategy.as_mut())
            .map_err(|e| format!("Failed to initialize strategy state: {}", e))?;

        info!(
            "Paper trading state tracking enabled for strategy {} ({})",
            self.strategy.name(),
            component_id
        );

        self.coordinator = Some(coordinator);
        self.strategy_component_id = Some(component_id);
        self.first_tick_received = false;
        self.warmup_transitioned = false;

        Ok(self)
    }

    /// Enable state lifecycle tracking with an existing coordinator.
    ///
    /// Use this when you want to share a coordinator across multiple processors
    /// or when you need to track state externally.
    pub fn with_coordinator(
        mut self,
        coordinator: Arc<StateCoordinator>,
        symbol: &str,
    ) -> Result<Self, String> {
        // Register strategy
        let component_id = coordinator
            .register_strategy(self.strategy.name(), symbol)
            .map_err(|e| format!("Failed to register strategy: {}", e))?;

        // Perform initial transitions: Undefined → SetDefaults → Configure
        coordinator
            .initialize_strategy(&component_id, self.strategy.as_mut())
            .map_err(|e| format!("Failed to initialize strategy state: {}", e))?;

        self.coordinator = Some(coordinator);
        self.strategy_component_id = Some(component_id);
        self.first_tick_received = false;
        self.warmup_transitioned = false;

        Ok(self)
    }

    /// Get the state tracker for the strategy (if state tracking is enabled).
    #[allow(dead_code)]
    pub fn state_tracker(&self) -> Option<StrategyStateTracker> {
        match (&self.coordinator, &self.strategy_component_id) {
            (Some(coord), Some(id)) => coord.get_tracker(id),
            _ => None,
        }
    }

    /// Get the current state of the strategy (if state tracking is enabled).
    #[allow(dead_code)]
    pub fn strategy_state(&self) -> Option<ComponentState> {
        match (&self.coordinator, &self.strategy_component_id) {
            (Some(coord), Some(id)) => coord.get_state(id),
            _ => None,
        }
    }

    /// Check if strategy has entered realtime mode.
    #[allow(dead_code)]
    pub fn is_realtime(&self) -> bool {
        self.strategy_state() == Some(ComponentState::Realtime)
    }

    /// Helper to transition state and notify strategy.
    #[allow(dead_code)]
    fn transition_state(&mut self, target: ComponentState, reason: Option<String>) {
        if let (Some(coord), Some(id)) = (&self.coordinator, &self.strategy_component_id) {
            if let Err(e) = coord.transition_and_notify(id, self.strategy.as_mut(), target, reason)
            {
                // Log but don't fail - state tracking is best-effort
                eprintln!("State transition warning: {}", e);
            }
        }
    }

    /// Helper to handle first tick: Configure → DataLoaded → Historical
    fn handle_first_tick(&mut self) {
        if self.first_tick_received {
            return;
        }

        self.first_tick_received = true;

        if let (Some(coord), Some(id)) = (&self.coordinator, &self.strategy_component_id) {
            if let Err(e) = coord.enter_data_processing(id, self.strategy.as_mut()) {
                eprintln!("State transition warning: {}", e);
            }
        }
    }

    /// Helper to handle warmup completion: Historical → Transition → Realtime
    fn check_warmup_transition(&mut self) {
        if self.warmup_transitioned {
            return;
        }

        if self.strategy.is_ready(&self.bars_context) {
            self.warmup_transitioned = true;

            if let (Some(coord), Some(id)) = (&self.coordinator, &self.strategy_component_id) {
                // For live trading, we go all the way to Realtime
                if let Err(e) = coord.enter_realtime(id, self.strategy.as_mut()) {
                    eprintln!("State transition warning: {}", e);
                } else {
                    info!(
                        "Strategy {} entering REALTIME mode",
                        self.strategy.name()
                    );
                }
            }
        }
    }

    /// Terminate the strategy (call during shutdown).
    pub fn shutdown(&mut self) {
        if let (Some(coord), Some(id)) = (&self.coordinator, &self.strategy_component_id) {
            let _ = coord.terminate_strategy(
                id,
                self.strategy.as_mut(),
                Some("Paper trading shutdown".to_string()),
            );
            info!("Strategy {} terminated", self.strategy.name());
        }
    }

    pub async fn process_tick(&mut self, tick: &TickData) -> Result<(), String> {
        // Start timer for paper trading tick processing
        let timer = metrics::PAPER_TICK_DURATION.start_timer();
        let start_time = Instant::now();

        // Handle first tick state transition: Configure → DataLoaded → Historical
        self.handle_first_tick();

        // 1. Get data from cache
        let cache_start = Instant::now();
        let recent_ticks = self
            .repository
            .get_cache()
            .get_recent_ticks(&tick.symbol, 20)
            .await
            .map_err(|e| format!("Cache error: {}", e))?;
        let cache_hit = !recent_ticks.is_empty();
        let cache_time = cache_start.elapsed().as_micros() as u64;

        // Update cache metrics
        if cache_hit {
            metrics::CACHE_HITS_TOTAL.inc();
        } else {
            metrics::CACHE_MISSES_TOTAL.inc();
        }

        // 2. Generate BarData events from tick using OHLC generator
        let bar_events = if let Some(ref mut generator) = self.ohlc_generator {
            generator.process_tick(tick)
        } else {
            // Fallback: create simple BarData from tick
            vec![BarData::from_single_tick(tick)]
        };

        // 3. Process each bar event through strategy
        let mut last_action_type = "HOLD".to_string();
        for bar_data in bar_events {
            // Update BarsContext BEFORE calling strategy
            self.bars_context.on_bar_update(&bar_data);

            // Execute strategy with BarData and BarsContext
            self.strategy.on_bar_data(&bar_data, &mut self.bars_context);

            // Skip order execution during warmup
            if !self.strategy.is_ready(&self.bars_context) {
                continue;
            }

            // Check for warmup completion transition to Realtime
            self.check_warmup_transition();

            // Get orders from strategy
            let orders = self.strategy.get_orders(&bar_data, &mut self.bars_context);

            // Execute orders
            for order in orders {
                let action_type = self.execute_order(&order, tick)?;

                // Track last non-HOLD action for logging
                if action_type != "HOLD" {
                    last_action_type = action_type;
                }
            }
        }

        // 4. Calculate Portfolio Value
        let portfolio_value = self.calculate_portfolio_value(tick.price);
        let total_pnl = portfolio_value - self.initial_capital;

        // Update portfolio metrics
        metrics::PAPER_PORTFOLIO_VALUE
            .set(portfolio_value.to_string().parse::<f64>().unwrap_or(0.0));
        metrics::PAPER_PNL.set(total_pnl.to_string().parse::<f64>().unwrap_or(0.0));

        // 5. Record to database
        let processing_time = start_time.elapsed().as_micros() as u64;
        let log = LiveStrategyLog {
            timestamp: tick.timestamp,
            strategy_id: self.strategy.name().to_string(),
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
        self.log_activity(
            &last_action_type,
            tick,
            portfolio_value,
            total_pnl,
            cache_hit,
            cache_time,
            processing_time,
        );

        // Stop timer for paper trading tick processing
        timer.observe_duration();

        Ok(())
    }

    /// Check timer for bar closing (for time-based bars)
    ///
    /// This should be called periodically (e.g., every 1 second) to check if
    /// a bar should close based on wall-clock time, even if no ticks received.
    pub async fn check_timer(&mut self) -> Result<(), String> {
        let now = Utc::now();

        // Collect bar events to process (to avoid borrow checker issues)
        let mut bar_events = Vec::new();

        if let Some(ref mut generator) = self.ohlc_generator {
            // Check if bar should close by timer
            if let Some(closed_bar) = generator.check_timer_close(now) {
                debug!("⏰ TIMER_CLOSE: Bar closed by timer");
                bar_events.push(closed_bar);
            }

            // Check if synthetic bar needed (no ticks during interval)
            if let Some(synthetic_bar) = generator.generate_synthetic_if_needed(now) {
                debug!("🔄 SYNTHETIC: Generated synthetic bar (no ticks)");
                bar_events.push(synthetic_bar);
            }
        }

        // Process bar events (after releasing the mutable borrow)
        for bar_data in bar_events {
            // Update BarsContext BEFORE calling strategy
            self.bars_context.on_bar_update(&bar_data);

            // Execute strategy with bar and BarsContext
            self.strategy.on_bar_data(&bar_data, &mut self.bars_context);

            // Skip order execution during warmup
            if !self.strategy.is_ready(&self.bars_context) {
                continue;
            }

            // Check for warmup completion transition to Realtime
            self.check_warmup_transition();

            // Get orders from strategy
            let orders = self.strategy.get_orders(&bar_data, &mut self.bars_context);

            // Use bar's close price for execution
            let execution_price = bar_data.ohlc_bar.close;

            // Create a synthetic tick for order execution
            let synthetic_tick = TickData::with_details(
                now,
                now,
                bar_data.ohlc_bar.symbol.clone(),
                "UNKNOWN".to_string(),
                execution_price,
                Decimal::ZERO,
                trading_common::data::types::TradeSide::Buy,
                "PAPER".to_string(),
                if bar_data.metadata.is_synthetic {
                    format!("synthetic_{}", now.timestamp())
                } else {
                    format!("timer_{}", now.timestamp())
                },
                false,
                0,
            );

            // Execute orders
            for order in orders {
                self.execute_order(&order, &synthetic_tick)?;
            }
        }

        Ok(())
    }

    fn execute_order(&mut self, order: &Order, tick: &TickData) -> Result<String, String> {
        let quantity = order.quantity;

        match order.side {
            OrderSide::Buy => {
                let cost = quantity * tick.price;

                if cost <= self.cash {
                    if self.position == Decimal::ZERO {
                        self.position = quantity;
                        self.avg_cost = tick.price;
                    } else {
                        let total_cost = (self.position * self.avg_cost) + cost;
                        self.position += quantity;
                        self.avg_cost = total_cost / self.position;
                    }

                    self.cash -= cost;
                    self.total_trades += 1;
                    metrics::PAPER_TRADES_TOTAL.inc();

                    debug!(
                        "BUY executed: {} @ {}, position: {}, cash: {}",
                        quantity, tick.price, self.position, self.cash
                    );
                    return Ok("BUY".to_string());
                } else {
                    debug!(
                        "BUY order ignored: insufficient cash ({} needed, {} available)",
                        cost, self.cash
                    );
                }
            }

            OrderSide::Sell => {
                if quantity <= self.position {
                    let proceeds = quantity * tick.price;
                    self.cash += proceeds;
                    self.position -= quantity;
                    self.total_trades += 1;
                    metrics::PAPER_TRADES_TOTAL.inc();

                    if self.position == Decimal::ZERO {
                        self.avg_cost = Decimal::ZERO;
                    }

                    debug!(
                        "SELL executed: {} @ {}, position: {}, cash: {}",
                        quantity, tick.price, self.position, self.cash
                    );
                    return Ok("SELL".to_string());
                } else {
                    debug!(
                        "SELL order ignored: insufficient position ({} needed, {} available)",
                        quantity, self.position
                    );
                }
            }
        }

        Ok("HOLD".to_string())
    }

    pub(crate) fn calculate_portfolio_value(&self, current_price: Decimal) -> Decimal {
        self.cash + (self.position * current_price)
    }

    fn log_activity(
        &self,
        action_type: &str,
        tick: &TickData,
        portfolio_value: Decimal,
        total_pnl: Decimal,
        cache_hit: bool,
        cache_time_us: u64,
        total_time_us: u64,
    ) {
        if action_type != "HOLD" {
            let return_pct = if self.initial_capital > Decimal::ZERO {
                total_pnl / self.initial_capital * Decimal::from(100)
            } else {
                Decimal::ZERO
            };

            println!("🎯 {} {} @ ${} | Portfolio: ${} | P&L: ${} ({:.2}%) | Position: {} | Cash: ${} | Trades: {} | Cache: {} ({}μs) | Total: {}μs",
                     action_type,
                     tick.symbol,
                     tick.price,
                     portfolio_value,
                     total_pnl,
                     return_pct,
                     self.position,
                     self.cash,
                     self.total_trades,
                     if cache_hit { "HIT" } else { "MISS" },
                     cache_time_us,
                     total_time_us);
        } else {
            if tick.timestamp.timestamp() % 10 == 0 {
                println!(
                    "📊 {} {} @ ${} | Portfolio: ${} | P&L: ${} | Cache: {} ({}μs)",
                    tick.symbol,
                    if cache_hit { "HIT" } else { "MISS" },
                    tick.price,
                    portfolio_value,
                    total_pnl,
                    if cache_hit { "✓" } else { "✗" },
                    cache_time_us
                );
            }
        }
    }
}
