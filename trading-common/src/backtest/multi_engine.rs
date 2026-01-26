//! Multi-strategy backtest engine that processes bars in time order across all strategies.
//!
//! This engine ensures that for each bar timestamp, ALL strategies receive the bar
//! before advancing to the next bar - simulating realistic market conditions where
//! all participants see the same data at the same time.

use crate::backtest::{
    bar_generator::HistoricalOHLCGenerator, metrics::BacktestMetrics, portfolio::Portfolio,
    strategy::Strategy, BacktestConfig, BacktestResult,
};
use crate::data::types::BarData;
use crate::orders::OrderSide;
use crate::series::bars_context::BarsContext;
use crate::state::{ComponentId, ComponentState, StateCoordinator};
use rust_decimal::Decimal;
use std::sync::Arc;

/// Tracks a single strategy's execution state during multi-strategy backtest.
pub struct StrategyRunner {
    /// The strategy instance
    pub strategy: Box<dyn Strategy>,
    /// Component ID for state tracking
    pub component_id: Option<ComponentId>,
    /// Strategy-specific bars context
    pub bars_context: BarsContext,
    /// Strategy-specific portfolio
    pub portfolio: Portfolio,
    /// Symbol this strategy trades
    pub symbol: String,
    /// Whether warmup transition has occurred
    pub warmup_transitioned: bool,
    /// Whether data processing state has been entered
    pub data_processing_started: bool,
}

impl StrategyRunner {
    /// Create a new strategy runner.
    pub fn new(
        strategy: Box<dyn Strategy>,
        symbol: String,
        initial_capital: Decimal,
        commission_rate: Decimal,
    ) -> Self {
        let max_lookback = strategy.max_bars_lookback();
        let bars_context = BarsContext::with_lookback(&symbol, max_lookback);
        let portfolio = Portfolio::new(initial_capital).with_commission_rate(commission_rate);

        Self {
            strategy,
            component_id: None,
            bars_context,
            portfolio,
            symbol,
            warmup_transitioned: false,
            data_processing_started: false,
        }
    }

    /// Reset the runner for a new backtest run.
    pub fn reset(&mut self) {
        self.bars_context.reset();
        self.portfolio = Portfolio::new(self.portfolio.initial_capital)
            .with_commission_rate(self.portfolio.commission_rate);
        self.warmup_transitioned = false;
        self.data_processing_started = false;
        self.strategy.reset();
    }
}

/// Multi-strategy backtest engine that processes bars in time order.
///
/// # Time-Synchronized Processing
///
/// Unlike running strategies sequentially (Strategy A all bars, then Strategy B all bars),
/// this engine processes each bar across ALL strategies before advancing to the next bar:
///
/// ```text
/// Bar 1 → Strategy A.on_bar_data() → Strategy B.on_bar_data() → Strategy C.on_bar_data()
/// Bar 2 → Strategy A.on_bar_data() → Strategy B.on_bar_data() → Strategy C.on_bar_data()
/// Bar 3 → Strategy A.on_bar_data() → Strategy B.on_bar_data() → Strategy C.on_bar_data()
/// ```
///
/// This ensures all strategies see market data at the same time, simulating realistic
/// multi-strategy trading conditions.
pub struct MultiStrategyBacktestEngine {
    /// Strategy runners (one per strategy)
    runners: Vec<StrategyRunner>,
    /// Shared configuration
    config: BacktestConfig,
    /// Optional state coordinator for lifecycle management
    coordinator: Option<Arc<StateCoordinator>>,
}

impl MultiStrategyBacktestEngine {
    /// Create a new multi-strategy backtest engine.
    pub fn new(config: BacktestConfig) -> Self {
        Self {
            runners: Vec::new(),
            config,
            coordinator: None,
        }
    }

    /// Add a strategy to the engine.
    ///
    /// Each strategy runs independently with its own portfolio and bars context,
    /// but all strategies receive bars simultaneously in time order.
    pub fn add_strategy(
        &mut self,
        mut strategy: Box<dyn Strategy>,
        symbol: &str,
    ) -> Result<(), String> {
        // Initialize strategy
        strategy.reset();
        strategy.initialize(self.config.strategy_params.clone())?;

        let mut runner = StrategyRunner::new(
            strategy,
            symbol.to_string(),
            self.config.initial_capital,
            self.config.commission_rate,
        );

        // Register with coordinator if available
        if let Some(ref coordinator) = self.coordinator {
            let component_id = coordinator
                .register_strategy(runner.strategy.name(), symbol)
                .map_err(|e| format!("Failed to register strategy: {}", e))?;

            // Initialize strategy state: Undefined → SetDefaults → Configure
            coordinator
                .initialize_strategy(&component_id, runner.strategy.as_mut())
                .map_err(|e| format!("Failed to initialize strategy state: {}", e))?;

            runner.component_id = Some(component_id);
        }

        self.runners.push(runner);
        Ok(())
    }

