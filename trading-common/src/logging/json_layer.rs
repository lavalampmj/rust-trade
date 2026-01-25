//! JSON logging layer for structured log output.
//!
//! Produces JSON log events suitable for:
//! - Log aggregation systems (ELK, Splunk, etc.)
//! - HTML/WebSocket clients for real-time log viewing
//! - Machine parsing and analysis

use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use chrono::{Local, Utc};
use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Sequence counter for log ordering
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// JSON log event structure
///
/// This structure is designed for easy parsing by HTML clients and log aggregators.
#[derive(Debug, Clone, Serialize)]
pub struct JsonLogEvent {
    /// ISO 8601 timestamp
    pub timestamp: String,
    /// Log level (TRACE, DEBUG, INFO, WARN, ERROR)
    pub level: String,
    /// Numeric level for filtering (0=TRACE, 1=DEBUG, 2=INFO, 3=WARN, 4=ERROR)
    pub level_num: u8,
    /// Target module path
    pub target: String,
    /// Log message
    pub message: String,
    /// Source file (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Source line number (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Thread ID (if enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Thread name (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_name: Option<String>,
    /// Application name (if configured)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Additional structured fields
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub fields: serde_json::Map<String, serde_json::Value>,
    /// Sequence number for ordering
    pub seq: u64,
}

impl JsonLogEvent {
    /// Create a new log event
    pub fn new(level: Level, target: &str, message: String) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            level: level.to_string(),
            level_num: level_to_num(level),
            target: target.to_string(),
            message,
            file: None,
            line: None,
            thread_id: None,
            thread_name: None,
            app: None,
            fields: serde_json::Map::new(),
            seq: SEQUENCE.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Set source location
    pub fn with_location(mut self, file: Option<&str>, line: Option<u32>) -> Self {
        self.file = file.map(|s| s.to_string());
        self.line = line;
        self
    }

    /// Set thread info
    pub fn with_thread_info(mut self) -> Self {
        self.thread_id = Some(format!("{:?}", thread::current().id()));
        self.thread_name = thread::current().name().map(|s| s.to_string());
        self
    }

    /// Set application name
    pub fn with_app(mut self, app: Option<&str>) -> Self {
        self.app = app.map(|s| s.to_string());
        self
    }

    /// Add a field
    pub fn add_field(&mut self, key: String, value: serde_json::Value) {
        self.fields.insert(key, value);
    }
}

/// Convert log level to numeric value
fn level_to_num(level: Level) -> u8 {
    match level {
        Level::TRACE => 0,
        Level::DEBUG => 1,
        Level::INFO => 2,
        Level::WARN => 3,
        Level::ERROR => 4,
    }
}

/// JSON logging layer
pub struct JsonLayer {
    app_name: Option<String>,
    include_location: bool,
    include_thread_ids: bool,
    use_utc: bool,
}

impl JsonLayer {
    /// Create a new JSON layer
    pub fn new(
        app_name: Option<String>,
        include_location: bool,
        include_thread_ids: bool,
        use_utc: bool,
    ) -> Self {
        Self {
            app_name,
            include_location,
            include_thread_ids,
            use_utc,
        }
    }
}

impl<S> Layer<S> for JsonLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();

        // Extract message and fields
        let mut visitor = JsonVisitor::default();
        event.record(&mut visitor);

        let message = visitor.message.unwrap_or_else(|| {
            visitor
                .fields
                .get("message")
                .map(|v| v.to_string())
                .unwrap_or_default()
        });

        // Build the event
        let mut log_event = JsonLogEvent::new(*metadata.level(), metadata.target(), message);

        // Add timestamp in appropriate format
        if self.use_utc {
            log_event.timestamp = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        } else {
            log_event.timestamp = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%z").to_string();
        }

        // Add location if enabled
        if self.include_location {
            log_event = log_event.with_location(metadata.file(), metadata.line());
        }

        // Add thread info if enabled
        if self.include_thread_ids {
            log_event = log_event.with_thread_info();
        }

        // Add app name
        log_event = log_event.with_app(self.app_name.as_deref());

        // Add extra fields
        for (key, value) in visitor.fields {
            if key != "message" {
                log_event.add_field(key, value);
            }
        }

        // Serialize and output
        if let Ok(json) = serde_json::to_string(&log_event) {
            let _ = writeln!(io::stdout(), "{}", json);
        }
    }
}

/// Visitor for extracting fields from tracing events
#[derive(Default)]
struct JsonVisitor {
    message: Option<String>,
    fields: serde_json::Map<String, serde_json::Value>,
}

impl Visit for JsonVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let value_str = format!("{:?}", value);
        if field.name() == "message" {
            self.message = Some(value_str.trim_matches('"').to_string());
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value_str),
            );
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(value.to_string()),
            );
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if let Some(n) = serde_json::Number::from_f64(value) {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::Number(n));
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::Bool(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_log_event_new() {
        let event = JsonLogEvent::new(Level::INFO, "test::module", "Test message".to_string());

        assert_eq!(event.level, "INFO");
        assert_eq!(event.level_num, 2);
        assert_eq!(event.target, "test::module");
        assert_eq!(event.message, "Test message");
        assert!(event.file.is_none());
        assert!(event.line.is_none());
    }

    #[test]
    fn test_json_log_event_with_location() {
        let event = JsonLogEvent::new(Level::ERROR, "test", "Error".to_string())
            .with_location(Some("src/main.rs"), Some(42));

        assert_eq!(event.file, Some("src/main.rs".to_string()));
        assert_eq!(event.line, Some(42));
    }

    #[test]
    fn test_json_log_event_with_thread_info() {
        let event = JsonLogEvent::new(Level::DEBUG, "test", "Debug".to_string()).with_thread_info();

        assert!(event.thread_id.is_some());
    }

    #[test]
    fn test_json_log_event_serialization() {
        let event = JsonLogEvent::new(Level::WARN, "test", "Warning".to_string());

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"level\":\"WARN\""));
        assert!(json.contains("\"message\":\"Warning\""));
        assert!(json.contains("\"target\":\"test\""));
    }

    #[test]
    fn test_level_to_num() {
        assert_eq!(level_to_num(Level::TRACE), 0);
        assert_eq!(level_to_num(Level::DEBUG), 1);
        assert_eq!(level_to_num(Level::INFO), 2);
        assert_eq!(level_to_num(Level::WARN), 3);
        assert_eq!(level_to_num(Level::ERROR), 4);
    }

    #[test]
    fn test_add_field() {
        let mut event = JsonLogEvent::new(Level::INFO, "test", "Test".to_string());
        event.add_field("user_id".to_string(), serde_json::json!(123));
        event.add_field("action".to_string(), serde_json::json!("login"));

        assert_eq!(event.fields.len(), 2);
        assert_eq!(event.fields.get("user_id"), Some(&serde_json::json!(123)));
    }
}
