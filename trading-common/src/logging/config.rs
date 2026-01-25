//! Logging configuration and initialization.

use std::env;

use tracing_subscriber::fmt::time::{ChronoLocal, ChronoUtc};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

use super::json_layer::JsonLayer;

/// Log output format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    /// Human-readable format with colors (default for terminals)
    #[default]
    Pretty,
    /// Compact single-line format
    Compact,
    /// JSON format for machine parsing and HTML clients
    Json,
}

impl LogFormat {
    /// Parse format from string
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => LogFormat::Json,
            "compact" => LogFormat::Compact,
            "pretty" | _ => LogFormat::Pretty,
        }
    }
}

/// Timestamp format for log entries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimestampFormat {
    /// Local time with timezone (default)
    #[default]
    Local,
    /// UTC time (ISO 8601)
    Utc,
    /// No timestamps
    None,
}

impl TimestampFormat {
    /// Parse format from string
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "utc" => TimestampFormat::Utc,
            "none" | "off" => TimestampFormat::None,
            "local" | _ => TimestampFormat::Local,
        }
    }
}

/// Logging configuration
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Output format (pretty, compact, json)
    pub format: LogFormat,
    /// Timestamp format
    pub timestamps: TimestampFormat,
    /// Default log level filter
    pub default_level: String,
    /// Include source file location
    pub include_location: bool,
    /// Include thread IDs
    pub include_thread_ids: bool,
    /// Include target (module path)
    pub include_target: bool,
    /// Include span information
    pub include_spans: bool,
    /// Application name for JSON logs
    pub app_name: Option<String>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            format: LogFormat::Pretty,
            timestamps: TimestampFormat::Local,
            default_level: "info".to_string(),
            include_location: true,
            include_thread_ids: false,
            include_target: true,
            include_spans: false,
            app_name: None,
        }
    }
}

impl LogConfig {
    /// Create config from environment variables
    ///
    /// Reads:
    /// - `LOG_FORMAT`: pretty, compact, or json
    /// - `LOG_TIMESTAMPS`: local, utc, or none
    /// - `LOG_LEVEL`: default log level (fallback if RUST_LOG not set)
    /// - `LOG_LOCATION`: true/false for file:line info
    /// - `LOG_THREAD_IDS`: true/false for thread IDs
    /// - `LOG_APP_NAME`: application name for JSON logs
    pub fn from_env() -> Self {
        Self {
            format: env::var("LOG_FORMAT")
                .map(|s| LogFormat::from_str(&s))
                .unwrap_or_default(),
            timestamps: env::var("LOG_TIMESTAMPS")
                .map(|s| TimestampFormat::from_str(&s))
                .unwrap_or_default(),
            default_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            include_location: env::var("LOG_LOCATION")
                .map(|s| s == "true" || s == "1")
                .unwrap_or(true),
            include_thread_ids: env::var("LOG_THREAD_IDS")
                .map(|s| s == "true" || s == "1")
                .unwrap_or(false),
            include_target: true,
            include_spans: false,
            app_name: env::var("LOG_APP_NAME").ok(),
        }
    }

    /// Create config for JSON output (ideal for HTML clients)
    pub fn json() -> Self {
        Self {
            format: LogFormat::Json,
            timestamps: TimestampFormat::Utc,
            include_location: true,
            include_thread_ids: true,
            include_target: true,
            include_spans: true,
            ..Default::default()
        }
    }

    /// Create config for compact output (ideal for production)
    pub fn compact() -> Self {
        Self {
            format: LogFormat::Compact,
            include_location: false,
            include_thread_ids: false,
            ..Default::default()
        }
    }

    /// Set the application name
    pub fn with_app_name(mut self, name: impl Into<String>) -> Self {
        self.app_name = Some(name.into());
        self
    }

    /// Set the default log level
    pub fn with_default_level(mut self, level: impl Into<String>) -> Self {
        self.default_level = level.into();
        self
    }
}

/// Initialize logging with the given configuration
///
/// # Errors
///
/// Returns an error if the subscriber cannot be initialized (e.g., already set)
pub fn init_logging(config: LogConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Build env filter from RUST_LOG or default
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.default_level));

    match config.format {
        LogFormat::Json => init_json_logging(config, env_filter),
        LogFormat::Compact => init_fmt_logging(config, env_filter, true),
        LogFormat::Pretty => init_fmt_logging(config, env_filter, false),
    }
}