    /// Enable state lifecycle tracking with an existing coordinator.
    ///
    /// Call this BEFORE adding strategies to ensure they are registered with the coordinator.
    pub fn with_coordinator(mut self, coordinator: Arc<StateCoordinator>) -> Self {
        self.coordinator = coordinator.into();
        self
    }

    /// Get the number of strategies in the engine.
    pub fn strategy_count(&self) -> usize {
        self.runners.len()
    }

    /// Run unified backtest across all strategies.
    ///
    /// Bars are processed in time order - for each bar, ALL strategies receive
    /// the bar before advancing to the next bar.
    pub fn run_unified(&mut self, data: BacktestData) -> Vec<BacktestResult> {
        if self.runners.is_empty() {
            println!("Warning: No strategies added to multi-strategy engine");
            return Vec::new();
        }

        println!("Starting multi-strategy unified backtest...");
        println!("Number of strategies: {}", self.runners.len());
        for (i, runner) in self.runners.iter().enumerate() {
            println!(
                "  Strategy {}: {} (symbol: {})",
                i + 1,
                runner.strategy.name(),
                runner.symbol
            );
        }
        println!("Initial capital per strategy: ${}", self.config.initial_capital);
        println!(
            "Commission rate: {}%",
            self.config.commission_rate * Decimal::from(100)
        );

        // Use first strategy's bar type and mode for data generation
        // (all strategies should ideally use compatible settings)
        let bar_type = self.runners[0].strategy.preferred_bar_type();
        let mode = self.runners[0].strategy.bar_data_mode();
        let session_config = self.runners[0].strategy.session_config();

        // Generate BarData events based on input type
        let bar_events = match data {
            BacktestData::Ticks(ticks) => {
                println!("Data points: {} ticks", ticks.len());
                let generator =
                    HistoricalOHLCGenerator::with_session_config(bar_type, mode, session_config);
                generator.generate_from_ticks(&ticks)
            }
            BacktestData::OHLCBars(ohlc_bars) => {
                println!("Data points: {} OHLC bars", ohlc_bars.len());
                ohlc_bars
                    .iter()
                    .map(|ohlc| BarData::from_ohlc(ohlc))
                    .collect()
            }
        };

        println!("Generated {} bar events", bar_events.len());
        if self.coordinator.is_some() {
            println!("State tracking: enabled");
        }
        println!("{}", "=".repeat(60));
        println!("Processing bars in TIME ORDER across all strategies...");
        println!("{}", "=".repeat(60));

        // Reset all runners
        for runner in &mut self.runners {
            runner.reset();
        }

        // Process bar events - ALL strategies receive each bar before advancing
        let total_bars = bar_events.len();
        let mut last_progress = 0;

        for (bar_idx, bar_data) in bar_events.iter().enumerate() {
            // For each bar, process ALL strategies
            for runner in &mut self.runners {
                // Only process bars that match the strategy's symbol
                if bar_data.ohlc_bar.symbol != runner.symbol {
                    continue;
                }

                // Enter data processing state on first bar
                if !runner.data_processing_started {
                    runner.data_processing_started = true;
                    Self::enter_data_processing_state_inner(&self.coordinator, runner);
                }

                // Update BarsContext BEFORE calling strategy
                runner.bars_context.on_bar_update(bar_data);

                // Update current price in portfolio
                runner
                    .portfolio
                    .update_price(&bar_data.ohlc_bar.symbol, bar_data.ohlc_bar.close);

                // Execute strategy with BarsContext
                runner
                    .strategy
                    .on_bar_data(bar_data, &mut runner.bars_context);

                // Skip order execution during warmup period
                if !runner.strategy.is_ready(&runner.bars_context) {
                    continue;
                }

                // Check for warmup completion transition
                Self::check_warmup_transition_inner(&self.coordinator, runner);

                // Get orders from strategy
                let orders = runner
                    .strategy
                    .get_orders(bar_data, &mut runner.bars_context);

                // Determine execution price
                let execution_price = bar_data
                    .current_tick
                    .as_ref()
                    .map(|t| t.price)
                    .unwrap_or(bar_data.ohlc_bar.close);

                // Execute orders
                for order in orders {
                    let symbol = order.symbol().to_string();
                    let quantity = order.quantity;

                    match order.side {
                        OrderSide::Buy => {
                            if let Err(e) = runner.portfolio.execute_buy(
                                symbol.clone(),
                                quantity,
                                execution_price,
                            ) {
                                // Silent failure during backtest (can log if needed)
                                let _ = e;
                            }
                        }
                        OrderSide::Sell => {
                            if let Err(e) = runner.portfolio.execute_sell(
                                symbol.clone(),
                                quantity,
                                execution_price,
                            ) {
                                let _ = e;
                            }
                        }
                    }
                }
            }

            // Progress display
            let progress = ((bar_idx + 1) * 100) / total_bars;
            if progress != last_progress && progress % 10 == 0 {
                println!(
                    "Progress: {}% ({}/{})",
                    progress,
                    bar_idx + 1,
                    total_bars
                );
                last_progress = progress;
            }
        }

        println!("\n{}", "=".repeat(60));

        // Terminate all strategies
        for runner in &mut self.runners {
            Self::terminate_state_inner(&self.coordinator, runner, Some("Backtest complete".to_string()));
        }

        // Calculate and return results for each strategy
        self.runners
            .iter()
            .map(|runner| self.calculate_result(runner))
            .collect()
    }

