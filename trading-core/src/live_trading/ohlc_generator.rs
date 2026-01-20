use chrono::{DateTime, Timelike, Utc};
use rust_decimal::Decimal;
use trading_common::data::types::{BarData, BarType, OHLCData, TickData, Timeframe};

/// Builder for accumulating ticks into OHLC bars
#[derive(Debug, Clone)]
struct BarBuilder {
    symbol: String,
    bar_type: BarType,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    volume: Decimal,
    tick_count: u64,
    first_tick: Option<TickData>,
}

impl BarBuilder {
    /// Create new bar builder for time-based bars
    fn new_time_based(symbol: String, timeframe: Timeframe, start_time: DateTime<Utc>) -> Self {
        let duration = timeframe.as_duration();
        let end_time = start_time + duration;

        BarBuilder {
            symbol,
            bar_type: BarType::TimeBased(timeframe),
            start_time,
            end_time,
            open: Decimal::ZERO,
            high: Decimal::ZERO,
            low: Decimal::ZERO,
            close: Decimal::ZERO,
            volume: Decimal::ZERO,
            tick_count: 0,
            first_tick: None,
        }
    }

    /// Create new bar builder for tick-based bars
    fn new_tick_based(symbol: String, tick_count_target: u32, first_tick: &TickData) -> Self {
        BarBuilder {
            symbol,
            bar_type: BarType::TickBased(tick_count_target),
            start_time: first_tick.timestamp,
            end_time: first_tick.timestamp, // Will be updated with last tick
            open: first_tick.price,
            high: first_tick.price,
            low: first_tick.price,
            close: first_tick.price,
            volume: first_tick.quantity,
            tick_count: 1,
            first_tick: Some(first_tick.clone()),
        }
    }

    /// Add tick to this bar
    fn add_tick(&mut self, tick: &TickData) {
        if self.tick_count == 0 {
            // First tick in the bar
            self.open = tick.price;
            self.high = tick.price;
            self.low = tick.price;
            self.close = tick.price;
            self.volume = tick.quantity;
            self.first_tick = Some(tick.clone());
        } else {
            // Subsequent ticks
            if tick.price > self.high {
                self.high = tick.price;
            }
            if tick.price < self.low {
                self.low = tick.price;
            }
            self.close = tick.price;
            self.volume += tick.quantity;
        }

        self.tick_count += 1;

        // Only update end_time for tick-based bars
        // For time-based bars, end_time is fixed at start_time + duration
        match self.bar_type {
            BarType::TickBased(_) => {
                self.end_time = tick.timestamp;
            }
            BarType::TimeBased(_) => {
                // Don't update end_time - it's fixed by the timeframe
            }
        }
    }

    /// Check if this bar should close (for time-based bars)
    fn should_close_time_based(&self, tick_time: DateTime<Utc>) -> bool {
        tick_time >= self.end_time
    }

    /// Check if this bar should close (for tick-based bars)
    fn should_close_tick_based(&self, target_tick_count: u32) -> bool {
        self.tick_count >= target_tick_count as u64
    }

    /// Build the final OHLC bar
    fn build(&self) -> OHLCData {
        let timeframe = match self.bar_type {
            BarType::TimeBased(tf) => tf,
            BarType::TickBased(_) => Timeframe::OneMinute, // Placeholder
        };

        OHLCData::new(
            self.start_time,
            self.symbol.clone(),
            timeframe,
            self.open,
            self.high,
            self.low,
            self.close,
            self.volume,
            self.tick_count,
        )
    }
}

/// Real-time OHLC bar generator
///
/// Accumulates ticks into OHLC bars with timer-based closing for time-based bars.
/// Supports both time-based (1m, 5m, 1h) and tick-based (100T, 500T) bars.
pub struct RealtimeOHLCGenerator {
    current_bar: Option<BarBuilder>,
    bar_type: BarType,
    last_price: Option<Decimal>,
    symbol: String,
}

impl RealtimeOHLCGenerator {
    /// Create new generator for time-based bars
    pub fn new_time_based(symbol: String, timeframe: Timeframe) -> Self {
        RealtimeOHLCGenerator {
            current_bar: None,
            bar_type: BarType::TimeBased(timeframe),
            last_price: None,
            symbol,
        }
    }

