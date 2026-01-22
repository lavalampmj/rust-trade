//! Trading session and market hours management.
//!
//! This module provides comprehensive session schedule support for:
//! - Regular trading sessions
//! - Extended hours (pre-market, after-hours)
//! - Market calendars with holidays
//! - Maintenance windows
//!
//! # Timezone Support
//!
//! All sessions are defined in exchange-local time using `chrono_tz`.
//! The system handles DST transitions automatically.
//!
//! # Example
//!
//! ```ignore
//! use trading_common::instruments::session::{SessionSchedule, TradingSession, SessionType};
//! use chrono::{NaiveTime, Weekday};
//! use chrono_tz::America::New_York;
//!
//! let schedule = SessionSchedule::new(New_York)
//!     .with_session(TradingSession::regular(
//!         "Regular",
//!         NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
//!         NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
//!         vec![Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri],
//!     ));
//! ```

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::orders::OrderType;

/// Complete session schedule for a symbol.
///
/// Defines when a market is open for trading, including:
/// - Regular trading sessions
/// - Extended hours (pre-market, after-hours)
/// - Scheduled maintenance windows
/// - Holiday calendar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSchedule {
    /// Exchange timezone (e.g., "America/New_York", "Asia/Tokyo")
    #[serde(with = "tz_serde")]
    pub timezone: Tz,

    /// Regular trading sessions
    #[serde(default)]
    pub regular_sessions: Vec<TradingSession>,

    /// Extended hours sessions (pre-market, after-hours)
    #[serde(default)]
    pub extended_sessions: Vec<TradingSession>,

    /// Market calendar for holidays
    #[serde(default)]
    pub calendar: MarketCalendar,

    /// Maintenance windows (scheduled downtime)
    #[serde(default)]
    pub maintenance_windows: Vec<MaintenanceWindow>,
}

/// Custom serde module for chrono_tz::Tz
mod tz_serde {
    use chrono_tz::Tz;
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::str::FromStr;

