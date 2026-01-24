//! Data gap detection algorithm
//!
//! This module provides utilities to detect gaps in tick data coverage.
//! Gaps are periods where data is expected but not present.
//!
//! # Session-Aware Gap Detection
//!
//! The gap detector can use `SessionSchedule` from the instruments module to
//! determine trading hours, avoiding false positives during market closures.
//!
//! ```ignore
//! use trading_common::data::gap_detection::{GapDetector, GapDetectionConfig};
//! use trading_common::instruments::SessionSchedule;
//!
//! // Create session schedule for US equities
//! let schedule = SessionSchedule::us_equity();
//!
//! // Configure gap detection with session awareness
//! let config = GapDetectionConfig {
//!     session_schedule: Some(schedule),
//!     ..Default::default()
//! };
//!
//! let detector = GapDetector::new(config);
//! ```

use chrono::{DateTime, Datelike, Duration, Timelike, Utc, Weekday};
use std::collections::HashMap;

use super::backfill::DataGap;
use super::types::TickData;
use crate::instruments::SessionSchedule;

/// Configuration for gap detection
#[derive(Debug, Clone)]
pub struct GapDetectionConfig {
    /// Minimum gap duration to consider (minutes)
    pub min_gap_minutes: i64,
    /// Expected average ticks per minute (for estimation)
    pub expected_ticks_per_minute: f64,
    /// Legacy trading hours for futures (UTC)
    /// CME Globex: Sunday 5pm - Friday 4pm CT (Sunday 23:00 - Friday 22:00 UTC)
    /// Deprecated: prefer using `session_schedule` instead
    pub trading_hours: Option<TradingHours>,
    /// Session schedule for session-aware gap detection
    /// Takes precedence over `trading_hours` if both are set
    pub session_schedule: Option<SessionSchedule>,
}

impl Default for GapDetectionConfig {
    fn default() -> Self {
        Self {
            min_gap_minutes: 5,
            expected_ticks_per_minute: 100.0, // Rough estimate
            trading_hours: Some(TradingHours::cme_globex()),
            session_schedule: None,
        }
    }
}

impl GapDetectionConfig {
    /// Create config with session schedule (preferred method)
    pub fn with_session_schedule(schedule: SessionSchedule) -> Self {
        Self {
            min_gap_minutes: 5,
            expected_ticks_per_minute: 100.0,
            trading_hours: None, // Prefer session schedule
            session_schedule: Some(schedule),
        }
    }

    /// Create config for 24/7 trading (crypto)
    pub fn crypto_24_7() -> Self {
        Self {
            min_gap_minutes: 5,
            expected_ticks_per_minute: 100.0,
            trading_hours: Some(TradingHours::crypto_24_7()),
            session_schedule: None,
        }
    }

    /// Create config with no trading hour filters
    pub fn no_trading_hours() -> Self {
        Self {
            min_gap_minutes: 5,
            expected_ticks_per_minute: 100.0,
            trading_hours: None,
            session_schedule: None,
        }
    }
}

/// Trading hours specification
#[derive(Debug, Clone)]
pub struct TradingHours {
    /// Days when trading is active
    pub active_days: Vec<Weekday>,
    /// Start hour (UTC)
    pub start_hour: u32,
    /// Start minute
    pub start_minute: u32,
    /// End hour (UTC)
    pub end_hour: u32,
    /// End minute
    pub end_minute: u32,
    /// Maintenance windows (daily breaks)
    pub maintenance_windows: Vec<(u32, u32, u32, u32)>, // (start_hour, start_min, end_hour, end_min)
}

