//! Bar data generation from ticks for backtesting.
//!
//! This module converts raw tick data into OHLC bars and [`BarData`] events
//! that strategies consume. It supports both time-based and tick-based bars
//! with session-aware generation for markets with trading hours.
//!
//! # Operational Modes
//!
//! The generator supports three modes that determine when [`BarData`] events are emitted:
//!
//! ## `OnEachTick` Mode
//! - **Event frequency**: One event per incoming tick
//! - **Use case**: High-frequency strategies that need to react to every price update
//! - **BarData contents**:
//!   - `current_tick`: The tick that triggered this event
//!   - `ohlc_bar`: Accumulated OHLC state (open from first tick, high/low updated, close = current)
//!   - `metadata.is_bar_closed`: `false` until the bar period ends
//! - **Example**: Scalping, market-making, tick-by-tick analysis
//!
//! ## `OnPriceMove` Mode
//! - **Event frequency**: One event per unique price (filters duplicate prices)
//! - **Use case**: Strategies that care about price changes, not volume/time
//! - **BarData contents**: Same as OnEachTick, but only when price differs from previous
//! - **Example**: Price action strategies, support/resistance detection
//!
//! ## `OnCloseBar` Mode
//! - **Event frequency**: One event per completed bar (e.g., every 1 minute)
//! - **Use case**: Traditional candle-based strategies
//! - **BarData contents**:
//!   - `current_tick`: `None` (bar close is timer-based, not tick-based)
//!   - `ohlc_bar`: Final OHLC values for the completed bar
//!   - `metadata.is_bar_closed`: Always `true`
//! - **Example**: Moving average crossover, RSI, most technical indicators
//!
//! # Bar Types
//!
//! ## Time-Based Bars (`BarType::TimeBased(Timeframe)`)
//! - Bars are closed when wall-clock time reaches the next interval boundary
//! - Supported timeframes: 1m, 5m, 15m, 1h, 4h, 1d, 1w
//! - Timestamps are aligned to interval boundaries (e.g., 9:30:00, 9:31:00, not 9:30:15)
//!
//! ## Tick-Based Bars (`BarType::TickBased(u32)`)
//! - Bars are closed after N ticks are received
//! - More consistent bar sizes in high-frequency scenarios
//! - Not affected by time gaps (e.g., overnight)
//!
//! # Session-Aware Generation
//!
//! For markets with trading hours (stocks, futures), the generator supports:
//!
//! - **Session open alignment**: First bar of each day aligned to session open time
//! - **Session close truncation**: Partial bars closed at session end
//! - **Gap handling**: Synthetic bars generated for gaps (OHLC = last price)
//!
//! # State Machine
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Bar State Machine                         │
//! │                                                              │
//! │  [No Bar] ──tick──► [Building] ──close──► [Completed]       │
//! │     ▲                   │  │                   │            │
//! │     │                   │  │                   │            │
//! │     └───session_close───┘  └──tick (same bar)─┘            │
//! │                                                              │
//! │  Close triggers:                                             │
//! │   - Time-based: timestamp >= bar_end                         │
//! │   - Tick-based: tick_count >= N                              │
//! │   - Session: timestamp >= session_close (if truncation on)   │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use trading_common::backtest::bar_generator::HistoricalOHLCGenerator;
//! use trading_common::data::types::{BarType, BarDataMode, Timeframe};
//!
//! // Create generator for 1-minute bars, emitting on bar close
//! let generator = HistoricalOHLCGenerator::new(
//!     BarType::TimeBased(Timeframe::OneMinute),
//!     BarDataMode::OnCloseBar,
//! );
//!
//! // Generate bar events from tick data
//! let bar_events = generator.generate_from_ticks(&ticks);
//! ```

use crate::data::types::{BarData, BarDataMode, BarType, OHLCData, TickData, Timeframe};
use crate::instruments::SessionSchedule;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use rust_decimal::Decimal;
use std::sync::Arc;

/// Configuration for session-aware bar generation
///
/// Controls how bars align to session boundaries (open/close times).
#[derive(Debug, Clone)]
pub struct SessionAwareConfig {
    /// Session schedule defining trading hours (None = 24/7 operation)
    pub session_schedule: Option<Arc<SessionSchedule>>,