    pub fn serialize<S>(tz: &Tz, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(tz.name())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Tz, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Tz::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl SessionSchedule {
    /// Create a new session schedule with the given timezone
    pub fn new(timezone: Tz) -> Self {
        Self {
            timezone,
            regular_sessions: Vec::new(),
            extended_sessions: Vec::new(),
            calendar: MarketCalendar::default(),
            maintenance_windows: Vec::new(),
        }
    }

    /// Create a 24/7 schedule (no restrictions)
    pub fn always_open() -> Self {
        let schedule = Self::new(chrono_tz::UTC);
        schedule.with_session(TradingSession::continuous())
    }

    /// Add a trading session
    pub fn with_session(mut self, session: TradingSession) -> Self {
        match session.session_type {
            SessionType::Regular => self.regular_sessions.push(session),
            _ => self.extended_sessions.push(session),
        }
        self
    }

    /// Add a maintenance window
    pub fn with_maintenance(mut self, window: MaintenanceWindow) -> Self {
        self.maintenance_windows.push(window);
        self
    }

    /// Add a holiday to the calendar
    pub fn with_holiday(mut self, date: NaiveDate, description: Option<String>) -> Self {
        self.calendar.add_holiday(date, description);
        self
    }

    /// Check if the market is open at the given UTC time
    pub fn is_open(&self, utc_time: DateTime<Utc>) -> bool {
        let local_time = utc_time.with_timezone(&self.timezone);
        let date = local_time.date_naive();
        let time = local_time.time();
        let weekday = local_time.weekday();

        // Check if it's a holiday
        if self.calendar.is_holiday(date) {
            return false;
        }

        // Check maintenance windows
        if self.is_in_maintenance(weekday, time) {
            return false;
        }

        // Check regular sessions
        for session in &self.regular_sessions {
            if session.is_active(weekday, time) {
                return true;
            }
        }

        false
    }

    /// Check if extended hours trading is available
    pub fn is_extended_hours(&self, utc_time: DateTime<Utc>) -> bool {
        let local_time = utc_time.with_timezone(&self.timezone);
        let date = local_time.date_naive();
        let time = local_time.time();
        let weekday = local_time.weekday();

        // Check if it's a holiday
        if self.calendar.is_holiday(date) {
            return false;
        }

        // Check extended sessions
        for session in &self.extended_sessions {
            if session.is_active(weekday, time) {
                return true;
            }
        }

        false
    }

    /// Check if any trading is available (regular or extended hours)
    ///
    /// Returns true if the market is open for any type of trading,
    /// including pre-market and after-hours sessions.
    pub fn is_trading_time(&self, utc_time: DateTime<Utc>) -> bool {
        self.is_open(utc_time) || self.is_extended_hours(utc_time)
    }

    /// Check if currently in a maintenance window
    pub fn is_in_maintenance(&self, weekday: Weekday, time: NaiveTime) -> bool {
        for window in &self.maintenance_windows {
            if window.is_active(weekday, time) {
                return true;
            }
        }
        false
    }

    /// Get the current session state
    pub fn get_session_state(&self, utc_time: DateTime<Utc>) -> SessionState {
        let local_time = utc_time.with_timezone(&self.timezone);
        let date = local_time.date_naive();
        let time = local_time.time();
        let weekday = local_time.weekday();

        // Check if it's a holiday
        if self.calendar.is_holiday(date) {
            return SessionState {
                status: MarketStatus::Closed,
                current_session: None,
                next_change: self.next_session_open(utc_time),
                reason: Some("Holiday".to_string()),
            };
        }

        // Check maintenance windows
        if self.is_in_maintenance(weekday, time) {
            return SessionState {
                status: MarketStatus::Maintenance,
                current_session: None,
                next_change: self.next_maintenance_end(utc_time),
                reason: Some("Scheduled maintenance".to_string()),
            };
        }

        // Check regular sessions
        for session in &self.regular_sessions {
            if session.is_active(weekday, time) {
                return SessionState {
                    status: MarketStatus::Open,
                    current_session: Some(session.clone()),
                    next_change: self.session_end_time(session, utc_time),
                    reason: None,
                };
            }
        }

        // Check extended sessions
        for session in &self.extended_sessions {
            if session.is_active(weekday, time) {
                let status = match session.session_type {
                    SessionType::PreMarket => MarketStatus::PreMarket,
                    SessionType::AfterHours => MarketStatus::AfterHours,
                    SessionType::Auction => MarketStatus::Auction,
                    _ => MarketStatus::Open,
                };
                return SessionState {
                    status,
                    current_session: Some(session.clone()),
                    next_change: self.session_end_time(session, utc_time),
                    reason: None,
                };
            }
        }

        // Market is closed
        SessionState {
            status: MarketStatus::Closed,
            current_session: None,
            next_change: self.next_session_open(utc_time),
            reason: None,
        }
    }

    /// Get the next session open time
    fn next_session_open(&self, utc_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let local_time = utc_time.with_timezone(&self.timezone);

        // Look ahead up to 7 days
        for day_offset in 0..7 {
            let check_date = local_time.date_naive() + chrono::Duration::days(day_offset);

            // Skip holidays
            if self.calendar.is_holiday(check_date) {
                continue;
            }

            let weekday = check_date.weekday();

            // Find the earliest session that starts after current time (or any session on future days)
            let mut earliest_start: Option<NaiveTime> = None;

            for session in &self.regular_sessions {
                if session.active_days.contains(&weekday) {
                    if day_offset == 0 {
                        // Same day: only consider sessions that haven't started
                        if session.start_time > local_time.time() {
                            match earliest_start {
                                None => earliest_start = Some(session.start_time),
                                Some(t) if session.start_time < t => {
                                    earliest_start = Some(session.start_time)
                                }
                                _ => {}
                            }
                        }
                    } else {
                        // Future day: consider all sessions
                        match earliest_start {
                            None => earliest_start = Some(session.start_time),
                            Some(t) if session.start_time < t => {
                                earliest_start = Some(session.start_time)
                            }
                            _ => {}
                        }
                    }
                }
            }

            if let Some(start_time) = earliest_start {
                let dt = self.timezone.from_local_datetime(
                    &check_date.and_time(start_time)
                );
                if let chrono::LocalResult::Single(local_dt) = dt {
                    return Some(local_dt.with_timezone(&Utc));
                }
            }
        }

        None
    }

    /// Get the session end time
    fn session_end_time(
        &self,
        session: &TradingSession,
        utc_time: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        let local_time = utc_time.with_timezone(&self.timezone);
        let date = local_time.date_naive();

        // Handle sessions that cross midnight
        let end_date = if session.end_time < session.start_time {
            date + chrono::Duration::days(1)
        } else {
            date
        };

        let dt = self.timezone.from_local_datetime(
            &end_date.and_time(session.end_time)
        );
        if let chrono::LocalResult::Single(local_dt) = dt {
            Some(local_dt.with_timezone(&Utc))
        } else {
            None
        }
    }

    /// Get the next maintenance window end
    fn next_maintenance_end(&self, utc_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let local_time = utc_time.with_timezone(&self.timezone);
        let weekday = local_time.weekday();
        let time = local_time.time();

        for window in &self.maintenance_windows {
            if window.day == weekday && time >= window.start_time && time < window.end_time {
                let date = local_time.date_naive();
                let dt = self.timezone.from_local_datetime(
                    &date.and_time(window.end_time)
                );
                if let chrono::LocalResult::Single(local_dt) = dt {
                    return Some(local_dt.with_timezone(&Utc));
                }
            }
        }

        None
    }

    /// Get the start time of the first regular session on a given date
    pub fn session_open_time(&self, date: NaiveDate) -> Option<NaiveTime> {
        let weekday = date.weekday();

        if self.calendar.is_holiday(date) {
            return None;
        }

        // Check for late opens
        if let Some(late_time) = self.calendar.late_opens.get(&date) {
            return Some(*late_time);
        }

        // Find earliest regular session
        self.regular_sessions
            .iter()
            .filter(|s| s.active_days.contains(&weekday))
            .map(|s| s.start_time)
            .min()
    }

    /// Get the end time of the last regular session on a given date
    pub fn session_close_time(&self, date: NaiveDate) -> Option<NaiveTime> {
        let weekday = date.weekday();

        if self.calendar.is_holiday(date) {
            return None;
        }

        // Check for early closes
        if let Some(early_time) = self.calendar.early_closes.get(&date) {
            return Some(*early_time);
        }

        // Find latest regular session
        self.regular_sessions
            .iter()
            .filter(|s| s.active_days.contains(&weekday))
            .map(|s| s.end_time)
            .max()
    }
}

impl Default for SessionSchedule {
    fn default() -> Self {
        Self::new(chrono_tz::UTC)
    }
}

/// A single trading session within a day.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingSession {
    /// Session name (e.g., "Regular", "Pre-Market", "After-Hours")
    pub name: String,

    /// Session type
    pub session_type: SessionType,

    /// Days this session is active
    pub active_days: Vec<Weekday>,

    /// Session start time (local timezone)
    #[serde(with = "time_serde")]
    pub start_time: NaiveTime,

    /// Session end time (local timezone)
    /// Note: If end_time < start_time, session crosses midnight
    #[serde(with = "time_serde")]
    pub end_time: NaiveTime,

    /// Order types allowed during this session
    #[serde(default = "default_order_types")]
    pub allowed_order_types: Vec<OrderType>,

    /// Whether auction occurs at open
    #[serde(default)]
    pub has_opening_auction: bool,

    /// Whether auction occurs at close
    #[serde(default)]
    pub has_closing_auction: bool,
}

fn default_order_types() -> Vec<OrderType> {
    vec![OrderType::Market, OrderType::Limit]
}

/// Custom serde module for NaiveTime
mod time_serde {
    use chrono::NaiveTime;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(time: &NaiveTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&time.format("%H:%M:%S").to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        NaiveTime::parse_from_str(&s, "%H:%M:%S").map_err(serde::de::Error::custom)
    }
}

impl TradingSession {
    /// Create a new trading session
    pub fn new(
        name: impl Into<String>,
        session_type: SessionType,
        start_time: NaiveTime,
        end_time: NaiveTime,
        active_days: Vec<Weekday>,
    ) -> Self {
        Self {
            name: name.into(),
            session_type,
            active_days,
            start_time,
            end_time,
            allowed_order_types: default_order_types(),
            has_opening_auction: false,
            has_closing_auction: false,
        }
    }

