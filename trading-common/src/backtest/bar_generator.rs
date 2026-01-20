use crate::data::types::{BarData, BarDataMode, BarType, OHLCData, TickData, Timeframe};
use chrono::{DateTime, Timelike, Utc};
use rust_decimal::Decimal;

/// Historical OHLC bar generator for backtesting
///
/// Converts historical tick data into BarData events based on the strategy's
/// preferred bar type and operational mode.
pub struct HistoricalOHLCGenerator {
    bar_type: BarType,
    mode: BarDataMode,
}

impl HistoricalOHLCGenerator {
    /// Create new historical bar generator
    pub fn new(bar_type: BarType, mode: BarDataMode) -> Self {
        HistoricalOHLCGenerator { bar_type, mode }
    }

    /// Generate BarData events from historical tick data
    ///
    /// Returns a vector of BarData events according to the mode:
    /// - OnEachTick: One event per tick with OHLC state
    /// - OnPriceMove: One event per price change with OHLC state
    /// - OnCloseBar: One event per completed bar (no tick reference)
    pub fn generate_from_ticks(&self, ticks: &[TickData]) -> Vec<BarData> {
        if ticks.is_empty() {
            return Vec::new();
        }

        match self.bar_type {
            BarType::TimeBased(timeframe) => {
                self.generate_time_based_bars(ticks, timeframe)
            }
            BarType::TickBased(tick_count) => {
                self.generate_tick_based_bars(ticks, tick_count)
            }
        }
    }

    /// Generate time-based bars from ticks
    fn generate_time_based_bars(
        &self,
        ticks: &[TickData],
        timeframe: Timeframe,
    ) -> Vec<BarData> {
        let mut result = Vec::new();
        let mut current_window_start: Option<DateTime<Utc>> = None;
        let mut window_ticks: Vec<TickData> = Vec::new();
        let mut last_price: Option<Decimal> = None;

        for tick in ticks {
            let tick_window = timeframe.align_timestamp(tick.timestamp);

            // Check if we're starting a new window
            if current_window_start.is_none() || current_window_start.unwrap() != tick_window {
                // Close previous window if exists
                if let Some(window_start) = current_window_start {
                    if !window_ticks.is_empty() {
                        result.extend(self.process_completed_window(
                            &window_ticks,
                            window_start,
                            timeframe,
                        ));
                        last_price = Some(window_ticks.last().unwrap().price);

                        // Generate synthetic bars for any gaps
                        let symbol = &window_ticks[0].symbol;
                        result.extend(self.generate_synthetic_bars_for_gap(
                            window_start,
                            tick_window,
                            timeframe,
                            last_price.unwrap(),
                            symbol,
                        ));
                    }
                }

                // Start new window
                current_window_start = Some(tick_window);
                window_ticks.clear();
            }

            window_ticks.push(tick.clone());
        }

        // Process final window
        if let Some(window_start) = current_window_start {
            if !window_ticks.is_empty() {
                result.extend(self.process_completed_window(
                    &window_ticks,
                    window_start,
                    timeframe,
                ));
            }
        }

        result
    }

    /// Generate synthetic bars for gaps between windows
    fn generate_synthetic_bars_for_gap(
        &self,
        last_window_start: DateTime<Utc>,
        current_window_start: DateTime<Utc>,
        timeframe: Timeframe,
        last_price: Decimal,
        symbol: &str,
    ) -> Vec<BarData> {
        let mut result = Vec::new();
        let duration = timeframe.as_duration();

        // Calculate how many periods are missing
        let mut gap_window_start = last_window_start + duration;

        while gap_window_start < current_window_start {
            // Generate synthetic bar for this gap period
            let synthetic = BarData::synthetic_bar(
                symbol.to_string(),
                self.bar_type,
                gap_window_start,
                last_price,
            );

            result.push(synthetic);
            gap_window_start = gap_window_start + duration;
        }

        result
    }

