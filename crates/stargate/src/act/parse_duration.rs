use chrono::Duration;
use regex::Regex;

#[derive(Debug)]
pub enum ParseDurationError {
    InvalidFormat,
    InvalidNumber,
    UnknownUnit,
}

pub fn parse_duration(input: &str) -> Result<Duration, ParseDurationError> {
    let re = Regex::new(r"(?i)^(\d+)(s|m|h|d|w)$").unwrap();
    if let Some(caps) = re.captures(input.trim()) {
        let value: i64 = caps[1]
            .parse()
            .map_err(|_| ParseDurationError::InvalidNumber)?;
        let unit = &caps[2].to_lowercase();

        match unit.as_str() {
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