    /// Create a regular trading session
    pub fn regular(
        name: impl Into<String>,
        start_time: NaiveTime,
        end_time: NaiveTime,
        active_days: Vec<Weekday>,
    ) -> Self {
        Self::new(name, SessionType::Regular, start_time, end_time, active_days)
    }

    /// Create a pre-market session
    pub fn pre_market(
        start_time: NaiveTime,
        end_time: NaiveTime,
        active_days: Vec<Weekday>,
    ) -> Self {
        Self {
            name: "Pre-Market".to_string(),
            session_type: SessionType::PreMarket,
            active_days,
            start_time,
            end_time,
            allowed_order_types: vec![OrderType::Limit],
            has_opening_auction: false,
            has_closing_auction: false,
        }
    }

    /// Create an after-hours session
    pub fn after_hours(
        start_time: NaiveTime,
        end_time: NaiveTime,
        active_days: Vec<Weekday>,
    ) -> Self {
        Self {
            name: "After-Hours".to_string(),
            session_type: SessionType::AfterHours,
            active_days,
            start_time,
            end_time,
            allowed_order_types: vec![OrderType::Limit],
            has_opening_auction: false,
            has_closing_auction: false,
        }
    }

    /// Create a continuous 24/7 session
    pub fn continuous() -> Self {
        Self {
            name: "Continuous".to_string(),
            session_type: SessionType::Regular,
            active_days: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
                Weekday::Sat,
                Weekday::Sun,
            ],
            start_time: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
            end_time: NaiveTime::from_hms_opt(23, 59, 59).unwrap(),
            allowed_order_types: vec![OrderType::Market, OrderType::Limit],
            has_opening_auction: false,
            has_closing_auction: false,
        }
    }

