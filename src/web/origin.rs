use crate::err::{ErrorCode, ErrorResponse};
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http::header::{AUTHORIZATION, ORIGIN};
use http::{HeaderMap, HeaderValue, Method};
use std::collections::HashSet;
use std::sync::OnceLock;

const TRUSTED_ORIGINS_ENV: &str = "TRUSTED_ORIGINS";
const AUTH_PUBLIC_BASE_URL_ENV: &str = "AUTH_PUBLIC_BASE_URL";

static TRUSTED_ORIGINS: OnceLock<TrustedOrigins> = OnceLock::new();

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
    fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
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

        Ok(Self { origins })
    }

    fn allows_header_origin(&self, raw: &str) -> bool {
        Origin::parse(raw)
            .map(|origin| self.origins.contains(&origin))
            .unwrap_or(false)
    }
}

fn settings() -> Result<TrustedOrigins, String> {
    let mut trusted =
        TrustedOrigins::parse(&std::env::var(TRUSTED_ORIGINS_ENV).unwrap_or_default())
            .map_err(|error| format!("{TRUSTED_ORIGINS_ENV}: {error}"))?;
    trusted.origins.insert(
        origin_of_url(&crate::conf::public_base_url()).map_err(|error| {
            format!("Invalid public base URL for browser origin checks: {error}")
        })?,
    );
    if let Some(auth_app) = nonempty_env(AUTH_PUBLIC_BASE_URL_ENV) {
        trusted.origins.insert(
            origin_of_url(&auth_app)
                .map_err(|error| format!("{AUTH_PUBLIC_BASE_URL_ENV}: {error}"))?,
        );
    }
    Ok(trusted)
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The browser origin of an absolute deployment URL, which may carry a path
/// prefix in front of the routes.
pub(crate) fn origin_of_url(url: &str) -> Result<Origin, String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("invalid URL `{url}`: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("URL `{url}` must use http or https"));
    }
    Origin::parse(&parsed.origin().ascii_serialization())
}

/// The public origin is always trusted, and so is the auth app's when
/// `AUTH_PUBLIC_BASE_URL` is set; configuration adds explicit browser origins.
pub(crate) fn init() -> Result<(), String> {
    let _ = TRUSTED_ORIGINS.set(settings()?);
    Ok(())
}

fn trusted_origins() -> &'static TrustedOrigins {
    TRUSTED_ORIGINS.get_or_init(|| settings().expect("browser origins are validated at startup"))
}

/// Browser-serialized URLs for deployment-bound destinations (social sign-in).
pub(crate) fn configured_origin_urls() -> Vec<String> {
    trusted_origins()
        .origins
        .iter()
        .map(Origin::to_url)
        .collect()
}