    /// Create new generator for tick-based bars
    pub fn new_tick_based(symbol: String, tick_count: u32) -> Self {
        RealtimeOHLCGenerator {
            current_bar: None,
            bar_type: BarType::TickBased(tick_count),
            last_price: None,
            symbol,
        }
    }

    /// Create generator from BarType
    pub fn new(symbol: String, bar_type: BarType) -> Self {
        match bar_type {
            BarType::TimeBased(tf) => Self::new_time_based(symbol, tf),
            BarType::TickBased(count) => Self::new_tick_based(symbol, count),
        }
    }

    /// Process incoming tick
    ///
    /// Returns Vec<BarData>:
    /// - Empty if bar is still accumulating
    /// - One element if tick updates current bar
    /// - Two elements if bar closed and new bar started (closed bar + first tick of new bar)
    pub fn process_tick(&mut self, tick: &TickData) -> Vec<BarData> {
        self.last_price = Some(tick.price);
        let mut result = Vec::new();

        match self.bar_type {
            BarType::TimeBased(timeframe) => {
                result.extend(self.process_tick_time_based(tick, timeframe));
            }
            BarType::TickBased(tick_count) => {
                result.extend(self.process_tick_tick_based(tick, tick_count));
            }
        }

        result
    }

    /// Process tick for time-based bars
    fn process_tick_time_based(
        &mut self,
        tick: &TickData,
        timeframe: Timeframe,
    ) -> Vec<BarData> {
        let mut result = Vec::new();

        if let Some(ref mut bar) = self.current_bar {
            // Check if we need to close current bar
            if bar.should_close_time_based(tick.timestamp) {
                // Close current bar
                let closed_ohlc = bar.build();
                result.push(BarData::new(
                    None, // No current tick for closed bar
                    closed_ohlc,
                    self.bar_type,
                    false,
                    true, // Bar is closed
                    bar.tick_count,
                ));

                // Start new bar
                let aligned_start = timeframe.align_timestamp(tick.timestamp);
                let mut new_bar = BarBuilder::new_time_based(
                    self.symbol.clone(),
                    timeframe,
                    aligned_start,
                );
                new_bar.add_tick(tick);

                // Emit first tick of new bar
                let current_ohlc = new_bar.build();
                result.push(BarData::new(
                    Some(tick.clone()),
                    current_ohlc,
                    self.bar_type,
                    true, // First tick of bar
                    false,
                    new_bar.tick_count,
                ));

                self.current_bar = Some(new_bar);
            } else {
                // Add to current bar
                let is_first = bar.tick_count == 0;
                bar.add_tick(tick);
                let current_ohlc = bar.build();
                result.push(BarData::new(
                    Some(tick.clone()),
                    current_ohlc,
                    self.bar_type,
                    is_first,
                    false,
                    bar.tick_count,
                ));
            }
        } else {
            // First tick ever - start new bar
            let aligned_start = timeframe.align_timestamp(tick.timestamp);
            let mut new_bar =
                BarBuilder::new_time_based(self.symbol.clone(), timeframe, aligned_start);
            new_bar.add_tick(tick);

            let current_ohlc = new_bar.build();
            result.push(BarData::new(
                Some(tick.clone()),
                current_ohlc,
                self.bar_type,
                true, // First tick of bar
                false,
                new_bar.tick_count,
            ));

            self.current_bar = Some(new_bar);
        }

        result
    }

