//! Time window types and utilities for rate limiting

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Represents different time windows for rate limiting quotas
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeWindow {
    /// One second window
    Second,
    /// One minute window
    Minute,
    /// One hour window
    Hour,
    /// One day window
    Day,
    /// One week window
    Week,
    /// One month window (average month duration)
    Month,
    /// One year window (average year duration)
    Year,
    /// Custom duration window
    Custom(Duration),
}

impl TimeWindow {
    /// Returns the duration of the time window
    pub fn duration(&self) -> Duration {
        match self {
            TimeWindow::Second => Duration::from_secs(1),
            TimeWindow::Minute => Duration::from_secs(60),
            TimeWindow::Hour => Duration::from_secs(3600),
            TimeWindow::Day => Duration::from_secs(86400),
            TimeWindow::Week => Duration::from_secs(604800),
            TimeWindow::Month => Duration::from_secs(2629746), // Average month
            TimeWindow::Year => Duration::from_secs(31556952), // Average year
            TimeWindow::Custom(duration) => *duration,
        }
    }

    /// Aligns a timestamp to the beginning of the time window
    ///
    /// This ensures that quota windows start at consistent boundaries.
    /// For example, a minute window will align to the start of the minute,
    /// an hour window to the start of the hour, etc.
    pub fn align_timestamp(&self, timestamp: u64) -> u64 {
        let duration_secs = self.duration().as_secs();
        (timestamp / duration_secs) * duration_secs
    }
}

impl From<&str> for TimeWindow {
    fn from(str: &str) -> TimeWindow {
        match str {
            "second" | "seconds" => TimeWindow::Second,
            "minute" | "minutes" => TimeWindow::Minute,
            "hour" | "hours" => TimeWindow::Hour,
            "day" | "days" => TimeWindow::Day,
            "week" | "weeks" => TimeWindow::Week,
            "month" | "months" => TimeWindow::Month,
            "year" | "years" => TimeWindow::Year,
            _ => TimeWindow::Custom(Duration::from_secs(300)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_window_durations() {
        assert_eq!(TimeWindow::Second.duration(), Duration::from_secs(1));
        assert_eq!(TimeWindow::Minute.duration(), Duration::from_secs(60));
        assert_eq!(TimeWindow::Hour.duration(), Duration::from_secs(3600));
        assert_eq!(TimeWindow::Day.duration(), Duration::from_secs(86400));
        assert_eq!(TimeWindow::Week.duration(), Duration::from_secs(604800));

        let custom = TimeWindow::Custom(Duration::from_secs(300));
        assert_eq!(custom.duration(), Duration::from_secs(300));
    }

    #[test]
    fn test_timestamp_alignment() {
        let timestamp = 1234567890; // Some arbitrary timestamp

        // Test minute alignment
        let aligned_minute = TimeWindow::Minute.align_timestamp(timestamp);
        assert_eq!(aligned_minute % 60, 0);
        assert!(aligned_minute <= timestamp);

        // Test hour alignment
        let aligned_hour = TimeWindow::Hour.align_timestamp(timestamp);
        assert_eq!(aligned_hour % 3600, 0);
        assert!(aligned_hour <= timestamp);

        // Test day alignment
        let aligned_day = TimeWindow::Day.align_timestamp(timestamp);
        assert_eq!(aligned_day % 86400, 0);
        assert!(aligned_day <= timestamp);
    }

    #[test]
    fn test_custom_window_alignment() {
        let custom_window = TimeWindow::Custom(Duration::from_secs(300)); // 5 minutes
        let timestamp = 1234567890;

        let aligned = custom_window.align_timestamp(timestamp);
        assert_eq!(aligned % 300, 0);
        assert!(aligned <= timestamp);
    }
}