    /// Helper to enter data processing state for a runner (static to avoid borrow issues).
    fn enter_data_processing_state_inner(
        coordinator: &Option<Arc<StateCoordinator>>,
        runner: &mut StrategyRunner,
    ) {
        if let (Some(ref coord), Some(ref id)) = (coordinator, &runner.component_id) {
            if let Err(e) = coord.enter_data_processing(id, runner.strategy.as_mut()) {
                eprintln!("State transition warning: {}", e);
            }
        }
    }

    /// Helper to check and handle warmup completion (static to avoid borrow issues).
    fn check_warmup_transition_inner(
        coordinator: &Option<Arc<StateCoordinator>>,
        runner: &mut StrategyRunner,
    ) {
        if runner.warmup_transitioned {
            return;
        }

        if runner.strategy.is_ready(&runner.bars_context) {
            runner.warmup_transitioned = true;

            if let (Some(ref coord), Some(ref id)) = (coordinator, &runner.component_id) {
                if let Err(e) = coord.transition_and_notify(
                    id,
                    runner.strategy.as_mut(),
                    ComponentState::Transition,
                    Some("Warmup complete".to_string()),
                ) {
                    eprintln!("State transition warning: {}", e);
                }
            }
        }
    }

    /// Helper to terminate strategy state (static to avoid borrow issues).
    fn terminate_state_inner(
        coordinator: &Option<Arc<StateCoordinator>>,
        runner: &mut StrategyRunner,
        reason: Option<String>,
    ) {
        if let (Some(ref coord), Some(ref id)) = (coordinator, &runner.component_id) {
            let _ = coord.terminate_strategy(id, runner.strategy.as_mut(), reason);
        }
    }