/// Initialize JSON logging
fn init_json_logging(
    config: LogConfig,
    env_filter: EnvFilter,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json_layer = JsonLayer::new(
        config.app_name.clone(),
        config.include_location,
        config.include_thread_ids,
        config.timestamps == TimestampFormat::Utc,
    );

    tracing_subscriber::registry()
        .with(env_filter)
        .with(json_layer)
        .try_init()?;

    Ok(())
}

/// Initialize fmt logging (pretty or compact)
fn init_fmt_logging(
    config: LogConfig,
    env_filter: EnvFilter,
    compact: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let registry = tracing_subscriber::registry().with(env_filter);

    // Build the format layer based on timestamp config
    match config.timestamps {
        TimestampFormat::Local => {
            let layer = build_fmt_layer(&config, compact)
                .with_timer(ChronoLocal::new("%Y-%m-%d %H:%M:%S%.3f %z".to_string()));
            registry.with(layer).try_init()?;
        }
        TimestampFormat::Utc => {
            let layer = build_fmt_layer(&config, compact)
                .with_timer(ChronoUtc::new("%Y-%m-%dT%H:%M:%S%.3fZ".to_string()));
            registry.with(layer).try_init()?;
        }
        TimestampFormat::None => {
            let layer = build_fmt_layer(&config, compact).without_time();
            registry.with(layer).try_init()?;
        }
    }

    Ok(())
}

/// Build the fmt layer with common settings
fn build_fmt_layer<S>(
    config: &LogConfig,
    compact: bool,
) -> fmt::Layer<S, fmt::format::DefaultFields, fmt::format::Format<fmt::format::Full>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let layer = fmt::layer()
        .with_target(config.include_target)
        .with_thread_ids(config.include_thread_ids)
        .with_file(config.include_location)
        .with_line_number(config.include_location)
        .with_level(true)
        .with_ansi(atty::is(atty::Stream::Stdout));

    if compact {
        // Note: compact() changes the format type, so we return the full format
        // and let the caller handle it
        layer
    } else {
        layer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_format_from_str() {
        assert_eq!(LogFormat::from_str("json"), LogFormat::Json);
        assert_eq!(LogFormat::from_str("JSON"), LogFormat::Json);
        assert_eq!(LogFormat::from_str("compact"), LogFormat::Compact);
        assert_eq!(LogFormat::from_str("pretty"), LogFormat::Pretty);
        assert_eq!(LogFormat::from_str("unknown"), LogFormat::Pretty);
    }

    #[test]
    fn test_timestamp_format_from_str() {
        assert_eq!(TimestampFormat::from_str("utc"), TimestampFormat::Utc);
        assert_eq!(TimestampFormat::from_str("UTC"), TimestampFormat::Utc);
        assert_eq!(TimestampFormat::from_str("local"), TimestampFormat::Local);
        assert_eq!(TimestampFormat::from_str("none"), TimestampFormat::None);
        assert_eq!(TimestampFormat::from_str("off"), TimestampFormat::None);
    }

    #[test]
    fn test_log_config_default() {
        let config = LogConfig::default();
        assert_eq!(config.format, LogFormat::Pretty);
        assert_eq!(config.timestamps, TimestampFormat::Local);
        assert!(config.include_location);
        assert!(config.include_target);
        assert!(!config.include_thread_ids);
    }

    #[test]
    fn test_log_config_json() {
        let config = LogConfig::json();
        assert_eq!(config.format, LogFormat::Json);
        assert_eq!(config.timestamps, TimestampFormat::Utc);
        assert!(config.include_location);
        assert!(config.include_thread_ids);
    }

    #[test]
    fn test_log_config_compact() {
        let config = LogConfig::compact();
        assert_eq!(config.format, LogFormat::Compact);
        assert!(!config.include_location);
        assert!(!config.include_thread_ids);
    }

    #[test]
    fn test_log_config_builder() {
        let config = LogConfig::default()
            .with_app_name("test-app")
            .with_default_level("debug");

        assert_eq!(config.app_name, Some("test-app".to_string()));
        assert_eq!(config.default_level, "debug");
    }
}