    /// Process a completed time window based on the mode
    fn process_completed_window(
        &self,
        ticks: &[TickData],
        window_start: DateTime<Utc>,
        timeframe: Timeframe,
    ) -> Vec<BarData> {
        if ticks.is_empty() {
            return Vec::new();
        }

        // Generate OHLC bar for this window
        let ohlc = OHLCData::from_ticks(ticks, timeframe, window_start)
            .expect("Should have OHLC from non-empty ticks");

        match self.mode {
            BarDataMode::OnEachTick => {
                // Emit event for each tick with accumulated OHLC state
                let mut result = Vec::new();
                let mut accumulated_ticks = Vec::new();

                for (idx, tick) in ticks.iter().enumerate() {
                    accumulated_ticks.push(tick.clone());

                    // Generate OHLC from ticks so far
                    let partial_ohlc = OHLCData::from_ticks(
                        &accumulated_ticks,
                        timeframe,
                        window_start,
                    )
                    .expect("Should have OHLC from accumulated ticks");

                    result.push(BarData::new(
                        Some(tick.clone()),
                        partial_ohlc,
                        self.bar_type,
                        idx == 0, // First tick of bar
                        false,    // Bar not closed yet
                        accumulated_ticks.len() as u64,
                    ));
                }

                result
            }
            BarDataMode::OnPriceMove => {
                // Emit only when price changes
                let mut result = Vec::new();
                let mut accumulated_ticks = Vec::new();
                let mut last_price: Option<Decimal> = None;

                for (idx, tick) in ticks.iter().enumerate() {
                    accumulated_ticks.push(tick.clone());

                    // Check if price changed
                    let price_changed = last_price.map_or(true, |last| last != tick.price);

                    if price_changed {
                        let partial_ohlc = OHLCData::from_ticks(
                            &accumulated_ticks,
                            timeframe,
                            window_start,
                        )
                        .expect("Should have OHLC from accumulated ticks");

                        result.push(BarData::new(
                            Some(tick.clone()),
                            partial_ohlc,
                            self.bar_type,
                            idx == 0, // First tick of bar
                            false,
                            accumulated_ticks.len() as u64,
                        ));

                        last_price = Some(tick.price);
                    }
                }

                result
            }
            BarDataMode::OnCloseBar => {
                // Emit only when bar is complete
                vec![BarData::new(
                    None, // No current tick for closed bar
                    ohlc,
                    self.bar_type,
                    false,
                    true, // Bar is closed
                    ticks.len() as u64,
                )]
            }
        }
    }