    /// If true, align first bar of each session to session open time
    /// rather than first tick arrival time.
    ///
    /// Example: Session opens at 9:30 AM, first tick at 9:30:15 AM
    /// - align_to_session_open = true: First 1-min bar is 9:30-9:31
    /// - align_to_session_open = false: First 1-min bar is 9:30:15-9:31:15
    pub align_to_session_open: bool,

    /// If true, force-close partial bars when session closes.
    ///
    /// Example for tick-based bars:
    /// - 500-tick bars, session closes at tick 300
    /// - truncate_at_session_close = true: Close bar at tick 300
    /// - truncate_at_session_close = false: Continue bar to next session
    ///
    /// Example for time-based bars:
    /// - 1-minute bars, session closes at 15:59:30
    /// - truncate_at_session_close = true: Close 15:59-16:00 bar at 15:59:30
    /// - truncate_at_session_close = false: Let bar complete naturally
    pub truncate_at_session_close: bool,
}

impl Default for SessionAwareConfig {
    fn default() -> Self {
        Self {
            session_schedule: None,
            align_to_session_open: false,
            truncate_at_session_close: false,
        }
    }
}

impl SessionAwareConfig {
    /// Create config with session schedule
    pub fn with_session(session_schedule: Arc<SessionSchedule>) -> Self {
        Self {
            session_schedule: Some(session_schedule),
            align_to_session_open: true,
            truncate_at_session_close: true,
        }
    }

    /// Create config for 24/7 markets (no session boundaries)
    pub fn continuous() -> Self {
        Self::default()
    }

    /// Enable session open alignment
    pub fn with_session_open_alignment(mut self, enabled: bool) -> Self {
        self.align_to_session_open = enabled;
        self
    }

    /// Enable session close truncation
    pub fn with_session_close_truncation(mut self, enabled: bool) -> Self {
        self.truncate_at_session_close = enabled;
        self
    }

    /// Get session open time for a given date (in UTC)
    pub fn get_session_open_utc(&self, date: NaiveDate) -> Option<DateTime<Utc>> {
        let schedule = self.session_schedule.as_ref()?;
        let open_time = schedule.session_open_time(date)?;

        // Convert local time to UTC
        let local_dt = date.and_time(open_time);
        let tz = self.get_timezone()?;
        let local_with_tz = tz.from_local_datetime(&local_dt).single()?;
        Some(local_with_tz.with_timezone(&Utc))
    }

    /// Get session close time for a given date (in UTC)
    pub fn get_session_close_utc(&self, date: NaiveDate) -> Option<DateTime<Utc>> {
        let schedule = self.session_schedule.as_ref()?;
        let close_time = schedule.session_close_time(date)?;

        // Convert local time to UTC
        let local_dt = date.and_time(close_time);
        let tz = self.get_timezone()?;
        let local_with_tz = tz.from_local_datetime(&local_dt).single()?;
        Some(local_with_tz.with_timezone(&Utc))
    }

    /// Check if a given UTC time is within a trading session
    pub fn is_within_session(&self, utc_time: DateTime<Utc>) -> bool {
        match &self.session_schedule {
            Some(schedule) => schedule.is_open(utc_time),
            None => true, // 24/7 - always open
        }
    }

    /// Get the next session close time after the given UTC time
    pub fn next_session_close(&self, utc_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let schedule = self.session_schedule.as_ref()?;
        let tz = schedule.timezone;
        let local_time = utc_time.with_timezone(&tz);
        let date = local_time.date_naive();

        // First try today's session close
        if let Some(close_utc) = self.get_session_close_utc(date) {
            if close_utc > utc_time {
                return Some(close_utc);
            }
        }

        // Otherwise try tomorrow's session close
        let tomorrow = date.succ_opt()?;
        self.get_session_close_utc(tomorrow)
    }

    /// Get the timezone from the session schedule
    fn get_timezone(&self) -> Option<Tz> {
        self.session_schedule.as_ref().map(|s| s.timezone)
    }
}

/// Historical OHLC bar generator for backtesting
///
/// Converts historical tick data into BarData events based on the strategy's
/// preferred bar type and operational mode.
pub struct HistoricalOHLCGenerator {
    bar_type: BarType,
    mode: BarDataMode,
    session_config: SessionAwareConfig,
}

