use crate::backtest::{
    bar_generator::HistoricalOHLCGenerator, metrics::BacktestMetrics, portfolio::Portfolio,
    strategy::Strategy,
};
use crate::data::types::{BarData, OHLCData, TickData};
use crate::execution::{LatencyModel, SimulatedExchange};
use crate::orders::{
    ContingentAction, ContingentOrderManager, Order, OrderEventAny, OrderList, OrderSide,
};
use crate::risk::FeeModel;
use crate::series::bars_context::BarsContext;
// Note: OHLCData and TickData are still used via BacktestData enum
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;

/// Input data for backtesting
#[derive(Debug, Clone)]
pub enum BacktestData {
    /// Raw tick data
    Ticks(Vec<TickData>),
    /// Pre-aggregated OHLC bars
    OHLCBars(Vec<OHLCData>),
}

#[derive(Debug, Clone)]
pub struct BacktestConfig {
    pub initial_capital: Decimal,
    pub commission_rate: Decimal,
    pub strategy_params: HashMap<String, String>,
}

impl BacktestConfig {
    pub fn new(initial_capital: Decimal) -> Self {
        Self {
            initial_capital,
            commission_rate: Decimal::from_str("0.001").unwrap_or(Decimal::ZERO), // 0.1% default
            strategy_params: HashMap::new(),
        }
    }

    pub fn with_commission_rate(mut self, rate: Decimal) -> Self {
        self.commission_rate = rate;
        self
    }

    pub fn with_param(mut self, key: &str, value: &str) -> Self {
        self.strategy_params
            .insert(key.to_string(), value.to_string());
        self
    }
}

pub struct BacktestEngine {
    portfolio: Portfolio,
    strategy: Box<dyn Strategy>,
    config: BacktestConfig,
    bars_context: BarsContext,
    /// Optional simulated exchange for realistic order execution
    exchange: Option<SimulatedExchange>,
    /// Manager for contingent orders (bracket orders, OCO, etc.)
    contingent_manager: ContingentOrderManager,
}

impl BacktestEngine {
    pub fn new(strategy: Box<dyn Strategy>, config: BacktestConfig) -> Result<Self, String> {
        let mut strategy = strategy;
        strategy.reset();
        strategy.initialize(config.strategy_params.clone())?;

        // Initialize BarsContext with strategy's max lookback setting
        let max_lookback = strategy.max_bars_lookback();
        let bars_context = BarsContext::with_lookback("", max_lookback);

        let portfolio =
            Portfolio::new(config.initial_capital).with_commission_rate(config.commission_rate);

        Ok(Self {
            portfolio,
            strategy,
            config,
            bars_context,
            exchange: None,
            contingent_manager: ContingentOrderManager::new(),
        })
    }

    /// Add a simulated exchange for realistic order execution.
    ///
    /// When an exchange is configured, orders are routed through it with:
    /// - Latency modeling (orders don't fill instantly)
    /// - Per-instrument matching engines
    /// - Proper limit/stop order handling
    ///
    /// Without an exchange, orders are executed immediately at the current price.
    pub fn with_exchange(mut self, exchange: SimulatedExchange) -> Self {
        self.exchange = Some(exchange);
        self
    }

    /// Create and configure a simulated exchange with the given venue name.
    pub fn with_simulated_exchange(mut self, venue: &str) -> Self {
        self.exchange = Some(SimulatedExchange::new(venue));
        self
    }

    /// Set the latency model for the exchange.
    ///
    /// Returns self unchanged if no exchange is configured.
    pub fn with_latency_model(mut self, model: Box<dyn LatencyModel>) -> Self {
        if let Some(exchange) = self.exchange.take() {
            self.exchange = Some(exchange.with_latency_model(model));
        }
        self
    }

    /// Set the fee model for the exchange.
    ///
    /// Returns self unchanged if no exchange is configured.
    pub fn with_fee_model(mut self, model: Box<dyn FeeModel>) -> Self {
        if let Some(exchange) = self.exchange.take() {
            self.exchange = Some(exchange.with_fee_model(model));
        }
        self
    }

    /// Register an order list for contingent order management.
    ///
    /// Use this to register bracket orders, OCO orders, etc. that need
    /// automatic handling when orders fill or cancel.
    pub fn register_order_list(&mut self, list: OrderList) {
        self.contingent_manager.register_list(list);
    }

