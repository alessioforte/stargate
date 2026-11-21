//! CORS for the IAM planes (`CORS_ORIGINS`), read and validated once at
//! startup like the other process-wide settings.

use crate::web::origin::Origin;
use http::HeaderValue;
use std::sync::OnceLock;
use tower_http::cors::{Any, CorsLayer};

const CORS_ORIGINS_ENV: &str = "CORS_ORIGINS";

static SETTINGS: OnceLock<CorsSettings> = OnceLock::new();

#[derive(Debug, PartialEq, Eq)]
enum Origins {
    /// `CORS_ORIGINS=*`: every origin, without credentials.
    Any,
    List(Vec<Origin>),
}

#[derive(Debug, PartialEq, Eq)]
struct CorsSettings {
    origins: Option<Origins>,
}

impl CorsSettings {
    fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(Self { origins: None });
        }
        if raw == "*" {
            return Ok(Self {
                origins: Some(Origins::Any),
            });
        }

        let mut origins = Vec::new();
        for entry in raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if entry == "*" {
                return Err("`*` may only be the whole value".to_string());
            }
            origins.push(Origin::parse(entry)?);
        }
        if origins.is_empty() {
            return Ok(Self { origins: None });
        }
        Ok(Self {
            origins: Some(Origins::List(origins)),
        })
    }
}

/// The origins a browser may call the IAM planes from; empty disables CORS.
fn settings() -> &'static CorsSettings {
    SETTINGS.get_or_init(|| {
        CorsSettings::parse(&std::env::var(CORS_ORIGINS_ENV).unwrap_or_default())
            .expect("CORS_ORIGINS is validated at startup")
    })
}

/// Reads and validates `CORS_ORIGINS` once at startup, so an invalid entry
/// stops the server instead of silently disappearing from the allow-list.
pub(crate) fn init() -> Result<(), String> {
    let settings = CorsSettings::parse(&std::env::var(CORS_ORIGINS_ENV).unwrap_or_default())
        .map_err(|error| format!("{CORS_ORIGINS_ENV}: {error}"))?;
    match &settings.origins {
        Some(Origins::Any) => tracing::warn!(
            "CORS accepts every origin (`{CORS_ORIGINS_ENV}=*`), without credentials"
        ),
        Some(Origins::List(origins)) => tracing::info!(
            "CORS origins configured: {} entries from {CORS_ORIGINS_ENV}",
            origins.len()
        ),
        None => {}
    }
    let _ = SETTINGS.set(settings);
    Ok(())
}

/// The configured origins as browser-serialized URLs (`*` excluded), for
/// destinations that must stay on this deployment (the social sign-in
/// `return_to`).
pub(crate) fn configured_origin_urls() -> Vec<String> {
    match &settings().origins {
        Some(Origins::List(origins)) => origins.iter().map(Origin::to_url).collect(),
        Some(Origins::Any) | None => Vec::new(),
    }
}

pub fn hyper_configure() -> CorsLayer {
    use http::{Method, header};
    use std::time::Duration;

    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .max_age(Duration::from_secs(3600));

    match &settings().origins {
        Some(Origins::Any) => layer.allow_origin(Any),
        Some(Origins::List(origins)) => layer
            .allow_origin(
                origins
                    .iter()
                    .map(Origin::header_value)
                    .collect::<Vec<HeaderValue>>(),
            )
            .allow_credentials(true),
        None => layer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(raw: &str) -> CorsSettings {
        CorsSettings::parse(raw).unwrap()
    }

    #[test]
    fn empty_and_blank_values_disable_cors() {
        assert_eq!(parsed(""), CorsSettings { origins: None });
        assert_eq!(parsed(" , "), CorsSettings { origins: None });
    }

    #[test]
    fn wildcard_is_only_valid_on_its_own() {
        assert_eq!(
            parsed("*"),
            CorsSettings {
                origins: Some(Origins::Any)
            }
        );
        assert!(CorsSettings::parse("*,https://app.example.com").is_err());
        assert!(CorsSettings::parse("https://app.example.com,*").is_err());
    }

    #[test]
    fn origins_are_validated_and_canonicalized() {
        let settings = parsed("https://app.example.com:443, http://localhost:5173");
        let Some(Origins::List(origins)) = &settings.origins else {
            panic!("expected a list");
        };
        assert_eq!(
            origins.iter().map(Origin::to_url).collect::<Vec<_>>(),
            ["https://app.example.com", "http://localhost:5173"]
        );
    }

    #[test]
    fn invalid_entries_are_errors() {
        for raw in [
            "not-an-origin",
            "https://app.example.com/login",
            "https://app.example.com?x=1",
            "ftp://app.example.com",
            "null",
            "https://user:pass@app.example.com",
        ] {
            assert!(CorsSettings::parse(raw).is_err(), "{raw}");
        }
    }
}