impl TradingHours {
    /// CME Globex trading hours (nearly 24 hours with maintenance break)
    /// Sunday 5pm CT to Friday 4pm CT with daily 1-hour maintenance 4-5pm CT
    pub fn cme_globex() -> Self {
        Self {
            active_days: vec![
                Weekday::Sun,
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
            ],
            // CME Globex opens Sunday 5pm CT (23:00 UTC)
            start_hour: 23,
            start_minute: 0,
            // CME Globex closes Friday 4pm CT (22:00 UTC)
            end_hour: 22,
            end_minute: 0,
            // Daily maintenance 4-5pm CT (22:00-23:00 UTC) except Sunday opening
            maintenance_windows: vec![(22, 0, 23, 0)],
        }
    }

    /// 24/7 crypto trading (no off hours)
    pub fn crypto_24_7() -> Self {
        Self {
            active_days: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
                Weekday::Sat,
                Weekday::Sun,
            ],
            start_hour: 0,
            start_minute: 0,
            end_hour: 23,
            end_minute: 59,
            maintenance_windows: vec![],
        }
    }

    /// Check if a given time is within trading hours
    pub fn is_trading_time(&self, dt: DateTime<Utc>) -> bool {
        let weekday = dt.weekday();

        // Check if day is active
        if !self.active_days.contains(&weekday) {
            return false;
        }

        let hour = dt.hour();
        let minute = dt.minute();

        // Check maintenance windows
        for (start_h, start_m, end_h, end_m) in &self.maintenance_windows {
            if hour >= *start_h && hour < *end_h {
                if hour == *start_h && minute < *start_m {
                    continue;
                }
                if hour == *end_h - 1 && minute >= *end_m {
                    continue;
                }
                return false;
            }
        }

        // Special handling for CME (spans midnight)
        // For simplicity, assume trading is active if not in maintenance
        // More sophisticated handling would require tracking session state
        true
    }
}

/// Gap detector for tick data
pub struct GapDetector {
    config: GapDetectionConfig,
}

impl GapDetector {
    /// Create a new gap detector
    pub fn new(config: GapDetectionConfig) -> Self {
        Self { config }
    }

    /// Create with default config
    pub fn default_config() -> Self {
        Self::new(GapDetectionConfig::default())
    }

    /// Detect gaps in tick data for a single symbol
    ///
    /// Takes ticks sorted by timestamp and returns gaps where data is missing
    pub fn detect_gaps(
        &self,
        symbol: &str,
        ticks: &[TickData],
        expected_start: DateTime<Utc>,
        expected_end: DateTime<Utc>,
    ) -> Vec<DataGap> {
        let mut gaps = Vec::new();
        let min_gap_duration = Duration::minutes(self.config.min_gap_minutes);

        if ticks.is_empty() {
            // Entire range is a gap
            let duration = expected_end - expected_start;
            if duration >= min_gap_duration {
                let gap = self.create_gap(symbol, expected_start, expected_end);
                gaps.push(gap);
            }
            return gaps;
        }

        // Check for gap at the beginning
        let first_tick_time = ticks[0].timestamp;
        if first_tick_time > expected_start {
            let duration = first_tick_time - expected_start;
            if duration >= min_gap_duration {
                if self.is_valid_gap(expected_start, first_tick_time) {
                    let gap = self.create_gap(symbol, expected_start, first_tick_time);
                    gaps.push(gap);
                }
            }
        }

        // Check for gaps between consecutive ticks
        for window in ticks.windows(2) {
            let prev_time = window[0].timestamp;
            let curr_time = window[1].timestamp;
            let duration = curr_time - prev_time;

            if duration >= min_gap_duration {
                if self.is_valid_gap(prev_time, curr_time) {
                    let gap = self.create_gap(symbol, prev_time, curr_time);
                    gaps.push(gap);
                }
            }
        }

        // Check for gap at the end
        let last_tick_time = ticks[ticks.len() - 1].timestamp;
        if last_tick_time < expected_end {
            let duration = expected_end - last_tick_time;
            if duration >= min_gap_duration {
                if self.is_valid_gap(last_tick_time, expected_end) {
                    let gap = self.create_gap(symbol, last_tick_time, expected_end);
                    gaps.push(gap);
                }
            }
        }

        gaps
    }