    /// Unified backtest runner using the strategy's preferred bar data mode
    ///
    /// This method automatically generates BarData events based on the strategy's
    /// `bar_data_mode()` and `preferred_bar_type()` settings.
    pub fn run_unified(&mut self, data: BacktestData) -> BacktestResult {
        let bar_type = self.strategy.preferred_bar_type();
        let mode = self.strategy.bar_data_mode();

        println!("Starting unified backtest...");
        println!("Strategy: {}", self.strategy.name());
        println!("Bar Type: {}", bar_type.as_str());
        println!(
            "Mode: {}",
            match mode {
                crate::data::types::BarDataMode::OnEachTick => "OnEachTick",
                crate::data::types::BarDataMode::OnPriceMove => "OnPriceMove",
                crate::data::types::BarDataMode::OnCloseBar => "OnCloseBar",
            }
        );
        println!("Warmup period: {} bars", self.strategy.warmup_period());
        println!("Initial capital: ${}", self.portfolio.initial_capital);
        println!(
            "Commission rate: {}%",
            self.config.commission_rate * Decimal::from(100)
        );

        // Get session configuration from strategy
        let session_config = self.strategy.session_config();
        let has_session = session_config.session_schedule.is_some();
        if has_session {
            println!(
                "Session: {} (align={}, truncate={})",
                if session_config.align_to_session_open {
                    "aligned"
                } else {
                    "not aligned"
                },
                session_config.align_to_session_open,
                session_config.truncate_at_session_close
            );
        } else {
            println!("Session: 24/7 (no boundaries)");
        }

        // Generate BarData events based on input type
        let bar_events = match data {
            BacktestData::Ticks(ticks) => {
                println!("Data points: {} ticks", ticks.len());
                // Use session-aware generator if strategy specifies session config
                let generator =
                    HistoricalOHLCGenerator::with_session_config(bar_type, mode, session_config);
                generator.generate_from_ticks(&ticks)
            }
            BacktestData::OHLCBars(ohlc_bars) => {
                println!("Data points: {} OHLC bars", ohlc_bars.len());
                // Convert OHLC bars directly to BarData
                ohlc_bars
                    .iter()
                    .map(|ohlc| BarData::from_ohlc(ohlc))
                    .collect()
            }
        };

        println!("Generated {} bar events", bar_events.len());
        println!("{}", "=".repeat(60));

        // Reset BarsContext for new run
        self.bars_context.reset();

        // Process bar events
        let mut processed = 0;
        let total = bar_events.len();
        let mut last_progress = 0;

        for bar_data in bar_events {
            // Update BarsContext BEFORE calling strategy
            self.bars_context.on_bar_update(&bar_data);

            // Update current price in portfolio
            self.portfolio
                .update_price(&bar_data.ohlc_bar.symbol, bar_data.ohlc_bar.close);

            // Execute strategy with BarsContext (populates pending orders internally)
            self.strategy.on_bar_data(&bar_data, &mut self.bars_context);

            // Skip order execution during warmup period
            if !self.strategy.is_ready(&self.bars_context) {
                processed += 1;
                continue;
            }

            // Get orders from strategy
            let orders = self.strategy.get_orders(&bar_data, &mut self.bars_context);

            // Determine execution price (use current tick if available, otherwise close)
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
                        if let Err(e) =
                            self.portfolio
                                .execute_buy(symbol.clone(), quantity, execution_price)
                        {
                            println!("Buy failed {}: {}", symbol, e);
                        } else {
                            println!("BUY {} {} @ ${}", symbol, quantity, execution_price);
                        }
                    }
                    OrderSide::Sell => {
                        if let Err(e) =
                            self.portfolio
                                .execute_sell(symbol.clone(), quantity, execution_price)
                        {
                            println!("Sell failed {}: {}", symbol, e);
                        } else {
                            println!("SELL {} {} @ ${}", symbol, quantity, execution_price);
                        }
                    }
                }
            }

            processed += 1;

            // Progress display
            let progress = (processed * 100) / total;
            if progress != last_progress && progress % 10 == 0 {
                let current_value = self.portfolio.total_value();
                let current_pnl = self.portfolio.total_pnl();
                println!(
                    "Progress: {}% ({}/{}) | Portfolio Value: ${} | P&L: ${}",
                    progress, processed, total, current_value, current_pnl
                );
                last_progress = progress;
            }
        }

        println!("\n{}", "=".repeat(60));

        // Calculate and return results
        self.calculate_results()
    }

    /// Run backtest with simulated exchange for realistic order execution.
    ///
    /// This method provides a more realistic simulation than `run_unified`:
    /// - Orders are submitted to a simulated exchange with latency modeling
    /// - Limit/stop orders only fill when price conditions are met
    /// - Contingent orders (brackets, OCO) are handled automatically
    /// - Fees are calculated by the exchange's fee model
    ///
    /// # Synchronization Model
    ///
    /// For each bar event:
    /// 1. Exchange advances time to match bar timestamp
    /// 2. Exchange processes inflight commands (latency-delayed orders)
    /// 3. Exchange matches orders against new prices → fill events
    /// 4. Fill events update portfolio and trigger contingent actions
    /// 5. Strategy sees the same bar data and makes decisions
    /// 6. New orders are submitted to exchange (with latency)
    ///
    /// This ensures orders can't fill on the same bar they're submitted.
    pub fn run_with_exchange(&mut self, data: BacktestData) -> BacktestResult {
        // Ensure we have an exchange configured
        if self.exchange.is_none() {
            println!("Warning: No exchange configured, falling back to run_unified");
            return self.run_unified(data);
        }

        let bar_type = self.strategy.preferred_bar_type();
        let mode = self.strategy.bar_data_mode();

        println!("Starting exchange-based backtest...");
        println!("Strategy: {}", self.strategy.name());
        println!("Bar Type: {}", bar_type.as_str());
        println!(
            "Mode: {}",
            match mode {
                crate::data::types::BarDataMode::OnEachTick => "OnEachTick",
                crate::data::types::BarDataMode::OnPriceMove => "OnPriceMove",
                crate::data::types::BarDataMode::OnCloseBar => "OnCloseBar",
            }
        );
        println!("Warmup period: {} bars", self.strategy.warmup_period());
        println!("Initial capital: ${}", self.portfolio.initial_capital);
        println!(
            "Exchange: {}",
            self.exchange.as_ref().map(|e| e.venue.as_str()).unwrap_or("SIMULATED")
        );
        println!("{}", "=".repeat(60));

        // Get session configuration from strategy
        let session_config = self.strategy.session_config();

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
        println!("{}", "=".repeat(60));

        // Reset state
        self.bars_context.reset();
        if let Some(exchange) = self.exchange.as_mut() {
            exchange.reset();
        }
        self.contingent_manager.clear();

        // Process bar events
        let mut processed = 0;
        let total = bar_events.len();
        let mut last_progress = 0;

        for bar_data in bar_events {
            let timestamp_ns = bar_data.ohlc_bar.timestamp.timestamp_nanos_opt().unwrap_or(0) as u64;

            // === Step 1: Exchange advances time and processes inflight commands ===
            let mut fill_events = Vec::new();
            if let Some(exchange) = self.exchange.as_mut() {
                exchange.advance_time(timestamp_ns);
                let inflight_events = exchange.process_inflight_commands();
                fill_events.extend(inflight_events);

                // === Step 2: Exchange processes bar through matching engines ===
                let match_events = exchange.process_bar(&bar_data.ohlc_bar);
                fill_events.extend(match_events);
            }

            // === Step 3: Handle fill events ===
            for event in &fill_events {
                self.handle_order_event(event, &bar_data);
            }

            // Update BarsContext BEFORE calling strategy
            self.bars_context.on_bar_update(&bar_data);

            // Update current price in portfolio
            self.portfolio
                .update_price(&bar_data.ohlc_bar.symbol, bar_data.ohlc_bar.close);

            // === Step 4: Strategy sees the same bar data ===
            self.strategy.on_bar_data(&bar_data, &mut self.bars_context);

            // Skip order submission during warmup period
            if !self.strategy.is_ready(&self.bars_context) {
                processed += 1;
                continue;
            }

            // === Step 5: Submit new orders (with latency) ===
            let orders = self.strategy.get_orders(&bar_data, &mut self.bars_context);
            for order in orders {
                self.submit_order(order);
            }

            // Handle order cancellations
            let cancellations = self.strategy.get_cancellations(&bar_data, &mut self.bars_context);
            for order_id in cancellations {
                if let Some(exchange) = self.exchange.as_mut() {
                    exchange.cancel_order(order_id);
                }
            }

            processed += 1;

            // Progress display
            let progress = (processed * 100) / total;
            if progress != last_progress && progress % 10 == 0 {
                let current_value = self.portfolio.total_value();
                let current_pnl = self.portfolio.total_pnl();
                let open_orders = self.exchange.as_ref().map(|e| e.open_order_count()).unwrap_or(0);
                println!(
                    "Progress: {}% | Portfolio: ${} | P&L: ${} | Open Orders: {}",
                    progress, current_value, current_pnl, open_orders
                );
                last_progress = progress;
            }
        }

        // Final processing - flush any remaining inflight orders
        if let Some(exchange) = self.exchange.as_mut() {
            // Advance time far into the future to process remaining orders
            exchange.advance_time(u64::MAX);
            let remaining_events = exchange.process_inflight_commands();
            for event in &remaining_events {
                // Note: Using a synthetic bar_data for final fills
                if let OrderEventAny::Filled(fill) = event {
                    let symbol = fill.instrument_id.symbol.clone();
                    let side = fill.order_side;
                    let quantity = fill.last_qty;
                    let price = fill.last_px;
                    let commission = fill.commission;

                    // Use _with_commission to avoid double-counting
                    match side {
                        OrderSide::Buy => {
                            let _ = self.portfolio.execute_buy_with_commission(
                                symbol.clone(),
                                quantity,
                                price,
                                commission,
                            );
                        }
                        OrderSide::Sell => {
                            let _ = self.portfolio.execute_sell_with_commission(
                                symbol.clone(),
                                quantity,
                                price,
                                commission,
                            );
                        }
                    }
                }
            }

            println!("\nFinal state:");
            println!("  Open orders remaining: {}", exchange.open_order_count());
        }

        println!("\n{}", "=".repeat(60));

        self.calculate_results()
    }

    /// Handle an order event from the exchange.
    fn handle_order_event(&mut self, event: &OrderEventAny, _bar_data: &BarData) {
        // Notify strategy of order update
        self.strategy.on_order_update(event);

        match event {
            OrderEventAny::Filled(fill) => {
                let symbol = fill.instrument_id.symbol.clone();
                let side = fill.order_side;
                let quantity = fill.last_qty;
                let price = fill.last_px;
                let commission = fill.commission;

                // Execute the fill in portfolio with explicit commission
                // Uses _with_commission methods to avoid double-counting
                // (exchange already calculated the commission in the fill event)
                match side {
                    OrderSide::Buy => {
                        if let Err(e) = self.portfolio.execute_buy_with_commission(
                            symbol.clone(),
                            quantity,
                            price,
                            commission,
                        ) {
                            println!("Buy failed {}: {}", symbol, e);
                        } else {
                            println!(
                                "FILL BUY {} {} @ ${} (commission: ${})",
                                symbol, quantity, price, commission
                            );
                        }
                    }
                    OrderSide::Sell => {
                        if let Err(e) = self.portfolio.execute_sell_with_commission(
                            symbol.clone(),
                            quantity,
                            price,
                            commission,
                        ) {
                            println!("Sell failed {}: {}", symbol, e);
                        } else {
                            println!(
                                "FILL SELL {} {} @ ${} (commission: ${})",
                                symbol, quantity, price, commission
                            );
                        }
                    }
                }

                // Note: Commission is already included in execute_*_with_commission
                // No separate add_commission call needed

                // Notify strategy of execution
                self.strategy.on_execution(fill);

                // Process contingent order actions
                let actions = self.contingent_manager.on_order_event(event);
                self.execute_contingent_actions(actions);
            }
            OrderEventAny::Canceled(cancel) => {
                self.strategy.on_order_canceled(cancel);

                // Process contingent order actions (OCO cancels siblings)
                let actions = self.contingent_manager.on_order_event(event);
                self.execute_contingent_actions(actions);
            }
            OrderEventAny::Rejected(reject) => {
                println!("Order rejected: {} - {}", reject.client_order_id, reject.reason);
                self.strategy.on_order_rejected(reject);
            }
            OrderEventAny::Accepted(_) => {
                // Order accepted, nothing special to do
            }
            _ => {}
        }
    }

    /// Submit an order to the exchange or execute immediately.
    fn submit_order(&mut self, order: Order) {
        // Notify strategy of order submission
        self.strategy.on_order_submitted(&order);

        if let Some(exchange) = self.exchange.as_mut() {
            // Submit to exchange with latency
            exchange.submit_order(order);
        } else {
            // Immediate execution (legacy behavior)
            let symbol = order.symbol().to_string();
            let quantity = order.quantity;
            let price = order.price.unwrap_or(Decimal::ZERO);

            match order.side {
                OrderSide::Buy => {
                    let _ = self.portfolio.execute_buy(symbol, quantity, price);
                }
                OrderSide::Sell => {
                    let _ = self.portfolio.execute_sell(symbol, quantity, price);
                }
            }
        }
    }

    /// Execute contingent order actions.
    fn execute_contingent_actions(&mut self, actions: Vec<ContingentAction>) {
        for action in actions {
            match action {
                ContingentAction::SubmitOrders(orders) => {
                    for order in orders {
                        println!("Contingent: submitting {}", order.client_order_id);
                        self.submit_order(order);
                    }
                }
                ContingentAction::CancelOrders(order_ids) => {
                    if let Some(exchange) = self.exchange.as_mut() {
                        for order_id in order_ids {
                            println!("Contingent: canceling {}", order_id);
                            exchange.cancel_order(order_id);
                        }
                    }
                }
                ContingentAction::UpdateOrders(updates) => {
                    if let Some(exchange) = self.exchange.as_mut() {
                        for (order_id, new_qty) in updates {
                            println!("Contingent: updating {} to qty {}", order_id, new_qty);
                            exchange.modify_order(order_id, None, Some(new_qty));
                        }
                    }
                }
            }
        }
    }

    /// Calculate backtest results from current portfolio state
    fn calculate_results(&self) -> BacktestResult {
        let final_value = self.portfolio.total_value();
        let total_pnl = self.portfolio.total_pnl();
        let total_return_pct = if self.portfolio.initial_capital > Decimal::ZERO {
            (total_pnl / self.portfolio.initial_capital) * Decimal::from(100)
        } else {
            Decimal::ZERO
        };

        // Calculate performance metrics
        let equity_curve = self.portfolio.get_equity_curve();
        let returns = Self::calculate_returns(&equity_curve);

        let max_drawdown = BacktestMetrics::calculate_max_drawdown(&equity_curve);
        let sharpe_ratio = BacktestMetrics::calculate_sharpe_ratio(&returns, Decimal::ZERO);
        let volatility = BacktestMetrics::calculate_volatility(&returns);
        let win_rate = BacktestMetrics::calculate_win_rate(&self.portfolio.trades);
        let profit_factor = BacktestMetrics::calculate_profit_factor(&self.portfolio.trades);
        let avg_trade_duration =
            BacktestMetrics::calculate_average_trade_duration(&self.portfolio.trades);

        BacktestResult {
            initial_capital: self.portfolio.initial_capital,
            final_value,
            total_pnl,
            return_percentage: total_return_pct,
            total_trades: self.portfolio.trades.len(),
            winning_trades: self.count_winning_trades(),
            losing_trades: self.count_losing_trades(),
            max_drawdown,
            sharpe_ratio,
            volatility,
            win_rate,
            profit_factor,
            avg_trade_duration_seconds: avg_trade_duration,
            total_commission: self.portfolio.total_commission(),
            positions: self.portfolio.positions.clone(),
            trades: self.portfolio.trades.clone(),
            equity_curve,
            strategy_name: self.strategy.name().to_string(),
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

    fn count_winning_trades(&self) -> usize {
        self.portfolio
            .trades
            .iter()
            .filter(|trade| trade.realized_pnl.map_or(false, |pnl| pnl > Decimal::ZERO))
            .count()
    }

    fn count_losing_trades(&self) -> usize {
        self.portfolio
            .trades
            .iter()
            .filter(|trade| trade.realized_pnl.map_or(false, |pnl| pnl < Decimal::ZERO))
            .count()
    }
}

#[derive(Debug)]
pub struct BacktestResult {
    pub initial_capital: Decimal,
    pub final_value: Decimal,
    pub total_pnl: Decimal,
    pub return_percentage: Decimal,
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub max_drawdown: Decimal,
    pub sharpe_ratio: Decimal,
    pub volatility: Decimal,
    pub win_rate: Decimal,
    pub profit_factor: Decimal,
    pub avg_trade_duration_seconds: f64,
    pub total_commission: Decimal,
    pub positions: HashMap<String, crate::backtest::portfolio::Position>,
    pub trades: Vec<crate::backtest::portfolio::Trade>,
    pub equity_curve: Vec<Decimal>,
    pub strategy_name: String,
}

impl BacktestResult {
    pub fn print_summary(&self) {
        println!("BACKTEST RESULTS SUMMARY");
        println!("{}", "=".repeat(60));
        println!("Strategy: {}", self.strategy_name);
        println!("Initial Capital: ${}", self.initial_capital);
        println!("Final Value: ${}", self.final_value);
        println!("Total P&L: ${}", self.total_pnl);
        println!("Return: {:.2}%", self.return_percentage);
        println!("Total Commission: ${}", self.total_commission);
        println!();

        println!("TRADING STATISTICS");
        println!("{}", "-".repeat(30));
        println!("Total Trades: {}", self.total_trades);

        if self.total_trades > 0 {
            println!(
                "Winning Trades: {} ({:.1}%)",
                self.winning_trades, self.win_rate
            );
            println!(
                "Losing Trades: {} ({:.1}%)",
                self.losing_trades,
                100.0 - self.win_rate.to_f64().unwrap_or(0.0)
            );
            println!("Profit Factor: {:.2}", self.profit_factor);
            println!(
                "Avg Trade Duration: {:.0} seconds",
                self.avg_trade_duration_seconds
            );
        }
        println!();

        println!("RISK METRICS");
        println!("{}", "-".repeat(30));
        println!(
            "Max Drawdown: {:.2}%",
            self.max_drawdown * Decimal::from(100)
        );
        println!("Sharpe Ratio: {:.2}", self.sharpe_ratio);
        println!("Volatility: {:.2}%", self.volatility * Decimal::from(100));
        println!();

        if !self.positions.is_empty() {
            println!("CURRENT POSITIONS");
            println!("{}", "-".repeat(30));
            for (symbol, position) in &self.positions {
                println!(
                    "{}: {} @ ${} (Unrealized P&L: ${})",
                    symbol, position.quantity, position.avg_price, position.unrealized_pnl
                );
            }
            println!();
        }

        if self.total_trades > 0 {
            println!("RECENT TRADES (Last 5)");
            println!("{}", "-".repeat(30));
            for trade in self.trades.iter().rev().take(5) {
                let pnl_str = trade
                    .realized_pnl
                    .map(|pnl| format!("(P&L: ${})", pnl))
                    .unwrap_or_default();
                println!(
                    "{} {} {} @ ${} {}",
                    trade.timestamp.format("%Y-%m-%d %H:%M:%S"),
                    match trade.side {
                        crate::data::types::TradeSide::Buy => "BUY ",
                        crate::data::types::TradeSide::Sell => "SELL",
                    },
                    trade.symbol,
                    trade.price,
                    pnl_str
                );
            }
        }

        println!("{}", "=".repeat(60));
    }

    pub fn is_profitable(&self) -> bool {
        self.total_pnl > Decimal::ZERO
    }

    pub fn calmar_ratio(&self) -> Decimal {
        BacktestMetrics::calculate_calmar_ratio(
            self.return_percentage / Decimal::from(100),
            self.max_drawdown,
        )
    }

    pub fn print_trade_analysis(&self) {
        if self.trades.is_empty() {
            println!("No trades executed");
            return;
        }

        println!("DETAILED TRADE ANALYSIS");
        println!("{}", "=".repeat(80));

        let mut buy_trades = Vec::new();
        let mut sell_trades = Vec::new();

        for trade in &self.trades {
            match trade.side {
                crate::data::types::TradeSide::Buy => buy_trades.push(trade),
                crate::data::types::TradeSide::Sell => sell_trades.push(trade),
            }
        }

        println!("Buy Trades: {}", buy_trades.len());
        println!("Sell Trades: {}", sell_trades.len());

        if !sell_trades.is_empty() {
            let profitable_sells = sell_trades
                .iter()
                .filter(|t| t.realized_pnl.map_or(false, |pnl| pnl > Decimal::ZERO))
                .count();

            let total_profit: Decimal = sell_trades
                .iter()
                .filter_map(|t| t.realized_pnl)
                .filter(|&pnl| pnl > Decimal::ZERO)
                .sum();

            let total_loss: Decimal = sell_trades
                .iter()
                .filter_map(|t| t.realized_pnl)
                .filter(|&pnl| pnl < Decimal::ZERO)
                .sum();

            println!(
                "Profitable Sells: {} ({:.1}%)",
                profitable_sells,
                (profitable_sells as f64 / sell_trades.len() as f64) * 100.0
            );
            println!("Total Gross Profit: ${}", total_profit);
            println!("Total Gross Loss: ${}", total_loss);

            if profitable_sells > 0 {
                println!(
                    "Average Profit per Winning Trade: ${}",
                    total_profit / Decimal::from(profitable_sells)
                );
            }

            let losing_sells = sell_trades.len() - profitable_sells;
            if losing_sells > 0 {
                println!(
                    "Average Loss per Losing Trade: ${}",
                    total_loss / Decimal::from(losing_sells)
                );
            }
        }

        println!("{}", "=".repeat(80));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{BarDataMode, Timeframe};
    use crate::execution::{FixedLatencyModel, SimulatedExchange};
    use crate::orders::Order;
    use crate::risk::PercentageFeeModel;
    use crate::series::bars_context::BarsContext;
    use chrono::Utc;
    use rust_decimal_macros::dec;

    /// A simple test strategy that buys when price drops and sells when it rises
    struct SimpleTestStrategy {
        pending_orders: Vec<Order>,
        bought: bool,
    }

    impl SimpleTestStrategy {
        fn new() -> Self {
            Self {
                pending_orders: Vec::new(),
                bought: false,
            }
        }
    }

    impl Strategy for SimpleTestStrategy {
        fn name(&self) -> &str {
            "SimpleTestStrategy"
        }

        fn on_bar_data(&mut self, bar_data: &BarData, _bars: &mut BarsContext) {
            self.pending_orders.clear();

            let close = bar_data.ohlc_bar.close;
            let symbol = bar_data.ohlc_bar.symbol.as_str();

            // Simple logic: buy if price < 50000, sell if price > 51000
            if close < dec!(50000) && !self.bought {
                self.pending_orders.push(
                    Order::market(symbol, OrderSide::Buy, dec!(1))
                        .with_venue("SIMULATED")
                        .build()
                        .unwrap(),
                );
                self.bought = true;
            } else if close > dec!(51000) && self.bought {
                self.pending_orders.push(
                    Order::market(symbol, OrderSide::Sell, dec!(1))
                        .with_venue("SIMULATED")
                        .build()
                        .unwrap(),
                );
                self.bought = false;
            }
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
            std::mem::take(&mut self.pending_orders)
        }
    }

    fn create_ohlc_bar(symbol: &str, close: Decimal, timestamp_offset_secs: i64) -> OHLCData {
        OHLCData {
            timestamp: Utc::now() + chrono::Duration::seconds(timestamp_offset_secs),
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
    fn test_backtest_engine_without_exchange() {
        let strategy = Box::new(SimpleTestStrategy::new());
        let config = BacktestConfig::new(dec!(100000));

        let mut engine = BacktestEngine::new(strategy, config).unwrap();

        // Create OHLC bars that should trigger buy then sell
        let bars = vec![
            create_ohlc_bar("BTCUSDT", dec!(49500), 0),   // Buy signal
            create_ohlc_bar("BTCUSDT", dec!(50500), 60),  // Hold
            create_ohlc_bar("BTCUSDT", dec!(51500), 120), // Sell signal
        ];

        let result = engine.run_unified(BacktestData::OHLCBars(bars));

        // Should have 2 trades (buy + sell)
        assert_eq!(result.total_trades, 2);
        assert!(result.final_value > Decimal::ZERO);
    }

    #[test]
    fn test_backtest_engine_with_exchange() {
        let strategy = Box::new(SimpleTestStrategy::new());
        let config = BacktestConfig::new(dec!(100000));

        let exchange = SimulatedExchange::new("SIMULATED");

        let mut engine = BacktestEngine::new(strategy, config)
            .unwrap()
            .with_exchange(exchange);

        // Create OHLC bars that should trigger buy then sell
        // Note: With no latency, orders are accepted on bar N but fill on bar N+1
        // when the next bar processes the matching engine
        let bars = vec![
            create_ohlc_bar("BTCUSDT", dec!(49500), 0),   // Bar 0: Buy signal submitted
            create_ohlc_bar("BTCUSDT", dec!(50500), 60),  // Bar 1: Buy fills (from bar 0)
            create_ohlc_bar("BTCUSDT", dec!(51500), 120), // Bar 2: Sell signal submitted
            create_ohlc_bar("BTCUSDT", dec!(51600), 180), // Bar 3: Sell fills (from bar 2)
        ];

        let result = engine.run_with_exchange(BacktestData::OHLCBars(bars));

        // Should have 2 trades (buy + sell) executed through exchange
        assert_eq!(result.total_trades, 2);
        assert!(result.final_value > Decimal::ZERO);
    }

    #[test]
    fn test_backtest_engine_with_latency() {
        let strategy = Box::new(SimpleTestStrategy::new());
        let config = BacktestConfig::new(dec!(100000));

        // 30-second latency - orders submitted on bar N fill on bar N+1
        let latency_model = FixedLatencyModel::new(30_000_000_000); // 30 seconds in nanos
        let exchange =
            SimulatedExchange::new("SIMULATED").with_latency_model(Box::new(latency_model));

        let mut engine = BacktestEngine::new(strategy, config)
            .unwrap()
            .with_exchange(exchange);

        // Create bars with 60-second spacing
        let bars = vec![
            create_ohlc_bar("BTCUSDT", dec!(49500), 0),   // Buy signal submitted
            create_ohlc_bar("BTCUSDT", dec!(49600), 60),  // Buy fills after latency
            create_ohlc_bar("BTCUSDT", dec!(51500), 120), // Sell signal submitted
            create_ohlc_bar("BTCUSDT", dec!(51600), 180), // Sell fills after latency
        ];

        let result = engine.run_with_exchange(BacktestData::OHLCBars(bars));

        // Orders should still execute but with delay
        assert_eq!(result.total_trades, 2);
    }

    #[test]
    fn test_backtest_engine_with_fees() {
        let strategy = Box::new(SimpleTestStrategy::new());
        let config = BacktestConfig::new(dec!(100000));

        // 0.1% fee model
        let fee_model = PercentageFeeModel::new(dec!(0.001), dec!(0.001));
        let exchange = SimulatedExchange::new("SIMULATED").with_fee_model(Box::new(fee_model));

        let mut engine = BacktestEngine::new(strategy, config)
            .unwrap()
            .with_exchange(exchange);

        let bars = vec![
            create_ohlc_bar("BTCUSDT", dec!(49500), 0),   // Buy signal submitted
            create_ohlc_bar("BTCUSDT", dec!(49600), 60),  // Buy fills
            create_ohlc_bar("BTCUSDT", dec!(51500), 120), // Sell signal submitted
            create_ohlc_bar("BTCUSDT", dec!(51600), 180), // Sell fills
        ];

        let result = engine.run_with_exchange(BacktestData::OHLCBars(bars));

        // Should have 2 trades with commission from exchange fee model
        assert_eq!(result.total_trades, 2);
    }

    #[test]
    fn test_backtest_engine_builder_pattern() {
        let strategy = Box::new(SimpleTestStrategy::new());
        let config = BacktestConfig::new(dec!(100000));

        let engine = BacktestEngine::new(strategy, config)
            .unwrap()
            .with_simulated_exchange("TEST_VENUE")
            .with_latency_model(Box::new(FixedLatencyModel::new(10_000_000)));

        assert!(engine.exchange.is_some());
        assert_eq!(engine.exchange.as_ref().unwrap().venue, "TEST_VENUE");
    }

    #[test]
    fn test_backtest_engine_fallback_without_exchange() {
        let strategy = Box::new(SimpleTestStrategy::new());
        let config = BacktestConfig::new(dec!(100000));

        let mut engine = BacktestEngine::new(strategy, config).unwrap();
        // No exchange configured

        let bars = vec![create_ohlc_bar("BTCUSDT", dec!(49500), 0)];

        // run_with_exchange should fall back to run_unified
        let result = engine.run_with_exchange(BacktestData::OHLCBars(bars));

        // Should still work (falls back to run_unified)
        assert!(result.initial_capital == dec!(100000));
    }
}