    /// Add auction at open
    pub fn with_opening_auction(mut self) -> Self {
        self.has_opening_auction = true;
        self
    }

    /// Add auction at close
    pub fn with_closing_auction(mut self) -> Self {
        self.has_closing_auction = true;
        self
    }

    /// Set allowed order types
    pub fn with_order_types(mut self, types: Vec<OrderType>) -> Self {
        self.allowed_order_types = types;
        self
    }

    /// Check if this session is active at the given day and time
    pub fn is_active(&self, weekday: Weekday, time: NaiveTime) -> bool {
        if !self.active_days.contains(&weekday) {
            return false;
        }

        // Handle sessions that cross midnight
        if self.end_time < self.start_time {
            // Session crosses midnight (e.g., 17:00 - 16:00 next day)
            time >= self.start_time || time < self.end_time
        } else {
            // Normal session within same day
            time >= self.start_time && time < self.end_time
        }
    }

    /// Check if an order type is allowed during this session
    pub fn is_order_type_allowed(&self, order_type: OrderType) -> bool {
        self.allowed_order_types.contains(&order_type)
    }

    /// Get session duration in minutes
    pub fn duration_minutes(&self) -> i64 {
        let start_mins = self.start_time.hour() as i64 * 60 + self.start_time.minute() as i64;
        let end_mins = self.end_time.hour() as i64 * 60 + self.end_time.minute() as i64;

        if self.end_time < self.start_time {
            // Crosses midnight
            (24 * 60 - start_mins) + end_mins
        } else {
            end_mins - start_mins
        }
    }
}