    /// Process tick for tick-based bars
    fn process_tick_tick_based(&mut self, tick: &TickData, target_tick_count: u32) -> Vec<BarData> {
        let mut result = Vec::new();

        if let Some(ref mut bar) = self.current_bar {
            // Check if we need to close current bar
            if bar.should_close_tick_based(target_tick_count) {
                // Close current bar
                let closed_ohlc = bar.build();
                result.push(BarData::new(
                    None, // No current tick for closed bar
                    closed_ohlc,
                    self.bar_type,
                    false,
                    true, // Bar is closed
                    bar.tick_count,
                ));

                // Start new bar with this tick
                let new_bar =
                    BarBuilder::new_tick_based(self.symbol.clone(), target_tick_count, tick);
                let current_ohlc = new_bar.build();
                result.push(BarData::new(
                    Some(tick.clone()),
                    current_ohlc,
                    self.bar_type,
                    true, // First tick of new bar
                    false,
                    new_bar.tick_count,
                ));

                self.current_bar = Some(new_bar);
            } else {
                // Add to current bar
                let is_first = bar.tick_count == 0;
                bar.add_tick(tick);
                let current_ohlc = bar.build();
                result.push(BarData::new(
                    Some(tick.clone()),
                    current_ohlc,
                    self.bar_type,
                    is_first,
                    false,
                    bar.tick_count,
                ));
            }
        } else {
            // First tick ever - start new bar
            let new_bar = BarBuilder::new_tick_based(self.symbol.clone(), target_tick_count, tick);
            let current_ohlc = new_bar.build();
            result.push(BarData::new(
                Some(tick.clone()),
                current_ohlc,
                self.bar_type,
                true, // First tick of bar
                false,
                new_bar.tick_count,
            ));

            self.current_bar = Some(new_bar);
        }

        result
    }

    /// Check if timer should close the current bar (for time-based bars)
    ///
    /// Called periodically (e.g., every 1 second) to check if bar should close
    /// even if no ticks received.
    pub fn check_timer_close(&mut self, now: DateTime<Utc>) -> Option<BarData> {
        if let BarType::TimeBased(_) = self.bar_type {
            if let Some(ref mut bar) = self.current_bar {
                if now >= bar.end_time && bar.tick_count > 0 {
                    // Close the bar
                    let closed_ohlc = bar.build();
                    let bar_data = BarData::new(
                        None,
                        closed_ohlc,
                        self.bar_type,
                        false,
                        true,
                        bar.tick_count,
                    );

                    // Clear current bar
                    self.current_bar = None;
                    return Some(bar_data);
                }
            }
        }

        None
    }