impl HistoricalOHLCGenerator {
    /// Create new historical bar generator
    pub fn new(bar_type: BarType, mode: BarDataMode) -> Self {
        HistoricalOHLCGenerator {
            bar_type,
            mode,
            session_config: SessionAwareConfig::default(),
        }
    }

    /// Create historical bar generator with session-aware configuration
    pub fn with_session_config(
        bar_type: BarType,
        mode: BarDataMode,
        session_config: SessionAwareConfig,
    ) -> Self {
        HistoricalOHLCGenerator {
            bar_type,
            mode,
            session_config,
        }
    }

    /// Set session configuration
    pub fn set_session_config(&mut self, config: SessionAwareConfig) {
        self.session_config = config;
    }

    /// Get session configuration reference
    pub fn session_config(&self) -> &SessionAwareConfig {
        &self.session_config
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

        let bars = match self.bar_type {
            BarType::TimeBased(timeframe) => self.generate_time_based_bars(ticks, timeframe),
            BarType::TickBased(tick_count) => self.generate_tick_based_bars(ticks, tick_count),
        };

        // Mark all bars as historical since this is the HistoricalOHLCGenerator
        bars.into_iter()
            .map(|bar| bar.with_historical(true))
            .collect()
    }

    /// Generate time-based bars from ticks
    ///
    /// Supports session-aware generation with:
    /// - Session open alignment: First bar aligned to session open time
    /// - Session close truncation: Partial bar closed at session end
    fn generate_time_based_bars(&self, ticks: &[TickData], timeframe: Timeframe) -> Vec<BarData> {
        let mut result = Vec::new();
        let mut current_window_start: Option<DateTime<Utc>> = None;
        let mut window_ticks: Vec<TickData> = Vec::new();
        let mut _last_price: Option<Decimal> = None; // Used for future synthetic bar generation
        let mut current_session_open: Option<DateTime<Utc>> = None;
        let mut is_first_bar_of_session = true;

        for tick in ticks {
            // Check for session transitions
            let in_session = self.session_config.is_within_session(tick.timestamp);

            // Handle session close truncation
            if self.session_config.truncate_at_session_close {
                if let Some(session_close) = self.get_current_session_close(tick.timestamp) {
                    // If we have ticks and session is closing, truncate the bar
                    if !window_ticks.is_empty()
                        && tick.timestamp >= session_close
                        && current_window_start.is_some()
                    {
                        // Force-close the current bar as session-truncated
                        result.extend(self.process_completed_window_with_session(
                            &window_ticks,
                            current_window_start.unwrap(),
                            timeframe,
                            true, // is_session_truncated
                            is_first_bar_of_session,
                        ));
                        _last_price = Some(window_ticks.last().unwrap().price);
                        window_ticks.clear();
                        current_window_start = None;
                        is_first_bar_of_session = true;
                        current_session_open = None;
                    }
                }
            }

            // Skip ticks outside session if we have session config
            if self.session_config.session_schedule.is_some() && !in_session {
                continue;
            }

            // Calculate window start - possibly aligned to session open
            let tick_window =
                if self.session_config.align_to_session_open && is_first_bar_of_session {
                    // Get session open time for alignment
                    if current_session_open.is_none() {
                        current_session_open = self.get_session_open_for_tick(tick.timestamp);
                    }

                    if let Some(session_open) = current_session_open {
                        // Align to session open for first bar
                        self.align_to_session_open_time(tick.timestamp, timeframe, session_open)
                    } else {
                        timeframe.align_timestamp(tick.timestamp)
                    }
                } else {
                    timeframe.align_timestamp(tick.timestamp)
                };

            // Check if we're starting a new window
            if current_window_start.is_none() || current_window_start.unwrap() != tick_window {
                // Close previous window if exists
                if let Some(window_start) = current_window_start {
                    if !window_ticks.is_empty() {
                        // Only mark as session-aligned if we have a schedule AND alignment is enabled
                        let actually_aligned = is_first_bar_of_session
                            && self.session_config.align_to_session_open
                            && self.session_config.session_schedule.is_some();

                        result.extend(self.process_completed_window_with_session(
                            &window_ticks,
                            window_start,
                            timeframe,
                            false, // is_session_truncated
                            actually_aligned,
                        ));
                        let price_for_synthetic = window_ticks.last().unwrap().price;
                        _last_price = Some(price_for_synthetic);
                        is_first_bar_of_session = false;

                        // Generate synthetic bars for any gaps (within session only)
                        let symbol = &window_ticks[0].symbol;
                        result.extend(self.generate_synthetic_bars_for_gap_session_aware(
                            window_start,
                            tick_window,
                            timeframe,
                            price_for_synthetic,
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
                // Check if final bar should be session-truncated
                let is_truncated = self.session_config.truncate_at_session_close
                    && self.session_config.session_schedule.is_some();

                // Only mark as session-aligned if we have a schedule AND alignment is enabled
                let actually_aligned = is_first_bar_of_session
                    && self.session_config.align_to_session_open
                    && self.session_config.session_schedule.is_some();

                result.extend(self.process_completed_window_with_session(
                    &window_ticks,
                    window_start,
                    timeframe,
                    is_truncated,
                    actually_aligned,
                ));
            }
        }

        result
    }

    /// Get session open time for a tick (considering the tick's date)
    fn get_session_open_for_tick(&self, tick_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let schedule = self.session_config.session_schedule.as_ref()?;
        let tz = schedule.timezone;
        let local_time = tick_time.with_timezone(&tz);
        let date = local_time.date_naive();
        self.session_config.get_session_open_utc(date)
    }

    /// Get current session close time
    fn get_current_session_close(&self, tick_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let schedule = self.session_config.session_schedule.as_ref()?;
        let tz = schedule.timezone;
        let local_time = tick_time.with_timezone(&tz);
        let date = local_time.date_naive();
        self.session_config.get_session_close_utc(date)
    }

    /// Align timestamp to session open time for the first bar
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

    /// Generate synthetic bars for gaps, respecting session boundaries
    fn generate_synthetic_bars_for_gap_session_aware(
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
            // Skip synthetic bars outside session if session-aware
            if self.session_config.session_schedule.is_some()
                && !self.session_config.is_within_session(gap_window_start)
            {
                gap_window_start = gap_window_start + duration;
                continue;
            }

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

    /// Process a completed time window with session awareness
    fn process_completed_window_with_session(
        &self,
        ticks: &[TickData],
        window_start: DateTime<Utc>,
        timeframe: Timeframe,
        is_session_truncated: bool,
        is_session_aligned: bool,
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
                    let partial_ohlc =
                        OHLCData::from_ticks(&accumulated_ticks, timeframe, window_start)
                            .expect("Should have OHLC from accumulated ticks");

                    // Session aligned only applies to first tick of first bar
                    let bar_is_session_aligned = is_session_aligned && idx == 0;

                    result.push(BarData::new_session_aware(
                        Some(tick.clone()),
                        partial_ohlc,
                        self.bar_type,
                        idx == 0, // First tick of bar
                        false,    // Bar not closed yet
                        accumulated_ticks.len() as u64,
                        false, // Not truncated while accumulating
                        bar_is_session_aligned,
                    ));
                }

                result
            }
            BarDataMode::OnPriceMove => {
                // Emit only when price changes
                let mut result = Vec::new();
                let mut accumulated_ticks = Vec::new();
                let mut last_price: Option<Decimal> = None;
                let mut first_emit = true;

                for (idx, tick) in ticks.iter().enumerate() {
                    accumulated_ticks.push(tick.clone());

                    // Check if price changed
                    let price_changed = last_price.map_or(true, |last| last != tick.price);

                    if price_changed {
                        let partial_ohlc =
                            OHLCData::from_ticks(&accumulated_ticks, timeframe, window_start)
                                .expect("Should have OHLC from accumulated ticks");

                        // Session aligned only applies to first emit
                        let bar_is_session_aligned = is_session_aligned && first_emit;
                        first_emit = false;

                        result.push(BarData::new_session_aware(
                            Some(tick.clone()),
                            partial_ohlc,
                            self.bar_type,
                            idx == 0, // First tick of bar
                            false,
                            accumulated_ticks.len() as u64,
                            false, // Not truncated while accumulating
                            bar_is_session_aligned,
                        ));

                        last_price = Some(tick.price);
                    }
                }

                result
            }
            BarDataMode::OnCloseBar => {
                // Emit only when bar is complete
                vec![BarData::new_session_aware(
                    None, // No current tick for closed bar
                    ohlc,
                    self.bar_type,
                    false,
                    true, // Bar is closed
                    ticks.len() as u64,
                    is_session_truncated,
                    is_session_aligned,
                )]
            }
        }
    }

    /// Generate tick-based bars from ticks
    ///
    /// Supports session-aware generation with:
    /// - Session close truncation: Partial bar closed at session end
    fn generate_tick_based_bars(&self, ticks: &[TickData], tick_count: u32) -> Vec<BarData> {
        // If no session config or no truncation, use simple chunking
        if !self.session_config.truncate_at_session_close
            || self.session_config.session_schedule.is_none()
        {
            return self.generate_tick_based_bars_simple(ticks, tick_count);
        }

        // Session-aware tick-based bar generation
        let mut result = Vec::new();
        let tick_count_usize = tick_count as usize;
        let mut current_bar_ticks: Vec<TickData> = Vec::new();
        let mut is_first_bar_of_session = true;
        let timeframe = Timeframe::OneMinute; // Placeholder for tick-based bars

        for tick in ticks {
            // Skip ticks outside session
            if !self.session_config.is_within_session(tick.timestamp) {
                // If we have accumulated ticks when leaving session, truncate the bar
                if !current_bar_ticks.is_empty() {
                    result.extend(self.emit_tick_bar(
                        &current_bar_ticks,
                        tick_count_usize,
                        timeframe,
                        true, // session_truncated
                        is_first_bar_of_session,
                    ));
                    current_bar_ticks.clear();
                    is_first_bar_of_session = true;
                }
                continue;
            }

            // Check if session is about to close
            if let Some(session_close) = self.get_current_session_close(tick.timestamp) {
                if tick.timestamp >= session_close && !current_bar_ticks.is_empty() {
                    // Force close current bar before session end
                    result.extend(self.emit_tick_bar(
                        &current_bar_ticks,
                        tick_count_usize,
                        timeframe,
                        true, // session_truncated
                        is_first_bar_of_session,
                    ));
                    current_bar_ticks.clear();
                    is_first_bar_of_session = true;
                }
            }

            current_bar_ticks.push(tick.clone());

            // Check if bar is complete (reached target tick count)
            if current_bar_ticks.len() >= tick_count_usize {
                result.extend(self.emit_tick_bar(
                    &current_bar_ticks,
                    tick_count_usize,
                    timeframe,
                    false, // not truncated - full bar
                    is_first_bar_of_session,
                ));
                current_bar_ticks.clear();
                is_first_bar_of_session = false;
            }
        }

        // Handle remaining ticks (partial bar at end of data)
        if !current_bar_ticks.is_empty() {
            let is_truncated = self.session_config.truncate_at_session_close;
            result.extend(self.emit_tick_bar(
                &current_bar_ticks,
                tick_count_usize,
                timeframe,
                is_truncated,
                is_first_bar_of_session,
            ));
        }

        result
    }

    /// Simple tick-based bar generation without session awareness
    fn generate_tick_based_bars_simple(&self, ticks: &[TickData], tick_count: u32) -> Vec<BarData> {
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

                        let partial_ohlc =
                            OHLCData::from_ticks(&accumulated_ticks, timeframe, window_start)
                                .expect("Should have OHLC from accumulated ticks");

                        let is_last_in_chunk = idx == chunk.len() - 1;
                        result.push(BarData::new(
                            Some(tick.clone()),
                            partial_ohlc,
                            self.bar_type,
                            idx == 0,                        // First tick of bar
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
                            let partial_ohlc =
                                OHLCData::from_ticks(&accumulated_ticks, timeframe, window_start)
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

    /// Emit a tick bar based on current mode
    fn emit_tick_bar(
        &self,
        ticks: &[TickData],
        target_tick_count: usize,
        timeframe: Timeframe,
        is_session_truncated: bool,
        is_session_aligned: bool,
    ) -> Vec<BarData> {
        if ticks.is_empty() {
            return Vec::new();
        }

        let window_start = ticks[0].timestamp;
        let is_full_bar = ticks.len() >= target_tick_count;

        match self.mode {
            BarDataMode::OnEachTick => {
                let mut result = Vec::new();
                let mut accumulated_ticks = Vec::new();

                for (idx, tick) in ticks.iter().enumerate() {
                    accumulated_ticks.push(tick.clone());

                    let partial_ohlc =
                        OHLCData::from_ticks(&accumulated_ticks, timeframe, window_start)
                            .expect("Should have OHLC from accumulated ticks");

                    let is_last = idx == ticks.len() - 1;
                    let is_closed = is_last && (is_full_bar || is_session_truncated);

                    result.push(BarData::new_session_aware(
                        Some(tick.clone()),
                        partial_ohlc,
                        self.bar_type,
                        idx == 0,
                        is_closed,
                        accumulated_ticks.len() as u64,
                        is_last && is_session_truncated && !is_full_bar,
                        is_session_aligned && idx == 0,
                    ));
                }

                result
            }
            BarDataMode::OnPriceMove => {
                let mut result = Vec::new();
                let mut accumulated_ticks = Vec::new();
                let mut last_price: Option<Decimal> = None;
                let mut first_emit = true;

                for (idx, tick) in ticks.iter().enumerate() {
                    accumulated_ticks.push(tick.clone());

                    let price_changed = last_price.map_or(true, |last| last != tick.price);

                    if price_changed {
                        let partial_ohlc =
                            OHLCData::from_ticks(&accumulated_ticks, timeframe, window_start)
                                .expect("Should have OHLC from accumulated ticks");

                        let is_last = idx == ticks.len() - 1;
                        let is_closed = is_last && (is_full_bar || is_session_truncated);

                        result.push(BarData::new_session_aware(
                            Some(tick.clone()),
                            partial_ohlc,
                            self.bar_type,
                            idx == 0,
                            is_closed,
                            accumulated_ticks.len() as u64,
                            is_last && is_session_truncated && !is_full_bar,
                            is_session_aligned && first_emit,
                        ));

                        last_price = Some(tick.price);
                        first_emit = false;
                    }
                }

                result
            }
            BarDataMode::OnCloseBar => {
                let ohlc = OHLCData::from_ticks(ticks, timeframe, window_start)
                    .expect("Should have OHLC from ticks");

                vec![BarData::new_session_aware(
                    None,
                    ohlc,
                    self.bar_type,
                    false,
                    true, // Bar is closed
                    ticks.len() as u64,
                    is_session_truncated && !is_full_bar,
                    is_session_aligned,
                )]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::TradeSide;
    use chrono::{Duration, Timelike};
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn create_tick(price: &str, timestamp: DateTime<Utc>) -> TickData {
        TickData::with_details(
            timestamp,
            timestamp,
            "BTCUSDT".to_string(),
            "TEST".to_string(),
            Decimal::from_str(price).unwrap(),
            Decimal::from_str("1.0").unwrap(),
            TradeSide::Buy,
            "TEST".to_string(),
            "test".to_string(),
            false,
            0,
        )
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
            create_tick(
                "50300",
                base_time + Duration::minutes(1) + Duration::seconds(30),
            ),
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
        let gen = HistoricalOHLCGenerator::new(BarType::TickBased(3), BarDataMode::OnCloseBar);

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
        let gen = HistoricalOHLCGenerator::new(BarType::TickBased(2), BarDataMode::OnEachTick);

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

    // =========================================================================
    // Session-Aware Bar Generation Tests
    // =========================================================================

    #[test]
    fn test_session_aware_config_default() {
        let config = SessionAwareConfig::default();
        assert!(config.session_schedule.is_none());
        assert!(!config.align_to_session_open);
        assert!(!config.truncate_at_session_close);
    }

    #[test]
    fn test_session_aware_config_continuous() {
        let config = SessionAwareConfig::continuous();
        assert!(config.session_schedule.is_none());
        // Continuous markets are always "in session"
        assert!(config.is_within_session(Utc::now()));
    }

    #[test]
    fn test_session_aware_config_builder() {
        let config = SessionAwareConfig::default()
            .with_session_open_alignment(true)
            .with_session_close_truncation(true);

        assert!(config.align_to_session_open);
        assert!(config.truncate_at_session_close);
    }

    #[test]
    fn test_generator_without_session_config() {
        // When no session config is set, generator should work as before
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
        ];

        let bars = gen.generate_from_ticks(&ticks);

        assert_eq!(bars.len(), 1);
        assert!(!bars[0].metadata.is_session_truncated);
        assert!(!bars[0].metadata.is_session_aligned);
    }

    #[test]
    fn test_tick_based_bars_session_truncation_logic() {
        // Test that partial bars can be marked as session-truncated
        // Note: Without an actual session schedule, truncation flag will NOT be set
        // because we can't determine session boundaries without a schedule
        let config = SessionAwareConfig::default().with_session_close_truncation(true);

        let mut gen = HistoricalOHLCGenerator::new(
            BarType::TickBased(5), // 5-tick bars
            BarDataMode::OnCloseBar,
        );
        gen.set_session_config(config);

        let base_time = Utc::now();

        // Only 3 ticks - partial bar
        let ticks = vec![
            create_tick("50000", base_time),
            create_tick("50100", base_time + Duration::seconds(1)),
            create_tick("50200", base_time + Duration::seconds(2)),
        ];

        let bars = gen.generate_from_ticks(&ticks);

        // Should generate 1 bar (partial)
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].metadata.tick_count_in_bar, 3);
        assert!(bars[0].metadata.is_bar_closed);
        // Without a session schedule, the truncation flag is NOT set
        // (we only mark as truncated when truncating at actual session close)
        assert!(!bars[0].metadata.is_session_truncated);
    }

    #[test]
    fn test_time_based_bars_default_session_flags() {
        // Verify session flags are properly set to false by default
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
        ];

        let bars = gen.generate_from_ticks(&ticks);

        assert_eq!(bars.len(), 1);
        assert!(!bars[0].metadata.is_session_truncated);
        assert!(!bars[0].metadata.is_session_aligned);
    }

    #[test]
    fn test_session_config_with_schedule_builder() {
        use crate::instruments::session::presets;

        // Create with US equity session schedule
        let schedule = Arc::new(presets::us_equity());
        let config = SessionAwareConfig::with_session(schedule.clone());

        assert!(config.session_schedule.is_some());
        assert!(config.align_to_session_open);
        assert!(config.truncate_at_session_close);
    }

    #[test]
    fn test_generator_set_session_config() {
        let mut gen = HistoricalOHLCGenerator::new(
            BarType::TimeBased(Timeframe::OneMinute),
            BarDataMode::OnCloseBar,
        );

        // Initially no session config
        assert!(gen.session_config().session_schedule.is_none());

        // Set new config
        let config = SessionAwareConfig::default()
            .with_session_open_alignment(true)
            .with_session_close_truncation(true);
        gen.set_session_config(config);

        assert!(gen.session_config().align_to_session_open);
        assert!(gen.session_config().truncate_at_session_close);
    }

    #[test]
    fn test_tick_based_full_bars_not_truncated() {
        // Full bars should not be marked as truncated
        let config = SessionAwareConfig::default().with_session_close_truncation(true);

        let mut gen = HistoricalOHLCGenerator::new(
            BarType::TickBased(2), // 2-tick bars
            BarDataMode::OnCloseBar,
        );
        gen.set_session_config(config);

        let base_time = Utc::now();

        // 4 ticks = 2 full bars
        let ticks = vec![
            create_tick("50000", base_time),
            create_tick("50100", base_time + Duration::seconds(1)),
            create_tick("50200", base_time + Duration::seconds(2)),
            create_tick("50300", base_time + Duration::seconds(3)),
        ];

        let bars = gen.generate_from_ticks(&ticks);

        // Should generate 2 full bars
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].metadata.tick_count_in_bar, 2);
        assert!(!bars[0].metadata.is_session_truncated); // Full bar
        assert_eq!(bars[1].metadata.tick_count_in_bar, 2);
        assert!(!bars[1].metadata.is_session_truncated); // Full bar
    }

    #[test]
    fn test_session_aware_config_is_within_session_no_schedule() {
        // Without a schedule, always returns true (24/7)
        let config = SessionAwareConfig::default();
        assert!(config.is_within_session(Utc::now()));
    }
}
