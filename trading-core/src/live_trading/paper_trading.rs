// src/live_trading/paper_trading.rs
use chrono::Utc;
use rust_decimal::Decimal;
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use crate::live_trading::RealtimeOHLCGenerator;
use crate::metrics;
use trading_common::backtest::strategy::{Signal, Strategy};
use trading_common::data::repository::TickDataRepository;
use trading_common::data::types::{BarData, LiveStrategyLog, TickData};
use trading_common::series::bars_context::BarsContext;

pub struct PaperTradingProcessor {
    strategy: Box<dyn Strategy + Send>,
    repository: Arc<TickDataRepository>,
    pub(crate) initial_capital: Decimal,

    // OHLC bar generator for unified strategy interface
    ohlc_generator: Option<RealtimeOHLCGenerator>,

    // BarsContext for strategy access to historical series
    bars_context: BarsContext,

    //Simple status tracking
    pub(crate) cash: Decimal,
    pub(crate) position: Decimal,
    pub(crate) avg_cost: Decimal,
    pub(crate) total_trades: u64,
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
        }
    }

    pub async fn process_tick(&mut self, tick: &TickData) -> Result<(), String> {
        // Start timer for paper trading tick processing
        let timer = metrics::PAPER_TICK_DURATION.start_timer();
        let start_time = Instant::now();

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
        let mut last_signal_type = "HOLD".to_string();
        for bar_data in bar_events {
            // Update BarsContext BEFORE calling strategy
            self.bars_context.on_bar_update(&bar_data);

            // Execute strategy with BarData and BarsContext
            let mut signal = self.strategy.on_bar_data(&bar_data, &mut self.bars_context);

            // Suppress signals during warmup (parity with backtest engine)
            if !self.strategy.is_ready(&self.bars_context) {
                match signal {
                    Signal::Buy { .. } | Signal::Sell { .. } => {
                        signal = Signal::Hold;
                    }
                    Signal::Hold => {}
                }
            }

            // Execute trading signal
            let signal_type = self.execute_signal(&signal, tick)?;

            // Track last non-HOLD signal for logging
            if signal_type != "HOLD" {
                last_signal_type = signal_type;
            }
        }

        // 4. Calculate Portfolio Value
        let portfolio_value = self.calculate_portfolio_value(tick.price);
        let total_pnl = portfolio_value - self.initial_capital;

        // Update portfolio metrics
        metrics::PAPER_PORTFOLIO_VALUE.set(portfolio_value.to_string().parse::<f64>().unwrap_or(0.0));
        metrics::PAPER_PNL.set(total_pnl.to_string().parse::<f64>().unwrap_or(0.0));

        // 5. Record to database
        let processing_time = start_time.elapsed().as_micros() as u64;
        let log = LiveStrategyLog {
            timestamp: tick.timestamp,
            strategy_id: self.strategy.name().to_string(),
            symbol: tick.symbol.clone(),
            current_price: tick.price,
            signal_type: last_signal_type.clone(),
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
            &last_signal_type,
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
            let mut signal = self.strategy.on_bar_data(&bar_data, &mut self.bars_context);

            // Suppress signals during warmup (parity with backtest engine)
            if !self.strategy.is_ready(&self.bars_context) {
                match signal {
                    Signal::Buy { .. } | Signal::Sell { .. } => {
                        signal = Signal::Hold;
                    }
                    Signal::Hold => {}
                }
            }

            // Use bar's close price for execution
            let execution_price = bar_data.ohlc_bar.close;

            // Create a synthetic tick for signal execution
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

            self.execute_signal(&signal, &synthetic_tick)?;
        }

        Ok(())
    }

    fn execute_signal(&mut self, signal: &Signal, tick: &TickData) -> Result<String, String> {
        match signal {
            Signal::Buy { quantity, .. } => {
                let cost = quantity * tick.price;

                if cost <= self.cash {
                    if self.position == Decimal::ZERO {
                        self.position = *quantity;
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
                        "BUY signal ignored: insufficient cash ({} needed, {} available)",
                        cost, self.cash
                    );
                }
            }

            Signal::Sell { quantity, .. } => {
                if *quantity <= self.position {
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
                        "SELL signal ignored: insufficient position ({} needed, {} available)",
                        quantity, self.position
                    );
                }
            }

            Signal::Hold => return Ok("HOLD".to_string()),
        }

        Ok("HOLD".to_string())
    }

    pub(crate) fn calculate_portfolio_value(&self, current_price: Decimal) -> Decimal {
        self.cash + (self.position * current_price)
    }

    fn log_activity(
        &self,
        signal_type: &str,
        tick: &TickData,
        portfolio_value: Decimal,
        total_pnl: Decimal,
        cache_hit: bool,
        cache_time_us: u64,
        total_time_us: u64,
    ) {
        if signal_type != "HOLD" {
            let return_pct = if self.initial_capital > Decimal::ZERO {
                total_pnl / self.initial_capital * Decimal::from(100)
            } else {
                Decimal::ZERO
            };

            println!("🎯 {} {} @ ${} | Portfolio: ${} | P&L: ${} ({:.2}%) | Position: {} | Cash: ${} | Trades: {} | Cache: {} ({}μs) | Total: {}μs",
                     signal_type,
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
