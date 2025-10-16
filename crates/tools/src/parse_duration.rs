use chrono::Duration;
use regex::Regex;

#[derive(Debug)]
pub enum ParseDurationError {
    InvalidFormat,
    InvalidNumber,
    UnknownUnit,
}

impl std::fmt::Display for ParseDurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseDurationError::InvalidFormat => write!(f, "Invalid duration format"),
            ParseDurationError::InvalidNumber => write!(f, "Invalid number in duration"),
            ParseDurationError::UnknownUnit => write!(f, "Unknown time unit in duration"),
        }
    }
}

pub fn deserialize_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = serde::Deserialize::deserialize(deserializer)?;
    parse_duration(&s).map_err(serde::de::Error::custom)
}

pub fn serialize_duration<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    let duration_str = duration_to_string(duration);
    serializer.serialize_str(&duration_str)
}

pub fn duration_to_string(duration: &Duration) -> String {
    if duration.num_weeks() != 0 {
        format!("{}w", duration.num_weeks())
    } else if duration.num_days() != 0 {
        format!("{}d", duration.num_days())
    } else if duration.num_hours() != 0 {
        format!("{}h", duration.num_hours())
    } else if duration.num_minutes() != 0 {
        format!("{}m", duration.num_minutes())
    } else if duration.num_seconds() != 0 {
        format!("{}s", duration.num_seconds())
    } else if duration.num_milliseconds() != 0 {
        format!("{}ms", duration.num_milliseconds())
    } else if duration.num_microseconds().unwrap_or(0) != 0 {
        format!("{}us", duration.num_microseconds().unwrap_or(0))
    } else {
        format!("{}ns", duration.num_nanoseconds().unwrap_or(0))
    }
}

pub fn parse_duration(input: &str) -> Result<Duration, ParseDurationError> {
    let re = Regex::new(r"(?i)^(\d+)(s|m|h|d|w|ns|us|ms)$")
        .map_err(|_| ParseDurationError::InvalidFormat)?;
    if let Some(caps) = re.captures(input.trim()) {
        let value: i64 = caps[1]
            .parse()
            .map_err(|_| ParseDurationError::InvalidNumber)?;
        let unit = &caps[2].to_lowercase();

        match unit.as_str() {
            "ns" => Ok(Duration::nanoseconds(value)),
            "us" => Ok(Duration::microseconds(value)),
            "ms" => Ok(Duration::milliseconds(value)),
            "s" => Ok(Duration::seconds(value)),
            "m" => Ok(Duration::minutes(value)),
            "h" => Ok(Duration::hours(value)),
            "d" => Ok(Duration::days(value)),
            "w" => Ok(Duration::weeks(value)),
            _ => Err(ParseDurationError::UnknownUnit),
        }
    } else {
        Err(ParseDurationError::InvalidFormat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testparse_duration() {
        assert_eq!(
            parse_duration("500ms").unwrap(),
            Duration::milliseconds(500)
        );
        assert_eq!(
            parse_duration("1500us").unwrap(),
            Duration::microseconds(1500)
        );
        assert_eq!(
            parse_duration("2000000ns").unwrap(),
            Duration::nanoseconds(2000000)
        );
        assert_eq!(parse_duration("10s").unwrap(), Duration::seconds(10));
        assert_eq!(parse_duration("5m").unwrap(), Duration::minutes(5));
        assert_eq!(parse_duration("2h").unwrap(), Duration::hours(2));
        assert_eq!(parse_duration("1d").unwrap(), Duration::days(1));
        assert_eq!(parse_duration("3w").unwrap(), Duration::weeks(3));
        assert_eq!(parse_duration("0s").unwrap(), Duration::seconds(0));
    }

    #[test]
    fn test_case_insensitive_units() {
        assert_eq!(parse_duration("5M").unwrap(), Duration::seconds(300));
        assert_eq!(parse_duration("10D").unwrap(), Duration::days(10));
    }

    #[test]
    fn test_invalid_format() {
        assert!(matches!(
            parse_duration(""),
            Err(ParseDurationError::InvalidFormat)
        ));
        assert!(matches!(
            parse_duration("  "),
            Err(ParseDurationError::InvalidFormat)
        ));
        assert!(matches!(
            parse_duration("nope"),
            Err(ParseDurationError::InvalidFormat)
        ));
        assert!(matches!(
            parse_duration("2"),
            Err(ParseDurationError::InvalidFormat)
        ));
        assert!(matches!(
            parse_duration("m"),
            Err(ParseDurationError::InvalidFormat)
        ));
    }

    #[test]
    fn test_invalid_number() {
        assert!(matches!(
            parse_duration("xs"),
            Err(ParseDurationError::InvalidFormat)
        ));
        assert!(matches!(
            parse_duration("1.5h"),
            Err(ParseDurationError::InvalidFormat)
        ));
    }
}
