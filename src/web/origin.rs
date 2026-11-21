use crate::err::{ErrorCode, ErrorResponse};
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::header::ORIGIN;
use http::{HeaderMap, HeaderValue, Method};
use std::collections::HashSet;
use std::sync::OnceLock;

const TRUSTED_ORIGINS_ENV: &str = "TRUSTED_ORIGINS";

static TRUSTED_ORIGINS: OnceLock<Option<TrustedOrigins>> = OnceLock::new();

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Origin {
    scheme: String,
    host: String,
    port: u16,
}

#[derive(Clone, Debug)]
struct TrustedOrigins {
    origins: HashSet<Origin>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OriginDecision {
    Allowed,
    Denied,
}

impl Origin {
    pub(crate) fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("null") {
            return Err("null origin is not trusted".to_string());
        }

        let url = url::Url::parse(raw).map_err(|_| format!("invalid origin `{raw}`"))?;
        let scheme = url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(format!("origin `{raw}` must use http or https"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(format!("origin `{raw}` must not contain credentials"));
        }
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(format!(
                "origin `{raw}` must not contain path, query, or fragment"
            ));
        }

        let host = url
            .host_str()
            .filter(|host| !host.is_empty())
            .ok_or_else(|| format!("origin `{raw}` must contain a host"))?
            .to_ascii_lowercase();
        if host.contains('*') {
            return Err(format!("origin `{raw}` must not contain a wildcard"));
        }
        let port = url
            .port_or_known_default()
            .ok_or_else(|| format!("origin `{raw}` must contain a port"))?;

        Ok(Self {
            scheme: scheme.to_string(),
            host,
            port,
        })
    }

    /// The origin as a browser serializes it: a default port is omitted.
    pub(crate) fn to_url(&self) -> String {
        let default_port = match self.scheme.as_str() {
            "http" => 80,
            "https" => 443,
            _ => 0,
        };
        if self.port == default_port {
            format!("{}://{}", self.scheme, self.host)
        } else {
            format!("{}://{}:{}", self.scheme, self.host, self.port)
        }
    }

    pub(crate) fn header_value(&self) -> HeaderValue {
        HeaderValue::from_str(&self.to_url())
            .expect("a parsed origin is always a valid HTTP header value")
    }
}

impl TrustedOrigins {
    fn parse(raw: &str) -> Result<Option<Self>, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(None);
        }

        let mut origins = HashSet::new();
        for entry in raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if entry == "*" {
                return Err("wildcard `*` is not allowed".to_string());
            }
            origins.insert(Origin::parse(entry)?);
        }

        if origins.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Self { origins }))
        }
    }

    fn allows_header_origin(&self, raw: &str) -> bool {
        Origin::parse(raw)
            .map(|origin| self.origins.contains(&origin))
            .unwrap_or(false)
    }
}

/// Reads and validates `TRUSTED_ORIGINS`; the server calls it once at startup,
/// so an invalid value stops it instead of failing every sensitive request.
pub(crate) fn init() -> Result<(), String> {
    let raw = std::env::var(TRUSTED_ORIGINS_ENV).unwrap_or_default();
    let trusted =
        TrustedOrigins::parse(&raw).map_err(|error| format!("{TRUSTED_ORIGINS_ENV}: {error}"))?;
    if let Some(trusted) = &trusted {
        tracing::info!(
            "Trusted browser origins configured: {} entries from {TRUSTED_ORIGINS_ENV}",
            trusted.origins.len()
        );
    }
    let _ = TRUSTED_ORIGINS.set(trusted);
    Ok(())
}

/// The validated origins. Outside the server (tests) they are read on first use.
fn trusted_origins() -> Option<&'static TrustedOrigins> {
    TRUSTED_ORIGINS
        .get_or_init(|| {
            TrustedOrigins::parse(&std::env::var(TRUSTED_ORIGINS_ENV).unwrap_or_default())
                .expect("TRUSTED_ORIGINS is validated at startup")
        })
        .as_ref()
}