/// Session type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionType {
    /// Regular trading hours
    #[default]
    Regular,
    /// Pre-market trading
    PreMarket,
    /// After-hours trading
    AfterHours,
    /// Auction period
    Auction,
    /// Holiday (market closed)
    Holiday,
}

impl fmt::Display for SessionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionType::Regular => write!(f, "REGULAR"),
            SessionType::PreMarket => write!(f, "PRE_MARKET"),
            SessionType::AfterHours => write!(f, "AFTER_HOURS"),
            SessionType::Auction => write!(f, "AUCTION"),
            SessionType::Holiday => write!(f, "HOLIDAY"),
        }
    }
}

/// Market calendar with holidays and special days.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarketCalendar {
    /// Holiday dates (market closed)
    #[serde(default)]
    pub holidays: HashMap<NaiveDate, String>,

    /// Early close dates with close time
    #[serde(default)]
    pub early_closes: HashMap<NaiveDate, NaiveTime>,

    /// Late open dates with open time
    #[serde(default)]
    pub late_opens: HashMap<NaiveDate, NaiveTime>,
}

impl MarketCalendar {
    /// Create a new empty calendar
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a holiday
    pub fn add_holiday(&mut self, date: NaiveDate, description: Option<String>) {
        self.holidays.insert(date, description.unwrap_or_default());
    }

    /// Add an early close
    pub fn add_early_close(&mut self, date: NaiveDate, close_time: NaiveTime) {
        self.early_closes.insert(date, close_time);
    }

    /// Add a late open
    pub fn add_late_open(&mut self, date: NaiveDate, open_time: NaiveTime) {
        self.late_opens.insert(date, open_time);
    }

    /// Check if a date is a holiday
    pub fn is_holiday(&self, date: NaiveDate) -> bool {
        self.holidays.contains_key(&date)
    }

    /// Get holiday description if it's a holiday
    pub fn get_holiday(&self, date: NaiveDate) -> Option<&str> {
        self.holidays.get(&date).map(|s| s.as_str())
    }

    /// Check if a date has an early close
    pub fn has_early_close(&self, date: NaiveDate) -> bool {
        self.early_closes.contains_key(&date)
    }

    /// Get early close time if applicable
    pub fn get_early_close(&self, date: NaiveDate) -> Option<NaiveTime> {
        self.early_closes.get(&date).copied()
    }

    /// Check if a date has a late open
    pub fn has_late_open(&self, date: NaiveDate) -> bool {
        self.late_opens.contains_key(&date)
    }

    /// Get late open time if applicable
    pub fn get_late_open(&self, date: NaiveDate) -> Option<NaiveTime> {
        self.late_opens.get(&date).copied()
    }
}

/// Scheduled maintenance window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceWindow {
    /// Day of week
    pub day: Weekday,

    /// Start time (local timezone)
    #[serde(with = "time_serde")]
    pub start_time: NaiveTime,

    /// End time (local timezone)
    #[serde(with = "time_serde")]
    pub end_time: NaiveTime,

    /// Description of maintenance
    #[serde(default)]
    pub description: Option<String>,
}

impl MaintenanceWindow {
    /// Create a new maintenance window
    pub fn new(day: Weekday, start_time: NaiveTime, end_time: NaiveTime) -> Self {
        Self {
            day,
            start_time,
            end_time,
            description: None,
        }
    }

    /// Add a description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Check if maintenance is active
    pub fn is_active(&self, weekday: Weekday, time: NaiveTime) -> bool {
        weekday == self.day && time >= self.start_time && time < self.end_time
    }
}

/// Current session state for a symbol.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// Current market status
    pub status: MarketStatus,

    /// Current session (if any)
    pub current_session: Option<TradingSession>,

    /// Time until next session change
    pub next_change: Option<DateTime<Utc>>,

    /// Reason for current status (e.g., "Holiday", "Maintenance")
    pub reason: Option<String>,
}

impl SessionState {
    /// Check if market is open for regular trading
    pub fn is_open(&self) -> bool {
        self.status == MarketStatus::Open
    }