    /// Detect gaps across multiple symbols
    pub fn detect_gaps_multi(
        &self,
        tick_data: &HashMap<String, Vec<TickData>>,
        expected_start: DateTime<Utc>,
        expected_end: DateTime<Utc>,
    ) -> Vec<DataGap> {
        let mut all_gaps = Vec::new();

        for (symbol, ticks) in tick_data {
            let gaps = self.detect_gaps(symbol, ticks, expected_start, expected_end);
            all_gaps.extend(gaps);
        }

        // Sort by symbol then start time
        all_gaps.sort_by(|a, b| a.symbol.cmp(&b.symbol).then(a.start.cmp(&b.start)));

        all_gaps
    }

    /// Check if a gap is valid (not during maintenance/off hours)
    fn is_valid_gap(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
        // Prefer session_schedule over legacy trading_hours
        if let Some(ref schedule) = self.config.session_schedule {
            // Check if at least part of the gap overlaps with trading hours
            // Use session schedule's is_trading_time method
            let start_tradeable = schedule.is_trading_time(start);
            let end_tradeable = schedule.is_trading_time(end);

            // Check if any point in the gap is during trading hours
            // For more accurate detection, sample points within the gap
            if start_tradeable || end_tradeable {
                return true;
            }

            // Check midpoint for longer gaps
            let midpoint = start + (end - start) / 2;
            schedule.is_trading_time(midpoint)
        } else if let Some(ref trading_hours) = self.config.trading_hours {
            // Fallback to legacy trading hours
            // Check if at least part of the gap overlaps with trading hours
            trading_hours.is_trading_time(start) || trading_hours.is_trading_time(end)
        } else {
            // No trading hours filter, all gaps are valid
            true
        }
    }

    /// Create a DataGap with estimated records
    fn create_gap(&self, symbol: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> DataGap {
        let duration_minutes = (end - start).num_minutes();
        let estimated_records =
            (duration_minutes as f64 * self.config.expected_ticks_per_minute) as u64;

        DataGap::new(symbol.to_string(), start, end).with_estimated_records(estimated_records)
    }

    /// Merge adjacent or overlapping gaps for the same symbol
    pub fn merge_gaps(gaps: &[DataGap]) -> Vec<DataGap> {
        if gaps.is_empty() {
            return Vec::new();
        }

        let mut merged: Vec<DataGap> = Vec::new();
        let mut sorted_gaps = gaps.to_vec();
        sorted_gaps.sort_by(|a, b| a.symbol.cmp(&b.symbol).then(a.start.cmp(&b.start)));

        for gap in sorted_gaps {
            if let Some(last) = merged.last_mut() {
                // Check if same symbol and adjacent/overlapping
                if last.symbol == gap.symbol && gap.start <= last.end + Duration::minutes(1) {
                    // Extend the existing gap
                    last.end = last.end.max(gap.end);
                    last.duration_minutes = (last.end - last.start).num_minutes();
                    if let (Some(a), Some(b)) = (last.estimated_records, gap.estimated_records) {
                        last.estimated_records = Some(a + b);
                    }
                    continue;
                }
            }
            merged.push(gap);
        }

        merged
    }

    /// Filter gaps by minimum size
    pub fn filter_by_size(gaps: Vec<DataGap>, min_minutes: i64) -> Vec<DataGap> {
        gaps.into_iter()
            .filter(|g| g.duration_minutes >= min_minutes)
            .collect()
    }

    /// Get total estimated records across all gaps
    pub fn total_estimated_records(gaps: &[DataGap]) -> u64 {
        gaps.iter().filter_map(|g| g.estimated_records).sum()
    }

    /// Get total duration across all gaps (minutes)
    pub fn total_duration_minutes(gaps: &[DataGap]) -> i64 {
        gaps.iter().map(|g| g.duration_minutes).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::TradeSide;
    use chrono::TimeZone;
    use rust_decimal::Decimal;

    fn create_tick(symbol: &str, timestamp: DateTime<Utc>) -> TickData {
        TickData::with_details(
            timestamp,
            timestamp,
            symbol.to_string(),
            "TEST".to_string(),
            Decimal::from(100),
            Decimal::from(1),
            TradeSide::Buy,
            "TEST".to_string(),
            format!("test_{}", timestamp.timestamp_millis()),
            false,
            0,
        )
    }

    #[test]
    fn test_detect_no_gaps() {
        let config = GapDetectionConfig {
            min_gap_minutes: 5,
            expected_ticks_per_minute: 10.0,
            trading_hours: None, // Disable trading hours filter for test
            session_schedule: None,
        };
        let detector = GapDetector::new(config);

        let start = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 1, 1, 10, 10, 0).unwrap();

        // Create ticks every minute
        let ticks: Vec<TickData> = (0..10)
            .map(|i| create_tick("ES", start + Duration::minutes(i)))
            .collect();

        let gaps = detector.detect_gaps("ES", &ticks, start, end);
        assert!(gaps.is_empty());
    }

    #[test]
    fn test_detect_gap_in_middle() {
        let config = GapDetectionConfig {
            min_gap_minutes: 5,
            expected_ticks_per_minute: 10.0,
            trading_hours: None,
            session_schedule: None,
        };
        let detector = GapDetector::new(config);

        let start = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 1, 1, 10, 30, 0).unwrap();

        // Create ticks with a gap in the middle
        // Ticks at minutes 0-4, then 15-29
        // Gap is from minute 4 to minute 15 = 11 minutes
        let mut ticks = Vec::new();
        for i in 0..5 {
            ticks.push(create_tick("ES", start + Duration::minutes(i)));
        }
        for i in 15..30 {
            ticks.push(create_tick("ES", start + Duration::minutes(i)));
        }

        let gaps = detector.detect_gaps("ES", &ticks, start, end);

        assert_eq!(gaps.len(), 1);
        // Gap is between last tick at minute 4 and first tick at minute 15
        assert_eq!(gaps[0].duration_minutes, 11);
    }