/// The trusted origins as browser-serialized URLs, for destinations that must
/// stay on this deployment (the social sign-in `return_to`).
pub(crate) fn configured_origin_urls() -> Vec<String> {
    trusted_origins()
        .map(|trusted| trusted.origins.iter().map(Origin::to_url).collect())
        .unwrap_or_default()
}

fn origin_decision(
    method: &Method,
    headers: &HeaderMap,
    trusted: &TrustedOrigins,
) -> OriginDecision {
    if method == Method::OPTIONS {
        return OriginDecision::Allowed;
    }

    let mut values = headers.get_all(ORIGIN).iter();
    let Some(value) = values.next() else {
        return OriginDecision::Allowed;
    };
    if values.next().is_some() {
        return OriginDecision::Denied;
    }

    let Ok(origin) = value.to_str() else {
        return OriginDecision::Denied;
    };

    if trusted.allows_header_origin(origin) {
        OriginDecision::Allowed
    } else {
        OriginDecision::Denied
    }
}

pub async fn trusted_origin_middleware(req: Request, next: Next) -> Response {
    let Some(trusted) = trusted_origins() else {
        return next.run(req).await;
    };

    match origin_decision(req.method(), req.headers(), trusted) {
        OriginDecision::Allowed => next.run(req).await,
        OriginDecision::Denied => {
            ErrorResponse::new(ErrorCode::AuthUntrustedOrigin).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trusted(raw: &str) -> TrustedOrigins {
        TrustedOrigins::parse(raw).unwrap().unwrap()
    }

    fn decision(origin: Option<&str>, trusted: &TrustedOrigins) -> OriginDecision {
        let mut headers = HeaderMap::new();
        if let Some(origin) = origin {
            headers.insert(ORIGIN, origin.parse().unwrap());
        }
        origin_decision(&Method::POST, &headers, trusted)
    }

    #[test]
    fn empty_config_disables_guard() {
        assert!(TrustedOrigins::parse("").unwrap().is_none());
        assert!(TrustedOrigins::parse(" , ").unwrap().is_none());
    }

    #[test]
    fn accepts_exact_configured_origin() {
        let trusted = trusted("https://app.example.com,http://localhost:3000");
        assert_eq!(
            decision(Some("https://app.example.com"), &trusted),
            OriginDecision::Allowed
        );
        assert_eq!(
            decision(Some("http://localhost:3000"), &trusted),
            OriginDecision::Allowed
        );
    }

    #[test]
    fn treats_default_ports_as_same_origin() {
        let trusted = trusted("https://app.example.com:443,http://localhost:80");
        assert_eq!(
            decision(Some("https://app.example.com"), &trusted),
            OriginDecision::Allowed
        );
        assert_eq!(
            decision(Some("http://localhost"), &trusted),
            OriginDecision::Allowed
        );
    }

    #[test]
    fn rejects_untrusted_or_invalid_origin_header() {
        let trusted = trusted("https://app.example.com");
        assert_eq!(
            decision(Some("https://evil.example.com"), &trusted),
            OriginDecision::Denied
        );
        assert_eq!(decision(Some("null"), &trusted), OriginDecision::Denied);
    }

    #[test]
    fn allows_missing_origin_and_options_preflight() {
        let trusted = trusted("https://app.example.com");
        let headers = HeaderMap::new();
        assert_eq!(
            origin_decision(&Method::POST, &headers, &trusted),
            OriginDecision::Allowed
        );
        assert_eq!(
            origin_decision(&Method::OPTIONS, &headers, &trusted),
            OriginDecision::Allowed
        );
    }

    #[test]
    fn rejects_invalid_config_entries() {
        assert!(TrustedOrigins::parse("*").is_err());
        assert!(TrustedOrigins::parse("https://*.example.com").is_err());
        assert!(TrustedOrigins::parse("https://app.example.com/login").is_err());
        assert!(TrustedOrigins::parse("ftp://app.example.com").is_err());
    }

    #[test]
    fn origins_serialize_as_browsers_send_them() {
        assert_eq!(
            Origin::parse("https://app.example.com:443")
                .unwrap()
                .to_url(),
            "https://app.example.com"
        );
        assert_eq!(
            Origin::parse("http://[::1]:8080").unwrap().to_url(),
            "http://[::1]:8080"
        );
    }
}
