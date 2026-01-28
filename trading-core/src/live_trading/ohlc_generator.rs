use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use trading_common::backtest::SessionAwareConfig;
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
    /// Flag indicating this bar was aligned to session open time
    is_session_aligned: bool,
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
            is_session_aligned: false,
        }
    }

    /// Create new bar builder for time-based bars with session alignment flag
    fn new_time_based_session_aware(
        symbol: String,
        timeframe: Timeframe,
        start_time: DateTime<Utc>,
        is_session_aligned: bool,
    ) -> Self {
        let mut bar = Self::new_time_based(symbol, timeframe, start_time);
        bar.is_session_aligned = is_session_aligned;
        bar
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
            is_session_aligned: false,
        }
    }

    /// Create new bar builder for tick-based bars with session alignment flag
    fn new_tick_based_session_aware(
        symbol: String,
        tick_count_target: u32,
        first_tick: &TickData,
        is_session_aligned: bool,
    ) -> Self {
        let mut bar = Self::new_tick_based(symbol, tick_count_target, first_tick);
        bar.is_session_aligned = is_session_aligned;
        bar
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
///
/// Session-aware features:
/// - Align first bar of each session to session open time
/// - Force-close partial bars at session close
pub struct RealtimeOHLCGenerator {
    current_bar: Option<BarBuilder>,
    bar_type: BarType,
    last_price: Option<Decimal>,
    symbol: String,
    /// Session configuration for session-aware bar generation
    session_config: SessionAwareConfig,
    /// Flag indicating whether this is the first bar of the current session
    is_first_bar_of_session: bool,
    /// Cached session open time for the current session
    current_session_open: Option<DateTime<Utc>>,
    /// Cached session close time for the current session
    current_session_close: Option<DateTime<Utc>>,
}

impl RealtimeOHLCGenerator {
    /// Create new generator for time-based bars
    pub fn new_time_based(symbol: String, timeframe: Timeframe) -> Self {
        RealtimeOHLCGenerator {
            current_bar: None,
            bar_type: BarType::TimeBased(timeframe),
            last_price: None,
            symbol,
            session_config: SessionAwareConfig::default(),
            is_first_bar_of_session: true,
            current_session_open: None,
            current_session_close: None,
        }
    }

    /// Create new generator for tick-based bars
    pub fn new_tick_based(symbol: String, tick_count: u32) -> Self {
        RealtimeOHLCGenerator {
            current_bar: None,
            bar_type: BarType::TickBased(tick_count),
            last_price: None,
            symbol,
            session_config: SessionAwareConfig::default(),
            is_first_bar_of_session: true,
            current_session_open: None,
            current_session_close: None,
        }
    }

    /// Create generator from BarType
    pub fn new(symbol: String, bar_type: BarType) -> Self {
        match bar_type {
            BarType::TimeBased(tf) => Self::new_time_based(symbol, tf),
            BarType::TickBased(count) => Self::new_tick_based(symbol, count),
        }
    }

    /// Create generator with session-aware configuration
    ///
    /// Enables session open alignment and session close truncation based on
    /// the provided session configuration.
    #[allow(dead_code)]
    pub fn with_session_config(
        symbol: String,
        bar_type: BarType,
        session_config: SessionAwareConfig,
    ) -> Self {
        RealtimeOHLCGenerator {
            current_bar: None,
            bar_type,
            last_price: None,
            symbol,
            session_config,
            is_first_bar_of_session: true,
            current_session_open: None,
            current_session_close: None,
        }
    }

    /// Set session configuration after creation
    #[allow(dead_code)]
    pub fn set_session_config(&mut self, config: SessionAwareConfig) {
        self.session_config = config;
        // Reset session state when config changes
        self.is_first_bar_of_session = true;
        self.current_session_open = None;
        self.current_session_close = None;
    }

    /// Get session configuration reference
    #[allow(dead_code)]
    pub fn session_config(&self) -> &SessionAwareConfig {
        &self.session_config
    }

    /// Notify that a new session has started
    ///
    /// Call this when the trading session opens to reset session state.
    /// This enables proper alignment of the first bar to the session open time.
    #[allow(dead_code)]
    pub fn on_session_start(&mut self, session_open: DateTime<Utc>) {
        self.is_first_bar_of_session = true;
        self.current_session_open = Some(session_open);
        self.current_session_close = None; // Will be calculated on next tick
    }

    /// Get session open time for a tick's date
    fn get_session_open_for_tick(&self, tick_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let schedule = self.session_config.session_schedule.as_ref()?;
        let local_time = tick_time.with_timezone(&schedule.timezone);
        let date = local_time.date_naive();
        self.session_config.get_session_open_utc(date)
    }

    /// Get current session close time
    fn get_current_session_close(&mut self, tick_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
        if self.current_session_close.is_none() {
            self.current_session_close = self.session_config.next_session_close(tick_time);
        }
        self.current_session_close
    }

    /// Align timestamp to session open boundary
    ///
    /// For first bar of session, aligns to session open time rather than
    /// standard timeframe alignment.
    fn align_to_session_open_time(
        &self,
        tick_time: DateTime<Utc>,
        timeframe: Timeframe,
        session_open: DateTime<Utc>,
    ) -> DateTime<Utc> {
        // If tick is before or at session open, use session open
        if tick_time <= session_open {
            return session_open;
        }

        // Otherwise, calculate which bar period contains this tick,
        // starting from session open
        let duration = timeframe.as_duration();
        let elapsed = tick_time.signed_duration_since(session_open);
        let periods = elapsed.num_milliseconds() / duration.num_milliseconds();
        session_open + duration * (periods as i32)
    }

    /// Reset session state (for use after session close)
    fn reset_session_state(&mut self) {
        self.is_first_bar_of_session = true;
        self.current_session_open = None;
        self.current_session_close = None;
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

    /// Process tick for time-based bars (session-aware)
    ///
    /// Handles:
    /// - Session close truncation: Force-close partial bar at session end
    /// - Session open alignment: Align first bar of session to session open time
    /// - Proper session metadata flags on emitted BarData
    fn process_tick_time_based(&mut self, tick: &TickData, timeframe: Timeframe) -> Vec<BarData> {
        let mut result = Vec::new();

        // Check for session close truncation BEFORE normal processing
        if self.session_config.truncate_at_session_close {
            if let Some(session_close) = self.get_current_session_close(tick.timestamp) {
                if tick.timestamp >= session_close {
                    if let Some(ref bar) = self.current_bar {
                        if bar.tick_count > 0 {
                            // Force-close current bar as session-truncated
                            let closed_ohlc = bar.build();
                            result.push(BarData::new_session_aware(
                                None,
                                closed_ohlc,
                                self.bar_type,
                                false,
                                true, // is_bar_closed
                                bar.tick_count,
                                true, // is_session_truncated
                                bar.is_session_aligned,
                            ));
                            self.current_bar = None;
                            self.reset_session_state();
                        }
                    }
                }
            }
        }

        // Calculate window start with potential session alignment
        let (aligned_start, is_session_aligned) =
            if self.session_config.align_to_session_open && self.is_first_bar_of_session {
                // Get session open for this tick's date
                if self.current_session_open.is_none() {
                    self.current_session_open = self.get_session_open_for_tick(tick.timestamp);
                }

                if let Some(session_open) = self.current_session_open {
                    // Check if session schedule exists for proper alignment
                    let aligned = self.session_config.session_schedule.is_some();
                    (
                        self.align_to_session_open_time(tick.timestamp, timeframe, session_open),
                        aligned,
                    )
                } else {
                    (timeframe.align_timestamp(tick.timestamp), false)
                }
            } else {
                (timeframe.align_timestamp(tick.timestamp), false)
            };

        if let Some(ref mut bar) = self.current_bar {
            // Check if we need to close current bar
            if bar.should_close_time_based(tick.timestamp) {
                // Close current bar
                let closed_ohlc = bar.build();
                result.push(BarData::new_session_aware(
                    None, // No current tick for closed bar
                    closed_ohlc,
                    self.bar_type,
                    false,
                    true, // Bar is closed
                    bar.tick_count,
                    false, // Not session truncated (normal close)
                    bar.is_session_aligned,
                ));

                // Mark that first bar of session is done
                self.is_first_bar_of_session = false;

                // Start new bar
                let mut new_bar = BarBuilder::new_time_based_session_aware(
                    self.symbol.clone(),
                    timeframe,
                    aligned_start,
                    false, // Subsequent bars are not session-aligned
                );
                new_bar.add_tick(tick);

                // Emit first tick of new bar
                let current_ohlc = new_bar.build();
                result.push(BarData::new_session_aware(
                    Some(tick.clone()),
                    current_ohlc,
                    self.bar_type,
                    true, // First tick of bar
                    false,
                    new_bar.tick_count,
                    false,
                    new_bar.is_session_aligned,
                ));

                self.current_bar = Some(new_bar);
            } else {
                // Add to current bar
                let is_first = bar.tick_count == 0;
                bar.add_tick(tick);
                let current_ohlc = bar.build();
                result.push(BarData::new_session_aware(
                    Some(tick.clone()),
                    current_ohlc,
                    self.bar_type,
                    is_first,
                    false,
                    bar.tick_count,
                    false,
                    bar.is_session_aligned,
                ));
            }
        } else {
            // First tick ever (or after session reset) - start new bar
            let mut new_bar = BarBuilder::new_time_based_session_aware(
                self.symbol.clone(),
                timeframe,
                aligned_start,
                is_session_aligned,
            );
            new_bar.add_tick(tick);

            let current_ohlc = new_bar.build();
            result.push(BarData::new_session_aware(
                Some(tick.clone()),
                current_ohlc,
                self.bar_type,
                true, // First tick of bar
                false,
                new_bar.tick_count,
                false,
                new_bar.is_session_aligned,
            ));

            self.current_bar = Some(new_bar);
        }

        result
    }

    /// Process tick for tick-based bars (session-aware)
    ///
    /// Handles session close truncation for tick-based bars.
    fn process_tick_tick_based(&mut self, tick: &TickData, target_tick_count: u32) -> Vec<BarData> {
        let mut result = Vec::new();

        // Check for session close truncation BEFORE normal processing
        if self.session_config.truncate_at_session_close {
            if let Some(session_close) = self.get_current_session_close(tick.timestamp) {
                if tick.timestamp >= session_close {
                    if let Some(ref bar) = self.current_bar {
                        if bar.tick_count > 0 {
                            // Force-close current bar as session-truncated
                            let closed_ohlc = bar.build();
                            result.push(BarData::new_session_aware(
                                None,
                                closed_ohlc,
                                self.bar_type,
                                false,
                                true, // is_bar_closed
                                bar.tick_count,
                                true, // is_session_truncated
                                bar.is_session_aligned,
                            ));
                            self.current_bar = None;
                            self.reset_session_state();
                        }
                    }
                }
            }
        }

        // Determine session alignment for first bar
        let is_first_bar = self.current_bar.is_none() && self.is_first_bar_of_session;
        let is_session_aligned = is_first_bar && self.session_config.session_schedule.is_some();

        if let Some(ref mut bar) = self.current_bar {
            // Check if we need to close current bar
            if bar.should_close_tick_based(target_tick_count) {
                // Close current bar
                let closed_ohlc = bar.build();
                result.push(BarData::new_session_aware(
                    None, // No current tick for closed bar
                    closed_ohlc,
                    self.bar_type,
                    false,
                    true, // Bar is closed
                    bar.tick_count,
                    false, // Not session truncated (normal close)
                    bar.is_session_aligned,
                ));

                // Mark that first bar of session is done
                self.is_first_bar_of_session = false;

                // Start new bar with this tick
                let new_bar = BarBuilder::new_tick_based_session_aware(
                    self.symbol.clone(),
                    target_tick_count,
                    tick,
                    false, // Subsequent bars are not session-aligned
                );
                let current_ohlc = new_bar.build();
                result.push(BarData::new_session_aware(
                    Some(tick.clone()),
                    current_ohlc,
                    self.bar_type,
                    true, // First tick of new bar
                    false,
                    new_bar.tick_count,
                    false,
                    new_bar.is_session_aligned,
                ));

                self.current_bar = Some(new_bar);
            } else {
                // Add to current bar
                let is_first = bar.tick_count == 0;
                bar.add_tick(tick);
                let current_ohlc = bar.build();
                result.push(BarData::new_session_aware(
                    Some(tick.clone()),
                    current_ohlc,
                    self.bar_type,
                    is_first,
                    false,
                    bar.tick_count,
                    false,
                    bar.is_session_aligned,
                ));
            }
        } else {
            // First tick ever (or after session reset) - start new bar
            let new_bar = BarBuilder::new_tick_based_session_aware(
                self.symbol.clone(),
                target_tick_count,
                tick,
                is_session_aligned,
            );
            let current_ohlc = new_bar.build();
            result.push(BarData::new_session_aware(
                Some(tick.clone()),
                current_ohlc,
                self.bar_type,
                true, // First tick of bar
                false,
                new_bar.tick_count,
                false,
                new_bar.is_session_aligned,
            ));

            self.current_bar = Some(new_bar);
        }

        result
    }

    /// Check if timer should close the current bar (for time-based bars)
    ///
    /// Called periodically (e.g., every 1 second) to check if bar should close
    /// even if no ticks received. Also handles session close truncation.
    pub fn check_timer_close(&mut self, now: DateTime<Utc>) -> Option<BarData> {
        // Check session close FIRST
        if self.session_config.truncate_at_session_close {
            if let Some(session_close) = self.current_session_close {
                if now >= session_close {
                    if let Some(ref bar) = self.current_bar {
                        if bar.tick_count > 0 {
                            let closed_ohlc = bar.build();
                            let bar_data = BarData::new_session_aware(
                                None,
                                closed_ohlc,
                                self.bar_type,
                                false,
                                true, // is_bar_closed
                                bar.tick_count,
                                true, // is_session_truncated
                                bar.is_session_aligned,
                            );
                            self.current_bar = None;
                            self.reset_session_state();
                            return Some(bar_data);
                        }
                    }
                }
            }
        }

        // Then check normal timer close
        if let BarType::TimeBased(_) = self.bar_type {
            if let Some(ref bar) = self.current_bar {
                if now >= bar.end_time && bar.tick_count > 0 {
                    // Close the bar
                    let closed_ohlc = bar.build();
                    let is_session_aligned = bar.is_session_aligned;
                    let bar_data = BarData::new_session_aware(
                        None,
                        closed_ohlc,
                        self.bar_type,
                        false,
                        true, // is_bar_closed
                        bar.tick_count,
                        false, // Not session truncated (normal close)
                        is_session_aligned,
                    );

                    // Mark that first bar of session is done
                    self.is_first_bar_of_session = false;

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
    use chrono::{Duration, Timelike};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn create_tick(price: &str, timestamp: DateTime<Utc>) -> TickData {
        TickData::with_details(
            timestamp,
            timestamp,
            "BTCUSD".to_string(),
            "TEST".to_string(),
            Decimal::from_str(price).unwrap(),
            Decimal::from_str("1.0").unwrap(),
            trading_common::data::types::TradeSide::Buy,
            "TEST".to_string(),
            "test".to_string(),
            false,
            0,
        )
    }

    #[test]
    fn test_time_based_single_bar() {
        let mut gen =
            RealtimeOHLCGenerator::new_time_based("BTCUSD".to_string(), Timeframe::OneMinute);

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
        let mut gen =
            RealtimeOHLCGenerator::new_time_based("BTCUSD".to_string(), Timeframe::OneMinute);

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
        let mut gen = RealtimeOHLCGenerator::new_tick_based("BTCUSD".to_string(), 3);

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
        let mut gen =
            RealtimeOHLCGenerator::new_time_based("BTCUSD".to_string(), Timeframe::OneMinute);

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
        let mut gen =
            RealtimeOHLCGenerator::new_time_based("BTCUSD".to_string(), Timeframe::OneMinute);

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

    // =========================================================================
    // Session-Aware Bar Generation Tests
    // =========================================================================

    #[test]
    fn test_session_config_default() {
        let gen =
            RealtimeOHLCGenerator::new_time_based("BTCUSD".to_string(), Timeframe::OneMinute);

        // Default config should be 24/7 (no session schedule)
        assert!(gen.session_config().session_schedule.is_none());
        assert!(!gen.session_config().align_to_session_open);
        assert!(!gen.session_config().truncate_at_session_close);
    }

    #[test]
    fn test_with_session_config_constructor() {
        let config = SessionAwareConfig::default()
            .with_session_open_alignment(true)
            .with_session_close_truncation(true);

        let gen = RealtimeOHLCGenerator::with_session_config(
            "BTCUSD".to_string(),
            BarType::TimeBased(Timeframe::OneMinute),
            config,
        );

        assert!(gen.session_config().align_to_session_open);
        assert!(gen.session_config().truncate_at_session_close);
    }

    #[test]
    fn test_set_session_config() {
        let mut gen =
            RealtimeOHLCGenerator::new_time_based("BTCUSD".to_string(), Timeframe::OneMinute);

        // Initially default config
        assert!(!gen.session_config().align_to_session_open);

        // Set new config
        let config = SessionAwareConfig::default()
            .with_session_open_alignment(true)
            .with_session_close_truncation(true);
        gen.set_session_config(config);

        assert!(gen.session_config().align_to_session_open);
        assert!(gen.session_config().truncate_at_session_close);
    }

    #[test]
    fn test_no_session_config_flags_false() {
        // Without session config, session flags should be false
        let mut gen =
            RealtimeOHLCGenerator::new_time_based("BTCUSD".to_string(), Timeframe::OneMinute);

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        let tick = create_tick("50000", base_time);
        let result = gen.process_tick(&tick);

        assert_eq!(result.len(), 1);
        assert!(!result[0].metadata.is_session_truncated);
        assert!(!result[0].metadata.is_session_aligned);
    }

    #[test]
    fn test_continuous_market_no_session() {
        // 24/7 markets should work correctly with session config set to continuous
        let config = SessionAwareConfig::continuous();
        let mut gen = RealtimeOHLCGenerator::with_session_config(
            "BTCUSD".to_string(),
            BarType::TimeBased(Timeframe::OneMinute),
            config,
        );

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        let tick = create_tick("50000", base_time);
        let result = gen.process_tick(&tick);

        assert_eq!(result.len(), 1);
        assert!(!result[0].metadata.is_session_truncated);
        // Without a session schedule, alignment flag should be false
        assert!(!result[0].metadata.is_session_aligned);
    }

    #[test]
    fn test_on_session_start_resets_state() {
        let mut gen =
            RealtimeOHLCGenerator::new_time_based("BTCUSD".to_string(), Timeframe::OneMinute);

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        // Process some ticks first
        let tick1 = create_tick("50000", base_time);
        gen.process_tick(&tick1);

        // Simulate session start
        let session_open = base_time + Duration::hours(1);
        gen.on_session_start(session_open);

        // Internal state should be reset
        assert!(gen.is_first_bar_of_session);
        assert_eq!(gen.current_session_open, Some(session_open));
        assert!(gen.current_session_close.is_none());
    }

    #[test]
    fn test_tick_based_session_flags_default() {
        // Tick-based bars should also have session flags defaulting to false
        let mut gen = RealtimeOHLCGenerator::new_tick_based("BTCUSD".to_string(), 3);

        let base_time = Utc::now();

        let tick = create_tick("50000", base_time);
        let result = gen.process_tick(&tick);

        assert_eq!(result.len(), 1);
        assert!(!result[0].metadata.is_session_truncated);
        assert!(!result[0].metadata.is_session_aligned);
    }

    #[test]
    fn test_timer_close_session_flags_propagated() {
        let mut gen =
            RealtimeOHLCGenerator::new_time_based("BTCUSD".to_string(), Timeframe::OneMinute);

        let base_time = Utc::now()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        // Add tick to start bar
        let tick = create_tick("50000", base_time);
        gen.process_tick(&tick);

        // Check timer close
        let after_bar_end = base_time + Duration::minutes(1) + Duration::seconds(5);
        let closed = gen.check_timer_close(after_bar_end);

        assert!(closed.is_some());
        let bar = closed.unwrap();
        // Normal close - should not be session truncated
        assert!(!bar.metadata.is_session_truncated);
    }

    #[test]
    fn test_session_config_builder_chain() {
        // Test builder pattern chaining
        let config = SessionAwareConfig::default()
            .with_session_open_alignment(true)
            .with_session_close_truncation(true);

        assert!(config.align_to_session_open);
        assert!(config.truncate_at_session_close);
        assert!(config.session_schedule.is_none());
    }

    #[test]
    fn test_bar_close_subsequent_bars_not_session_aligned() {
        // After first bar closes, subsequent bars should not be session-aligned
        let mut gen =
            RealtimeOHLCGenerator::new_time_based("BTCUSD".to_string(), Timeframe::OneMinute);

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

        // First result is closed bar
        assert_eq!(result.len(), 2);
        assert!(!result[0].metadata.is_session_aligned); // Closed bar

        // Second result is first tick of new bar
        assert!(!result[1].metadata.is_session_aligned); // Without session schedule
    }

    #[test]
    fn test_tick_based_bar_close_subsequent_bars_not_session_aligned() {
        let mut gen = RealtimeOHLCGenerator::new_tick_based("BTCUSD".to_string(), 2);

        let base_time = Utc::now();

        // Process first bar (2 ticks)
        let tick1 = create_tick("50000", base_time);
        gen.process_tick(&tick1);
        let tick2 = create_tick("50100", base_time + Duration::seconds(1));
        gen.process_tick(&tick2);

        // Third tick starts new bar
        let tick3 = create_tick("50200", base_time + Duration::seconds(2));
        let result = gen.process_tick(&tick3);

        // Should get 2 events: closed bar + first tick of new bar
        assert_eq!(result.len(), 2);
        assert!(!result[0].metadata.is_session_aligned);
        assert!(!result[1].metadata.is_session_aligned);
    }
}