    #[test]
    fn test_detect_gap_at_start() {
        let config = GapDetectionConfig {
            min_gap_minutes: 5,
            expected_ticks_per_minute: 10.0,
            trading_hours: None,
            session_schedule: None,
        };
        let detector = GapDetector::new(config);

        let start = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 1, 1, 10, 30, 0).unwrap();

        // Ticks only from minute 10 onwards
        let ticks: Vec<TickData> = (10..30)
            .map(|i| create_tick("ES", start + Duration::minutes(i)))
            .collect();

        let gaps = detector.detect_gaps("ES", &ticks, start, end);

        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].duration_minutes, 10);
        assert_eq!(gaps[0].start, start);
    }

    #[test]
    fn test_detect_gap_at_end() {
        let config = GapDetectionConfig {
            min_gap_minutes: 5,
            expected_ticks_per_minute: 10.0,
            trading_hours: None,
            session_schedule: None,
        };
        let detector = GapDetector::new(config);

        let start = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 1, 1, 10, 30, 0).unwrap();

        // Ticks only up to minute 19 (inclusive: minutes 0-19 = 20 ticks)
        // Gap is from minute 19 to minute 30 = 11 minutes
        let ticks: Vec<TickData> = (0..20)
            .map(|i| create_tick("ES", start + Duration::minutes(i)))
            .collect();

        let gaps = detector.detect_gaps("ES", &ticks, start, end);

        assert_eq!(gaps.len(), 1);
        // Gap is from last tick at minute 19 to end at minute 30 = 11 minutes
        assert_eq!(gaps[0].duration_minutes, 11);
    }

    #[test]
    fn test_detect_empty_data_full_gap() {
        let config = GapDetectionConfig {
            min_gap_minutes: 5,
            expected_ticks_per_minute: 10.0,
            trading_hours: None,
            session_schedule: None,
        };
        let detector = GapDetector::new(config);

        let start = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 1, 1, 10, 30, 0).unwrap();

        let ticks: Vec<TickData> = vec![];

        let gaps = detector.detect_gaps("ES", &ticks, start, end);

        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].duration_minutes, 30);
        assert_eq!(gaps[0].start, start);
        assert_eq!(gaps[0].end, end);
    }

    #[test]
    fn test_merge_adjacent_gaps() {
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();

        let gaps = vec![
            DataGap::new("ES".to_string(), start, start + Duration::minutes(10)),
            DataGap::new(
                "ES".to_string(),
                start + Duration::minutes(10),
                start + Duration::minutes(20),
            ),
            DataGap::new("NQ".to_string(), start, start + Duration::minutes(15)),
        ];

        let merged = GapDetector::merge_gaps(&gaps);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].symbol, "ES");
        assert_eq!(merged[0].duration_minutes, 20);
        assert_eq!(merged[1].symbol, "NQ");
    }

    #[test]
    fn test_estimated_records() {
        let config = GapDetectionConfig {
            min_gap_minutes: 5,
            expected_ticks_per_minute: 100.0,
            trading_hours: None,
            session_schedule: None,
        };
        let detector = GapDetector::new(config);

        let start = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2025, 1, 1, 10, 10, 0).unwrap();

        let gaps = detector.detect_gaps("ES", &[], start, end);

        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].estimated_records, Some(1000)); // 10 mins * 100 ticks/min
    }

    #[test]
    fn test_total_calculations() {
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();

        let gaps = vec![
            DataGap::new("ES".to_string(), start, start + Duration::minutes(10))
                .with_estimated_records(1000),
            DataGap::new(
                "ES".to_string(),
                start + Duration::minutes(20),
                start + Duration::minutes(30),
            )
            .with_estimated_records(1000),
        ];

        assert_eq!(GapDetector::total_duration_minutes(&gaps), 20);
        assert_eq!(GapDetector::total_estimated_records(&gaps), 2000);
    }

    #[test]
    fn test_config_with_session_schedule() {
        use crate::instruments::SessionSchedule;

        // Create a simple 24/7 schedule for testing
        let schedule = SessionSchedule::always_open();
        let config = GapDetectionConfig::with_session_schedule(schedule);

        assert!(config.session_schedule.is_some());
        assert!(config.trading_hours.is_none()); // Prefer session schedule

        let detector = GapDetector::new(config);

        // Test that the detector is created successfully
        assert_eq!(detector.config.min_gap_minutes, 5);

        // With always_open schedule, gaps should be detected at any time
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 3, 0, 0).unwrap();
        let end = start + Duration::minutes(10);
        let gaps = detector.detect_gaps("TEST", &[], start, end);
        assert_eq!(gaps.len(), 1);
    }

    #[test]
    fn test_config_crypto_24_7() {
        let config = GapDetectionConfig::crypto_24_7();
        assert!(config.trading_hours.is_some());
        assert!(config.session_schedule.is_none());

        let trading_hours = config.trading_hours.unwrap();

        // Crypto should be trading at any time
        let any_time = Utc.with_ymd_and_hms(2025, 1, 1, 3, 0, 0).unwrap();
        assert!(trading_hours.is_trading_time(any_time));
    }

    #[test]
    fn test_config_no_trading_hours() {
        let config = GapDetectionConfig::no_trading_hours();
        assert!(config.trading_hours.is_none());
        assert!(config.session_schedule.is_none());

        let detector = GapDetector::new(config);

        // All gaps should be detected when no trading hours filter
        let start = Utc.with_ymd_and_hms(2025, 1, 1, 3, 0, 0).unwrap();
        let end = start + Duration::minutes(10);

        // With empty ticks, entire range should be a gap
        let gaps = detector.detect_gaps("TEST", &[], start, end);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].duration_minutes, 10);
    }
}