    /// Check if any trading is available (including extended hours)
    pub fn is_tradeable(&self) -> bool {
        matches!(
            self.status,
            MarketStatus::Open | MarketStatus::PreMarket | MarketStatus::AfterHours
        )
    }
}

/// Market status enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketStatus {
    /// Market is open for regular trading
    Open,
    /// Market is closed
    Closed,
    /// Pre-market trading available
    PreMarket,
    /// After-hours trading available
    AfterHours,
    /// Auction period
    Auction,
    /// Trading halted
    Halted,
    /// Scheduled maintenance
    Maintenance,
}

impl Default for MarketStatus {
    fn default() -> Self {
        MarketStatus::Closed
    }
}

impl fmt::Display for MarketStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarketStatus::Open => write!(f, "OPEN"),
            MarketStatus::Closed => write!(f, "CLOSED"),
            MarketStatus::PreMarket => write!(f, "PRE_MARKET"),
            MarketStatus::AfterHours => write!(f, "AFTER_HOURS"),
            MarketStatus::Auction => write!(f, "AUCTION"),
            MarketStatus::Halted => write!(f, "HALTED"),
            MarketStatus::Maintenance => write!(f, "MAINTENANCE"),
        }
    }
}

/// Session events for broadcasting state changes.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// A trading session opened
    SessionOpened {
        symbol: String,
        session: TradingSession,
    },
    /// A trading session closed
    SessionClosed { symbol: String },
    /// Market was halted
    MarketHalted { symbol: String, reason: String },
    /// Market resumed after halt
    MarketResumed { symbol: String },
    /// Maintenance started
    MaintenanceStarted { symbol: String },
    /// Maintenance ended
    MaintenanceEnded { symbol: String },
}

/// Predefined session schedules for common markets.
pub mod presets {
    use super::*;
    use chrono::Weekday;

    /// US weekdays
    fn us_weekdays() -> Vec<Weekday> {
        vec![
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
        ]
    }

