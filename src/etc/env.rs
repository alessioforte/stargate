use std::env::VarError;

#[derive(Debug, thiserror::Error)]
pub enum EnvError {
    #[error("environment variable `{name}` contains invalid value `{value}`: {message}")]
    Parse {
        name: String,
        value: String,
        message: String,
    },
    #[error("failed to read environment variable `{name}`: {source}")]
    Read { name: String, source: VarError },
}

pub fn parse_or<T>(name: &str, default: T) -> Result<T, EnvError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match std::env::var(name) {
        Ok(value) => value.parse::<T>().map_err(|source| EnvError::Parse {
            name: name.to_string(),
            value,
            message: source.to_string(),
        }),
        Err(VarError::NotPresent) => Ok(default),
        Err(source) => Err(EnvError::Read {
            name: name.to_string(),
            source,
        }),
    }
}

pub fn bool_or(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

pub fn string_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

pub fn optional_string(name: &str) -> Option<String> {
    std::env::var(name).ok()
}