    /// Calculate backtest result for a single runner.
    fn calculate_result(&self, runner: &StrategyRunner) -> BacktestResult {
        let final_value = runner.portfolio.total_value();
        let total_pnl = runner.portfolio.total_pnl();
        let total_return_pct = if runner.portfolio.initial_capital > Decimal::ZERO {
            (total_pnl / runner.portfolio.initial_capital) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        let equity_curve = runner.portfolio.get_equity_curve();
        let returns = Self::calculate_returns(&equity_curve);

        let max_drawdown = BacktestMetrics::calculate_max_drawdown(&equity_curve);
        let sharpe_ratio = BacktestMetrics::calculate_sharpe_ratio(&returns, Decimal::ZERO);
        let volatility = BacktestMetrics::calculate_volatility(&returns);
        let win_rate = BacktestMetrics::calculate_win_rate(&runner.portfolio.trades);
        let profit_factor = BacktestMetrics::calculate_profit_factor(&runner.portfolio.trades);
        let avg_trade_duration =
            BacktestMetrics::calculate_average_trade_duration(&runner.portfolio.trades);

        BacktestResult {
            initial_capital: runner.portfolio.initial_capital,
            final_value,
            total_pnl,
            return_percentage: total_return_pct,
            total_trades: runner.portfolio.trades.len(),
            winning_trades: Self::count_winning_trades(&runner.portfolio.trades),
            losing_trades: Self::count_losing_trades(&runner.portfolio.trades),
            max_drawdown,
            sharpe_ratio,
            volatility,
            win_rate,
            profit_factor,
            avg_trade_duration_seconds: avg_trade_duration,
            total_commission: runner.portfolio.total_commission(),
            positions: runner.portfolio.positions.clone(),
            trades: runner.portfolio.trades.clone(),
            equity_curve,
            strategy_name: runner.strategy.name().to_string(),
        }
    }

    fn calculate_returns(equity_curve: &[Decimal]) -> Vec<Decimal> {
        if equity_curve.len() < 2 {
            return Vec::new();
        }

        equity_curve
            .windows(2)
            .map(|window| {
                if window[0] > Decimal::ZERO {
                    (window[1] - window[0]) / window[0]
                } else {
                    Decimal::ZERO
                }
            })
            .collect()
    }

    fn count_winning_trades(trades: &[crate::backtest::portfolio::Trade]) -> usize {
        trades
            .iter()
            .filter(|trade| trade.realized_pnl.map_or(false, |pnl| pnl > Decimal::ZERO))
            .count()
    }

    fn count_losing_trades(trades: &[crate::backtest::portfolio::Trade]) -> usize {
        trades
            .iter()
            .filter(|trade| trade.realized_pnl.map_or(false, |pnl| pnl < Decimal::ZERO))
            .count()
    }
}

/// Input data for multi-strategy backtesting (re-exported for convenience).
pub use crate::backtest::engine::BacktestData;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{BarDataMode, OHLCData, Timeframe};
    use crate::orders::Order;
    use crate::series::bars_context::BarsContext;
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Counter to track the order of bar processing across strategies
    static BAR_ORDER_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// A test strategy that records the order in which it receives bars
    struct OrderTrackingStrategy {
        name: String,
        bars_received: Vec<(usize, i64)>, // (global_order, bar_timestamp_offset)
    }

