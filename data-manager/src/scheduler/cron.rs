//! Cron-based scheduling for recurring jobs

use chrono::{DateTime, Datelike, Duration, Utc, Weekday};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::debug;

/// Cron-like schedule specification
#[derive(Debug, Clone)]
pub struct Schedule {
    /// Schedule name
    pub name: String,
    /// Schedule expression
    pub expression: ScheduleExpression,
    /// Whether the schedule is enabled
    pub enabled: bool,
    /// Last run time
    pub last_run: Option<DateTime<Utc>>,
    /// Next run time
    pub next_run: Option<DateTime<Utc>>,
}

impl Schedule {
    /// Create a new schedule
    pub fn new(name: String, expression: ScheduleExpression) -> Self {
        let next_run = expression.next_occurrence(Utc::now());
        Self {
            name,
            expression,
            enabled: true,
            last_run: None,
            next_run,
        }
    }

    /// Enable the schedule
    pub fn enable(&mut self) {
        self.enabled = true;
        self.next_run = self.expression.next_occurrence(Utc::now());
    }

    /// Disable the schedule
    pub fn disable(&mut self) {
        self.enabled = false;
        self.next_run = None;
    }

    /// Mark as run and calculate next occurrence
    pub fn mark_run(&mut self) {
        self.last_run = Some(Utc::now());
        self.next_run = self.expression.next_occurrence(Utc::now());
    }

    /// Check if schedule should run now
    pub fn should_run(&self) -> bool {
        if !self.enabled {
            return false;
        }
        match self.next_run {
            Some(next) => Utc::now() >= next,
            None => false,
        }
    }
}

/// Schedule expression (simplified cron-like)
#[derive(Debug, Clone)]
pub enum ScheduleExpression {
    /// Run every N seconds
    EverySeconds(u32),
    /// Run every N minutes
    EveryMinutes(u32),
    /// Run every N hours
    EveryHours(u32),
    /// Run daily at specific time (hour, minute)
    DailyAt(u32, u32),
    /// Run weekly on specific day and time
    WeeklyAt(Weekday, u32, u32),
    /// Run at specific interval
    Interval(Duration),
}

impl ScheduleExpression {
    /// Calculate next occurrence from a given time
    pub fn next_occurrence(&self, from: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            ScheduleExpression::EverySeconds(s) => {
                Some(from + Duration::seconds(*s as i64))
            }
            ScheduleExpression::EveryMinutes(m) => {
                Some(from + Duration::minutes(*m as i64))
            }
            ScheduleExpression::EveryHours(h) => {
                Some(from + Duration::hours(*h as i64))
            }
            ScheduleExpression::DailyAt(hour, minute) => {
                let today = from.date_naive();
                let time = chrono::NaiveTime::from_hms_opt(*hour, *minute, 0)?;
                let datetime = today.and_time(time);
                let datetime_utc = DateTime::<Utc>::from_naive_utc_and_offset(datetime, Utc);

                if datetime_utc > from {
                    Some(datetime_utc)
                } else {
                    Some(datetime_utc + Duration::days(1))
                }
            }
            ScheduleExpression::WeeklyAt(weekday, hour, minute) => {
                let today = from.date_naive();
                let time = chrono::NaiveTime::from_hms_opt(*hour, *minute, 0)?;

                // Find next occurrence of the weekday
                let current_weekday = from.weekday();
                let days_until = (*weekday as i64 - current_weekday as i64 + 7) % 7;
                let days_until = if days_until == 0 {
                    let today_time = today.and_time(time);
                    let today_utc = DateTime::<Utc>::from_naive_utc_and_offset(today_time, Utc);
                    if today_utc > from {
                        0
                    } else {
                        7
                    }
                } else {
                    days_until
                };

                let target_date = today + Duration::days(days_until);
                let datetime = target_date.and_time(time);
                Some(DateTime::<Utc>::from_naive_utc_and_offset(datetime, Utc))
            }
            ScheduleExpression::Interval(duration) => Some(from + *duration),
        }
    }
}

/// Scheduled task
pub trait ScheduledTask: Send + Sync {
    /// Task name
    fn name(&self) -> &str;

    /// Execute the task
    fn execute(&self) -> Result<(), String>;
}

/// Simple scheduler for recurring tasks
pub struct Scheduler {
    /// Schedules by name
    schedules: Arc<RwLock<HashMap<String, Schedule>>>,
    /// Shutdown signal (reserved for graceful shutdown implementation)
    #[allow(dead_code)]
    shutdown_tx: Option<broadcast::Sender<()>>,
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new() -> Self {
        Self {
            schedules: Arc::new(RwLock::new(HashMap::new())),
            shutdown_tx: None,
        }
    }

    /// Add a schedule
    pub fn add_schedule(&self, schedule: Schedule) {
        let name = schedule.name.clone();
        self.schedules.write().insert(name.clone(), schedule);
        debug!("Added schedule: {}", name);
    }

    /// Remove a schedule
    pub fn remove_schedule(&self, name: &str) -> bool {
        self.schedules.write().remove(name).is_some()
    }

    /// Enable a schedule
    pub fn enable(&self, name: &str) -> bool {
        if let Some(schedule) = self.schedules.write().get_mut(name) {
            schedule.enable();
            true
        } else {
            false
        }
    }

    /// Disable a schedule
    pub fn disable(&self, name: &str) -> bool {
        if let Some(schedule) = self.schedules.write().get_mut(name) {
            schedule.disable();
            true
        } else {
            false
        }
    }

    /// Get schedules that should run now
    pub fn due_schedules(&self) -> Vec<String> {
        self.schedules
            .read()
            .iter()
            .filter(|(_, s): &(&String, &Schedule)| s.should_run())
            .map(|(name, _): (&String, &Schedule)| name.clone())
            .collect()
    }

    /// Mark a schedule as run
    pub fn mark_run(&self, name: &str) {
        if let Some(ref mut schedule) = self.schedules.write().get_mut(name) {
            schedule.mark_run();
        }
    }

    /// List all schedules
    pub fn list_schedules(&self) -> Vec<Schedule> {
        self.schedules.read().values().cloned().collect()
    }

    /// Get a specific schedule
    pub fn get_schedule(&self, name: &str) -> Option<Schedule> {
        self.schedules.read().get(name).cloned()
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn test_schedule_every_seconds() {
        let expr = ScheduleExpression::EverySeconds(30);
        let now = Utc::now();
        let next = expr.next_occurrence(now).unwrap();
        assert!(next > now);
        assert!((next - now).num_seconds() == 30);
    }

    #[test]
    fn test_schedule_daily() {
        let expr = ScheduleExpression::DailyAt(14, 30);
        let now = Utc::now();
        let next = expr.next_occurrence(now).unwrap();
        assert!(next > now);
        assert_eq!(next.hour(), 14);
        assert_eq!(next.minute(), 30);
    }

    #[test]
    fn test_schedule_management() {
        let scheduler = Scheduler::new();

        let schedule = Schedule::new(
            "test_schedule".to_string(),
            ScheduleExpression::EveryMinutes(5),
        );
        scheduler.add_schedule(schedule);

        assert!(scheduler.get_schedule("test_schedule").is_some());
        assert!(scheduler.disable("test_schedule"));

        let schedule = scheduler.get_schedule("test_schedule").unwrap();
        assert!(!schedule.enabled);

        assert!(scheduler.remove_schedule("test_schedule"));
        assert!(scheduler.get_schedule("test_schedule").is_none());
    }
}