/// The guard covers cookie-only requests: a request that carries an
/// `Authorization` header is authenticated by it, is not exposed to CSRF, and
/// never reads the session cookie (`RequestExt::get_token`).
fn origin_decision(
    method: &Method,
    headers: &HeaderMap,
    trusted: &TrustedOrigins,
) -> OriginDecision {
    if method == Method::OPTIONS || headers.contains_key(AUTHORIZATION) {
        return OriginDecision::Allowed;
    }

    let mut values = headers.get_all(ORIGIN).iter();
    let Some(value) = values.next() else {
        // Fetch Metadata §2.3: same-site includes sibling origins, which are
        // not trusted implicitly. An explicit trusted Origin above can permit
        // a hosted app on another origin. Unknown values are ignored per §2.3.
        // https://www.w3.org/TR/fetch-metadata/#sec-fetch-site-header
        return match headers
            .get("sec-fetch-site")
            .and_then(|value| value.to_str().ok())
        {
            Some("same-site" | "cross-site") => OriginDecision::Denied,
            _ => OriginDecision::Allowed,
        };
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
    let mut response = match origin_decision(req.method(), req.headers(), trusted_origins()) {
        OriginDecision::Allowed => next.run(req).await,
        OriginDecision::Denied => {
            ErrorResponse::new(ErrorCode::AuthUntrustedOrigin).into_response()
        }
    };
    // Fetch Metadata §5.1: caches must not reuse a decision for another initiator.
    response.headers_mut().append(
        http::header::VARY,
        HeaderValue::from_static("Origin, Sec-Fetch-Site"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trusted(raw: &str) -> TrustedOrigins {
        TrustedOrigins::parse(raw).unwrap()
    }

    fn decision(origin: Option<&str>, trusted: &TrustedOrigins) -> OriginDecision {
        let mut headers = HeaderMap::new();
        if let Some(origin) = origin {
            headers.insert(ORIGIN, origin.parse().unwrap());
        }
        origin_decision(&Method::POST, &headers, trusted)
    }

    #[test]
    fn empty_config_adds_no_extra_trusted_origins() {
        assert!(TrustedOrigins::parse("").unwrap().origins.is_empty());
        assert!(TrustedOrigins::parse(" , ").unwrap().origins.is_empty());
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
    fn fetch_metadata_rejects_sibling_and_cross_site_requests_without_origin() {
        let trusted = trusted("https://identity.example.com,https://auth.example.com");
        for site in ["same-site", "cross-site"] {
            let mut headers = HeaderMap::new();
            headers.insert("sec-fetch-site", site.parse().unwrap());
            assert_eq!(
                origin_decision(&Method::GET, &headers, &trusted),
                OriginDecision::Denied
            );
            headers.insert(ORIGIN, "https://auth.example.com".parse().unwrap());
            assert_eq!(
                origin_decision(&Method::POST, &headers, &trusted),
                OriginDecision::Allowed
            );
        }
        for site in ["same-origin", "none", "future-value"] {
            let mut headers = HeaderMap::new();
            headers.insert("sec-fetch-site", site.parse().unwrap());
            assert_eq!(
                origin_decision(&Method::POST, &headers, &trusted),
                OriginDecision::Allowed
            );
        }
    }

    #[test]
    fn duplicate_origins_are_not_trusted() {
        let trusted = trusted("https://identity.example.com");
        let mut headers = HeaderMap::new();
        headers.append(ORIGIN, "https://identity.example.com".parse().unwrap());
        headers.append(ORIGIN, "https://identity.example.com".parse().unwrap());
        assert_eq!(
            origin_decision(&Method::POST, &headers, &trusted),
            OriginDecision::Denied
        );
    }

    #[test]
    fn authorization_requests_skip_the_origin_check() {
        let trusted = trusted("https://identity.example.com");
        for value in [
            "Bearer a-token",
            "Bearer ",
            "Basic dXNlcjpwYXNz",
            "not-a-scheme",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(ORIGIN, "https://evil.example.com".parse().unwrap());
            headers.insert("sec-fetch-site", "cross-site".parse().unwrap());
            headers.insert(AUTHORIZATION, value.parse().unwrap());
            assert_eq!(
                origin_decision(&Method::POST, &headers, &trusted),
                OriginDecision::Allowed,
                "{value}"
            );
        }
    }

    #[test]
    fn the_public_origin_guard_is_enabled_without_extra_configuration() {
        const CHILD: &str = "STARGATE_ORIGIN_GUARD_TEST";
        if std::env::var_os(CHILD).is_none() {
            let output = crate::conf::child_test::run(
                "web::origin::tests::the_public_origin_guard_is_enabled_without_extra_configuration",
                |command| {
                    command
                        .env(CHILD, "1")
                        .env_remove("TRUSTED_ORIGINS")
                        .env("OAUTH_BASE_URL", "https://identity.example.com/identity");
                },
            );
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        init().unwrap();
        let trusted = trusted_origins();
        assert_eq!(
            decision(Some("https://identity.example.com"), trusted),
            OriginDecision::Allowed
        );
        assert_eq!(
            decision(Some("https://evil.example.com"), trusted),
            OriginDecision::Denied
        );
    }

    #[test]
    fn the_auth_app_origin_is_trusted_too() {
        const CHILD: &str = "STARGATE_AUTH_ORIGIN_GUARD_TEST";
        if std::env::var_os(CHILD).is_none() {
            let output = crate::conf::child_test::run(
                "web::origin::tests::the_auth_app_origin_is_trusted_too",
                |command| {
                    command
                        .env(CHILD, "1")
                        .env_remove("TRUSTED_ORIGINS")
                        .env("OAUTH_BASE_URL", "https://identity.example.com")
                        .env("AUTH_PUBLIC_BASE_URL", "https://auth.example.com/login");
                },
            );
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        init().unwrap();
        let trusted = trusted_origins();
        assert_eq!(
            decision(Some("https://auth.example.com"), trusted),
            OriginDecision::Allowed
        );
        assert_eq!(
            decision(Some("https://identity.example.com"), trusted),
            OriginDecision::Allowed
        );
        assert_eq!(
            decision(Some("https://evil.example.com"), trusted),
            OriginDecision::Denied
        );
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