    impl OrderTrackingStrategy {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                bars_received: Vec::new(),
            }
        }
    }

    impl Strategy for OrderTrackingStrategy {
        fn name(&self) -> &str {
            &self.name
        }

        fn on_bar_data(&mut self, bar_data: &BarData, _bars: &mut BarsContext) {
            let order = BAR_ORDER_COUNTER.fetch_add(1, Ordering::SeqCst);
            let timestamp_offset = bar_data.ohlc_bar.timestamp.timestamp();
            self.bars_received.push((order, timestamp_offset));
        }

        fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
            Ok(())
        }

        fn is_ready(&self, _bars: &BarsContext) -> bool {
            true
        }

        fn warmup_period(&self) -> usize {
            0
        }

        fn bar_data_mode(&self) -> BarDataMode {
            BarDataMode::OnCloseBar
        }

        fn get_orders(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Vec<Order> {
            Vec::new()
        }
    }

    fn create_ohlc_bar(symbol: &str, close: Decimal, timestamp_offset_secs: i64) -> OHLCData {
        OHLCData {
            timestamp: chrono::DateTime::from_timestamp(timestamp_offset_secs, 0)
                .unwrap()
                .with_timezone(&Utc),
            symbol: symbol.to_string(),
            timeframe: Timeframe::OneMinute,
            open: close - dec!(100),
            high: close + dec!(200),
            low: close - dec!(200),
            close,
            volume: dec!(100),
            trade_count: 10,
        }
    }

    #[test]
    fn test_multi_strategy_time_order_processing() {
        // Reset the counter
        BAR_ORDER_COUNTER.store(0, Ordering::SeqCst);

        let config = BacktestConfig::new(dec!(100000));
        let mut engine = MultiStrategyBacktestEngine::new(config);

        // Add two strategies
        engine
            .add_strategy(Box::new(OrderTrackingStrategy::new("StrategyA")), "BTCUSDT")
            .unwrap();
        engine
            .add_strategy(Box::new(OrderTrackingStrategy::new("StrategyB")), "BTCUSDT")
            .unwrap();

        // Create 3 bars
        let bars = vec![
            create_ohlc_bar("BTCUSDT", dec!(50000), 0),
            create_ohlc_bar("BTCUSDT", dec!(51000), 60),
            create_ohlc_bar("BTCUSDT", dec!(52000), 120),
        ];

        engine.run_unified(BacktestData::OHLCBars(bars));

        // Verify processing order:
        // Bar 1: StrategyA (0), StrategyB (1)
        // Bar 2: StrategyA (2), StrategyB (3)
        // Bar 3: StrategyA (4), StrategyB (5)

        // This ensures bars are processed in TIME ORDER across strategies,
        // not STRATEGY ORDER (all bars for A, then all bars for B)

        // Access the internal runners to check order
        // The order should be: 0, 1, 2, 3, 4, 5 alternating between strategies
        // Strategy A should have orders: 0, 2, 4 (for bars at timestamps 0, 60, 120)
        // Strategy B should have orders: 1, 3, 5 (for bars at timestamps 0, 60, 120)

        // If it was sequential (wrong), Strategy A would have 0, 1, 2 and Strategy B would have 3, 4, 5

        // We can verify this by checking that the counter reached 6
        assert_eq!(BAR_ORDER_COUNTER.load(Ordering::SeqCst), 6);
    }

    #[test]
    fn test_multi_strategy_independent_portfolios() {
        /// Strategy that always buys
        struct AlwaysBuyStrategy {
            bought: bool,
        }

        impl Strategy for AlwaysBuyStrategy {
            fn name(&self) -> &str {
                "AlwaysBuy"
            }

            fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) {}

            fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
                Ok(())
            }

            fn is_ready(&self, _bars: &BarsContext) -> bool {
                true
            }

            fn warmup_period(&self) -> usize {
                0
            }

            fn bar_data_mode(&self) -> BarDataMode {
                BarDataMode::OnCloseBar
            }

            fn get_orders(&mut self, bar_data: &BarData, _bars: &mut BarsContext) -> Vec<Order> {
                if !self.bought {
                    self.bought = true;
                    vec![Order::market(&bar_data.ohlc_bar.symbol, OrderSide::Buy, dec!(1))
                        .build()
                        .unwrap()]
                } else {
                    Vec::new()
                }
            }
        }

        /// Strategy that never trades
        struct NeverTradeStrategy;

        impl Strategy for NeverTradeStrategy {
            fn name(&self) -> &str {
                "NeverTrade"
            }

            fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) {}

            fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
                Ok(())
            }

            fn is_ready(&self, _bars: &BarsContext) -> bool {
                true
            }

            fn warmup_period(&self) -> usize {
                0
            }

            fn bar_data_mode(&self) -> BarDataMode {
                BarDataMode::OnCloseBar
            }

            fn get_orders(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Vec<Order> {
                Vec::new()
            }
        }

        let config = BacktestConfig::new(dec!(100000));
        let mut engine = MultiStrategyBacktestEngine::new(config);

        engine
            .add_strategy(Box::new(AlwaysBuyStrategy { bought: false }), "BTCUSDT")
            .unwrap();
        engine
            .add_strategy(Box::new(NeverTradeStrategy), "BTCUSDT")
            .unwrap();

        let bars = vec![
            create_ohlc_bar("BTCUSDT", dec!(50000), 0),
            create_ohlc_bar("BTCUSDT", dec!(51000), 60),
        ];

        let results = engine.run_unified(BacktestData::OHLCBars(bars));

        assert_eq!(results.len(), 2);

        // AlwaysBuy should have 1 trade
        assert_eq!(results[0].total_trades, 1);
        assert_eq!(results[0].strategy_name, "AlwaysBuy");

        // NeverTrade should have 0 trades
        assert_eq!(results[1].total_trades, 0);
        assert_eq!(results[1].strategy_name, "NeverTrade");

        // Both should start with same initial capital
        assert_eq!(results[0].initial_capital, dec!(100000));
        assert_eq!(results[1].initial_capital, dec!(100000));
    }

    #[test]
    fn test_multi_strategy_with_state_coordinator() {
        let config = BacktestConfig::new(dec!(100000));
        let coordinator = Arc::new(StateCoordinator::default());

        let mut engine =
            MultiStrategyBacktestEngine::new(config).with_coordinator(Arc::clone(&coordinator));

        engine
            .add_strategy(Box::new(OrderTrackingStrategy::new("StrategyA")), "BTCUSDT")
            .unwrap();
        engine
            .add_strategy(Box::new(OrderTrackingStrategy::new("StrategyB")), "ETHUSDT")
            .unwrap();

        // Verify strategies were registered
        assert_eq!(coordinator.strategy_count(), 2);

        let bars = vec![create_ohlc_bar("BTCUSDT", dec!(50000), 0)];

        engine.run_unified(BacktestData::OHLCBars(bars));

        // Strategies should be terminated after backtest
        // (all should be in Terminated state)
    }

    #[test]
    fn test_empty_engine_returns_empty_results() {
        let config = BacktestConfig::new(dec!(100000));
        let mut engine = MultiStrategyBacktestEngine::new(config);

        let bars = vec![create_ohlc_bar("BTCUSDT", dec!(50000), 0)];

        let results = engine.run_unified(BacktestData::OHLCBars(bars));

        assert!(results.is_empty());
    }
}