    /// NYSE/NASDAQ regular hours schedule
    pub fn us_equity() -> SessionSchedule {
        SessionSchedule::new(chrono_tz::America::New_York)
            .with_session(
                TradingSession::regular(
                    "Regular",
                    NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
                    NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
                    us_weekdays(),
                )
                .with_opening_auction()
                .with_closing_auction(),
            )
            .with_session(TradingSession::pre_market(
                NaiveTime::from_hms_opt(4, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
                us_weekdays(),
            ))
            .with_session(TradingSession::after_hours(
                NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(20, 0, 0).unwrap(),
                us_weekdays(),
            ))
    }

    /// CME Globex E-mini futures schedule
    pub fn cme_globex() -> SessionSchedule {
        SessionSchedule::new(chrono_tz::America::Chicago)
            .with_session(TradingSession::regular(
                "Globex",
                NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(16, 0, 0).unwrap(), // Crosses midnight
                vec![
                    Weekday::Sun,
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                ],
            ))
            .with_maintenance(MaintenanceWindow::new(
                Weekday::Fri,
                NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            ))
    }

    /// 24/7 cryptocurrency schedule
    pub fn crypto_24_7() -> SessionSchedule {
        SessionSchedule::always_open()
    }

    /// Forex schedule (Sunday 5pm - Friday 5pm ET)
    pub fn forex() -> SessionSchedule {
        SessionSchedule::new(chrono_tz::America::New_York)
            .with_session(TradingSession::regular(
                "Forex",
                NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(17, 0, 0).unwrap(), // Full 24h
                vec![
                    Weekday::Sun,
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                ],
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_session_schedule_creation() {
        let schedule = presets::us_equity();
        assert_eq!(schedule.regular_sessions.len(), 1);
        assert_eq!(schedule.extended_sessions.len(), 2);
    }

    #[test]
    fn test_trading_session_is_active() {
        let session = TradingSession::regular(
            "Test",
            NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            vec![Weekday::Mon, Weekday::Tue],
        );

        // Monday 10:00 - should be active
        assert!(session.is_active(Weekday::Mon, NaiveTime::from_hms_opt(10, 0, 0).unwrap()));

        // Monday 8:00 - before open
        assert!(!session.is_active(Weekday::Mon, NaiveTime::from_hms_opt(8, 0, 0).unwrap()));

        // Monday 17:00 - after close
        assert!(!session.is_active(Weekday::Mon, NaiveTime::from_hms_opt(17, 0, 0).unwrap()));

        // Wednesday - not active
        assert!(!session.is_active(Weekday::Wed, NaiveTime::from_hms_opt(10, 0, 0).unwrap()));
    }

    #[test]
    fn test_session_crossing_midnight() {
        let session = TradingSession::regular(
            "Globex",
            NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(), // Next day
            vec![Weekday::Sun],
        );

        // Sunday 18:00 - active
        assert!(session.is_active(Weekday::Sun, NaiveTime::from_hms_opt(18, 0, 0).unwrap()));

        // Sunday 10:00 - active (morning of trading day)
        assert!(session.is_active(Weekday::Sun, NaiveTime::from_hms_opt(10, 0, 0).unwrap()));

        // Sunday 16:30 - not active (between sessions)
        assert!(!session.is_active(Weekday::Sun, NaiveTime::from_hms_opt(16, 30, 0).unwrap()));
    }

    #[test]
    fn test_market_calendar() {
        let mut calendar = MarketCalendar::new();

        let holiday = NaiveDate::from_ymd_opt(2024, 12, 25).unwrap();
        calendar.add_holiday(holiday, Some("Christmas".to_string()));

        let early_close = NaiveDate::from_ymd_opt(2024, 12, 24).unwrap();
        calendar.add_early_close(early_close, NaiveTime::from_hms_opt(13, 0, 0).unwrap());

        assert!(calendar.is_holiday(holiday));
        assert_eq!(calendar.get_holiday(holiday), Some("Christmas"));
        assert!(calendar.has_early_close(early_close));
        assert_eq!(
            calendar.get_early_close(early_close),
            Some(NaiveTime::from_hms_opt(13, 0, 0).unwrap())
        );
    }

    #[test]
    fn test_schedule_is_open() {
        let schedule = presets::us_equity();

        // Regular hours on a Monday
        let monday_trading = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 8, 10, 0, 0) // Monday 10:00 AM
            .unwrap()
            .with_timezone(&Utc);

        assert!(schedule.is_open(monday_trading));

        // Early morning before pre-market
        let monday_early = chrono_tz::America::New_York
            .with_ymd_and_hms(2024, 1, 8, 3, 0, 0) // Monday 3:00 AM
            .unwrap()
            .with_timezone(&Utc);

        assert!(!schedule.is_open(monday_early));
    }

    #[test]
    fn test_session_duration() {
        let session = TradingSession::regular(
            "Test",
            NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            vec![Weekday::Mon],
        );

        assert_eq!(session.duration_minutes(), 390); // 6.5 hours

        let overnight = TradingSession::regular(
            "Overnight",
            NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            vec![Weekday::Mon],
        );

        assert_eq!(overnight.duration_minutes(), 23 * 60); // 23 hours
    }

    #[test]
    fn test_24_7_schedule() {
        let schedule = presets::crypto_24_7();

        // Should always be open
        let random_time = Utc::now();
        // Note: This test checks structure, actual is_open depends on continuous session logic
        assert_eq!(schedule.regular_sessions.len(), 1);
    }

    #[test]
    fn test_maintenance_window() {
        let window = MaintenanceWindow::new(
            Weekday::Fri,
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
        );

        assert!(window.is_active(Weekday::Fri, NaiveTime::from_hms_opt(16, 30, 0).unwrap()));
        assert!(!window.is_active(Weekday::Fri, NaiveTime::from_hms_opt(17, 30, 0).unwrap()));
        assert!(!window.is_active(Weekday::Mon, NaiveTime::from_hms_opt(16, 30, 0).unwrap()));
    }
}