    /// Generate synthetic bar if needed (no ticks during interval)
    ///
    /// For time-based bars, if current bar is None and we're in a new period,
    /// generate a synthetic bar for the PREVIOUS period with O=H=L=C=last_known_price
    pub fn generate_synthetic_if_needed(&mut self, now: DateTime<Utc>) -> Option<BarData> {
        if let BarType::TimeBased(timeframe) = self.bar_type {
            if self.current_bar.is_none() {
                if let Some(last_price) = self.last_price {
                    // Get the current period's start
                    let current_aligned_start = timeframe.align_timestamp(now);

                    // Check if we're in a new period (past the start of current period)
                    // If so, generate synthetic for the PREVIOUS period that had no ticks
                    if now >= current_aligned_start {
                        let duration = timeframe.as_duration();
                        let previous_bar_start = current_aligned_start - duration;

                        // Generate synthetic bar for the previous period
                        let synthetic = BarData::synthetic_bar(
                            self.symbol.clone(),
                            self.bar_type,
                            previous_bar_start,
                            last_price,
                        );
                        return Some(synthetic);
                    }
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn create_tick(price: &str, timestamp: DateTime<Utc>) -> TickData {
        TickData {
            timestamp,
            symbol: "BTCUSDT".to_string(),
            price: Decimal::from_str(price).unwrap(),
            quantity: Decimal::from_str("1.0").unwrap(),
            side: trading_common::data::types::TradeSide::Buy,
            trade_id: "test".to_string(),
            is_buyer_maker: false,
        }
    }

    #[test]
    fn test_time_based_single_bar() {
        let mut gen = RealtimeOHLCGenerator::new_time_based(
            "BTCUSDT".to_string(),
            Timeframe::OneMinute,
        );

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        // First tick
        let tick1 = create_tick("50000", base_time);
        let result = gen.process_tick(&tick1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].metadata.is_first_tick_of_bar, true);
        assert_eq!(result[0].metadata.is_bar_closed, false);
        assert_eq!(result[0].metadata.tick_count_in_bar, 1);

        // Second tick in same bar
        let tick2 = create_tick("50100", base_time + Duration::seconds(30));
        let result = gen.process_tick(&tick2);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].metadata.is_first_tick_of_bar, false);
        assert_eq!(result[0].metadata.tick_count_in_bar, 2);
    }

    #[test]
    fn test_time_based_bar_close() {
        let mut gen = RealtimeOHLCGenerator::new_time_based(
            "BTCUSDT".to_string(),
            Timeframe::OneMinute,
        );

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        // First tick
        let tick1 = create_tick("50000", base_time);
        gen.process_tick(&tick1);

        // Tick in next bar - should close previous and start new
        let tick2 = create_tick("50200", base_time + Duration::minutes(1));
        let result = gen.process_tick(&tick2);

        // Should get 2 events: closed bar + first tick of new bar
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].metadata.is_bar_closed, true);
        assert_eq!(result[0].current_tick, None);
        assert_eq!(result[1].metadata.is_first_tick_of_bar, true);
        assert_eq!(result[1].metadata.is_bar_closed, false);
    }

    #[test]
    fn test_tick_based_bar() {
        let mut gen = RealtimeOHLCGenerator::new_tick_based("BTCUSDT".to_string(), 3);

        let base_time = Utc::now();

        // Process 3 ticks to complete one bar
        let tick1 = create_tick("50000", base_time);
        let result1 = gen.process_tick(&tick1);
        assert_eq!(result1.len(), 1);
        assert_eq!(result1[0].metadata.tick_count_in_bar, 1);

        let tick2 = create_tick("50100", base_time + Duration::seconds(1));
        let result2 = gen.process_tick(&tick2);
        assert_eq!(result2.len(), 1);
        assert_eq!(result2[0].metadata.tick_count_in_bar, 2);

        let tick3 = create_tick("50200", base_time + Duration::seconds(2));
        let result3 = gen.process_tick(&tick3);
        assert_eq!(result3.len(), 1);
        assert_eq!(result3[0].metadata.tick_count_in_bar, 3);

        // 4th tick should close bar and start new
        let tick4 = create_tick("50300", base_time + Duration::seconds(3));
        let result4 = gen.process_tick(&tick4);
        assert_eq!(result4.len(), 2);
        assert_eq!(result4[0].metadata.is_bar_closed, true);
        assert_eq!(result4[0].metadata.tick_count_in_bar, 3);
        assert_eq!(result4[1].metadata.is_first_tick_of_bar, true);
        assert_eq!(result4[1].metadata.tick_count_in_bar, 1);
    }

    #[test]
    fn test_timer_close() {
        let mut gen = RealtimeOHLCGenerator::new_time_based(
            "BTCUSDT".to_string(),
            Timeframe::OneMinute,
        );

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        // Add tick to start bar
        let tick = create_tick("50000", base_time);
        gen.process_tick(&tick);

        // Check timer after bar should close
        let after_bar_end = base_time + Duration::minutes(1) + Duration::seconds(5);
        let closed = gen.check_timer_close(after_bar_end);

        assert!(closed.is_some());
        let bar = closed.unwrap();
        assert_eq!(bar.metadata.is_bar_closed, true);
        assert_eq!(bar.current_tick, None);
    }

    #[test]
    fn test_synthetic_bar() {
        let mut gen = RealtimeOHLCGenerator::new_time_based(
            "BTCUSDT".to_string(),
            Timeframe::OneMinute,
        );

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        // Process one tick to establish last_price
        let tick = create_tick("50000", base_time);
        gen.process_tick(&tick);

        // Close the bar via timer
        let after_bar = base_time + Duration::minutes(1);
        gen.check_timer_close(after_bar);

        // Check for synthetic bar after another minute with no ticks
        let after_2_bars = base_time + Duration::minutes(2);
        let synthetic = gen.generate_synthetic_if_needed(after_2_bars);

        assert!(synthetic.is_some());
        let bar = synthetic.unwrap();
        assert_eq!(bar.metadata.is_synthetic, true);
        assert_eq!(bar.metadata.tick_count_in_bar, 0);
        assert_eq!(bar.ohlc_bar.open, Decimal::from_str("50000").unwrap());
    }
}
