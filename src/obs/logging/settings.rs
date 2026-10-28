use crate::conf::env::{optional_string, parse_bool};
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Compact,
    Pretty,
}

impl LogFormat {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Compact => "compact",
            Self::Pretty => "pretty",
        }
    }

    pub(super) fn ansi(self, stdout_is_terminal: bool) -> bool {
        stdout_is_terminal && self != Self::Json
    }
}

#[derive(Debug)]
pub struct Settings {
    pub(super) format: LogFormat,
    pub(super) file_enabled: bool,
    pub(super) console_enabled: bool,
}

impl Settings {
    pub fn from_env() -> io::Result<Self> {
        let format = match std::env::var("LOG_FORMAT") {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "LOG_FORMAT must contain valid Unicode",
                ));
            }
        };
        Self::from_values(
            format.as_deref(),
            optional_string("LOG_FILE_ENABLED").as_deref(),
            optional_string("LOG_CONSOLE_ENABLED").as_deref(),
            cfg!(debug_assertions),
        )
    }

    fn from_values(
        format: Option<&str>,
        file_enabled: Option<&str>,
        console_enabled: Option<&str>,
        debug: bool,
    ) -> io::Result<Self> {
        let format = match format {
            Some("json") => LogFormat::Json,
            Some("compact") => LogFormat::Compact,
            Some("pretty") => LogFormat::Pretty,
            None if debug => LogFormat::Pretty,
            None => LogFormat::Json,
            Some(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "LOG_FORMAT must be json, compact, or pretty",
                ));
            }
        };
        // Preserve the existing sink flag parsing: unknown values mean false.
        let file_enabled = file_enabled.is_some_and(|value| parse_bool(value).unwrap_or(false));
        let console_enabled =
            console_enabled.is_none_or(|value| parse_bool(value).unwrap_or(false));
        Ok(Self {
            format,
            file_enabled,
            console_enabled: console_enabled || !file_enabled,
        })
    }

    pub fn format(&self) -> LogFormat {
        self.format
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_stdout_in_both_build_modes() {
        for (debug, format) in [(true, LogFormat::Pretty), (false, LogFormat::Json)] {
            let settings = Settings::from_values(None, None, None, debug).unwrap();
            assert_eq!(settings.format, format);
            assert!(settings.console_enabled);
            assert!(!settings.file_enabled);
        }
    }

    #[test]
    fn explicit_formats_override_build_defaults() {
        for debug in [true, false] {
            for (value, expected) in [
                ("json", LogFormat::Json),
                ("compact", LogFormat::Compact),
                ("pretty", LogFormat::Pretty),
            ] {
                let settings = Settings::from_values(Some(value), None, None, debug).unwrap();
                assert_eq!(settings.format, expected);
            }
        }
    }

    #[test]
    fn invalid_formats_are_rejected() {
        for value in ["", "JSON", "pretty ", "text", "jsonl"] {
            let error = Settings::from_values(Some(value), None, None, false).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(error.to_string().contains("LOG_FORMAT"));
        }
    }

    #[test]
    fn file_and_console_sinks_can_be_selected_independently() {
        for (file, console) in [(true, true), (true, false), (false, true)] {
            let settings = Settings::from_values(
                None,
                Some(if file { "true" } else { "false" }),
                Some(if console { "true" } else { "false" }),
                false,
            )
            .unwrap();
            assert_eq!(settings.file_enabled, file);
            assert_eq!(settings.console_enabled, console);
        }
    }

    #[test]
    fn disabling_both_sinks_keeps_stdout_enabled() {
        let settings = Settings::from_values(None, Some("false"), Some("false"), false).unwrap();
        assert!(settings.console_enabled);
        assert!(!settings.file_enabled);
    }

    #[test]
    fn ansi_requires_a_terminal_and_a_text_format() {
        for format in [LogFormat::Json, LogFormat::Compact, LogFormat::Pretty] {
            assert!(!format.ansi(false));
            assert_eq!(format.ansi(true), format != LogFormat::Json);
        }
    }
}