    /// Generate tick-based bars from ticks
    fn generate_tick_based_bars(&self, ticks: &[TickData], tick_count: u32) -> Vec<BarData> {
        let mut result = Vec::new();
        let tick_count_usize = tick_count as usize;

        for chunk in ticks.chunks(tick_count_usize) {
            if chunk.is_empty() {
                continue;
            }

            let window_start = chunk[0].timestamp;
            let timeframe = Timeframe::OneMinute; // Placeholder for tick-based bars

            match self.mode {
                BarDataMode::OnEachTick => {
                    // Emit event for each tick with accumulated OHLC state
                    let mut accumulated_ticks = Vec::new();
                    let is_full_bar = chunk.len() == tick_count_usize;

                    for (idx, tick) in chunk.iter().enumerate() {
                        accumulated_ticks.push(tick.clone());

                        let partial_ohlc = OHLCData::from_ticks(
                            &accumulated_ticks,
                            timeframe,
                            window_start,
                        )
                        .expect("Should have OHLC from accumulated ticks");

                        let is_last_in_chunk = idx == chunk.len() - 1;
                        result.push(BarData::new(
                            Some(tick.clone()),
                            partial_ohlc,
                            self.bar_type,
                            idx == 0, // First tick of bar
                            is_last_in_chunk && is_full_bar, // Bar closed only if full and last tick
                            accumulated_ticks.len() as u64,
                        ));
                    }
                }
                BarDataMode::OnPriceMove => {
                    // Emit only when price changes
                    let mut accumulated_ticks = Vec::new();
                    let mut last_price: Option<Decimal> = None;
                    let is_full_bar = chunk.len() == tick_count_usize;

                    for (idx, tick) in chunk.iter().enumerate() {
                        accumulated_ticks.push(tick.clone());

                        let price_changed = last_price.map_or(true, |last| last != tick.price);

                        if price_changed {
                            let partial_ohlc = OHLCData::from_ticks(
                                &accumulated_ticks,
                                timeframe,
                                window_start,
                            )
                            .expect("Should have OHLC from accumulated ticks");

                            let is_last_in_chunk = idx == chunk.len() - 1;
                            result.push(BarData::new(
                                Some(tick.clone()),
                                partial_ohlc,
                                self.bar_type,
                                idx == 0,
                                is_last_in_chunk && is_full_bar,
                                accumulated_ticks.len() as u64,
                            ));

                            last_price = Some(tick.price);
                        }
                    }
                }
                BarDataMode::OnCloseBar => {
                    // Emit only when bar is complete
                    let ohlc = OHLCData::from_ticks(chunk, timeframe, window_start)
                        .expect("Should have OHLC from chunk");

                    result.push(BarData::new(
                        None,
                        ohlc,
                        self.bar_type,
                        false,
                        true, // Bar is closed
                        chunk.len() as u64,
                    ));
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::TradeSide;
    use chrono::Duration;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn create_tick(price: &str, timestamp: DateTime<Utc>) -> TickData {
        TickData {
            timestamp,
            symbol: "BTCUSDT".to_string(),
            price: Decimal::from_str(price).unwrap(),
            quantity: Decimal::from_str("1.0").unwrap(),
            side: TradeSide::Buy,
            trade_id: "test".to_string(),
            is_buyer_maker: false,
        }
    }

    #[test]
    fn test_on_close_bar_time_based() {
        let gen = HistoricalOHLCGenerator::new(
            BarType::TimeBased(Timeframe::OneMinute),
            BarDataMode::OnCloseBar,
        );

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        let ticks = vec![
            create_tick("50000", base_time),
            create_tick("50100", base_time + Duration::seconds(30)),
            create_tick("50200", base_time + Duration::minutes(1)),
            create_tick("50300", base_time + Duration::minutes(1) + Duration::seconds(30)),
        ];

        let bars = gen.generate_from_ticks(&ticks);

        // Should generate 2 bars (one for each minute)
        assert_eq!(bars.len(), 2);

        // First bar
        assert_eq!(bars[0].current_tick, None);
        assert_eq!(bars[0].metadata.is_bar_closed, true);
        assert_eq!(bars[0].metadata.tick_count_in_bar, 2);

        // Second bar
        assert_eq!(bars[1].current_tick, None);
        assert_eq!(bars[1].metadata.is_bar_closed, true);
        assert_eq!(bars[1].metadata.tick_count_in_bar, 2);
    }

    #[test]
    fn test_on_each_tick_mode() {
        let gen = HistoricalOHLCGenerator::new(
            BarType::TimeBased(Timeframe::OneMinute),
            BarDataMode::OnEachTick,
        );

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        let ticks = vec![
            create_tick("50000", base_time),
            create_tick("50100", base_time + Duration::seconds(30)),
        ];

        let bars = gen.generate_from_ticks(&ticks);

        // Should generate 2 events (one per tick)
        assert_eq!(bars.len(), 2);

        // First tick
        assert!(bars[0].current_tick.is_some());
        assert_eq!(bars[0].metadata.is_first_tick_of_bar, true);
        assert_eq!(bars[0].metadata.tick_count_in_bar, 1);

        // Second tick
        assert!(bars[1].current_tick.is_some());
        assert_eq!(bars[1].metadata.is_first_tick_of_bar, false);
        assert_eq!(bars[1].metadata.tick_count_in_bar, 2);
    }

    #[test]
    fn test_on_price_move_mode() {
        let gen = HistoricalOHLCGenerator::new(
            BarType::TimeBased(Timeframe::OneMinute),
            BarDataMode::OnPriceMove,
        );

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        let ticks = vec![
            create_tick("50000", base_time),
            create_tick("50000", base_time + Duration::seconds(10)), // Same price
            create_tick("50100", base_time + Duration::seconds(20)), // Price change
            create_tick("50100", base_time + Duration::seconds(30)), // Same price
            create_tick("50200", base_time + Duration::seconds(40)), // Price change
        ];

        let bars = gen.generate_from_ticks(&ticks);

        // Should generate 3 events (only when price changes)
        assert_eq!(bars.len(), 3);
        assert!(bars[0].current_tick.is_some());
        assert_eq!(
            bars[0].current_tick.as_ref().unwrap().price,
            Decimal::from_str("50000").unwrap()
        );
        assert_eq!(
            bars[1].current_tick.as_ref().unwrap().price,
            Decimal::from_str("50100").unwrap()
        );
        assert_eq!(
            bars[2].current_tick.as_ref().unwrap().price,
            Decimal::from_str("50200").unwrap()
        );
    }

    #[test]
    fn test_tick_based_on_close_bar() {
        let gen = HistoricalOHLCGenerator::new(
            BarType::TickBased(3),
            BarDataMode::OnCloseBar,
        );

        let base_time = Utc::now();

        let ticks = vec![
            create_tick("50000", base_time),
            create_tick("50100", base_time + Duration::seconds(1)),
            create_tick("50200", base_time + Duration::seconds(2)),
            create_tick("50300", base_time + Duration::seconds(3)),
            create_tick("50400", base_time + Duration::seconds(4)),
        ];

        let bars = gen.generate_from_ticks(&ticks);

        // Should generate 2 bars (3 ticks + 2 ticks)
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].metadata.tick_count_in_bar, 3);
        assert_eq!(bars[0].metadata.is_bar_closed, true);
        assert_eq!(bars[1].metadata.tick_count_in_bar, 2);
        assert_eq!(bars[1].metadata.is_bar_closed, true);
    }

    #[test]
    fn test_tick_based_on_each_tick() {
        let gen = HistoricalOHLCGenerator::new(
            BarType::TickBased(2),
            BarDataMode::OnEachTick,
        );

        let base_time = Utc::now();

        let ticks = vec![
            create_tick("50000", base_time),
            create_tick("50100", base_time + Duration::seconds(1)),
            create_tick("50200", base_time + Duration::seconds(2)),
        ];

        let bars = gen.generate_from_ticks(&ticks);

        // Should generate 3 events (2 for first bar, 1 for second bar)
        assert_eq!(bars.len(), 3);

        // First bar events
        assert_eq!(bars[0].metadata.tick_count_in_bar, 1);
        assert_eq!(bars[0].metadata.is_first_tick_of_bar, true);
        assert_eq!(bars[0].metadata.is_bar_closed, false);

        assert_eq!(bars[1].metadata.tick_count_in_bar, 2);
        assert_eq!(bars[1].metadata.is_first_tick_of_bar, false);
        assert_eq!(bars[1].metadata.is_bar_closed, true); // Bar closes on 2nd tick

        // Second bar first event
        assert_eq!(bars[2].metadata.tick_count_in_bar, 1);
        assert_eq!(bars[2].metadata.is_first_tick_of_bar, true);
        assert_eq!(bars[2].metadata.is_bar_closed, false);
    }
}
