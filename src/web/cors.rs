//! CORS for the IAM planes (`CORS_ORIGINS`), read and validated once at
//! startup like the other process-wide settings. The Console's public origin
//! is admitted on the token, revocation, message-catalog, and profile
//! endpoints, the cross-origin calls a Console signed in on another origin
//! makes without the session cookie.

use crate::web::origin::Origin;
use http::HeaderValue;
use std::sync::OnceLock;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

const CORS_ORIGINS_ENV: &str = "CORS_ORIGINS";
const CONSOLE_PUBLIC_URL_ENV: &str = "CONSOLE_PUBLIC_URL";

/// Cross-origin Console calls that never carry the session cookie: the OAuth
/// token and revocation endpoints, the API message catalog, and the profile
/// preference (bearer-authenticated).
const CONSOLE_CORS_PATHS: [&str; 3] = ["/oauth/token", "/oauth/revoke", "/account/profile"];

static SETTINGS: OnceLock<CorsSettings> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
enum Origins {
    /// `CORS_ORIGINS=*`: every origin, without credentials.
    Any,
    List(Vec<Origin>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CorsSettings {
    origins: Option<Origins>,
    /// `CONSOLE_PUBLIC_URL`, else the public base URL.
    console: Origin,
}

impl CorsSettings {
    fn parse(raw: &str, console: Origin) -> Result<Self, String> {
        Ok(Self {
            origins: parse_origins(raw)?,
            console,
        })
    }

    fn from_env() -> Result<Self, String> {
        let console = crate::web::origin::origin_of_url(&crate::conf::apps::console_public_url())
            .map_err(|error| format!("{CONSOLE_PUBLIC_URL_ENV}: {error}"))?;
        Self::parse(
            &std::env::var(CORS_ORIGINS_ENV).unwrap_or_default(),
            console,
        )
        .map_err(|error| format!("{CORS_ORIGINS_ENV}: {error}"))
    }

    /// Configured origins are allowed on every IAM path; the Console's origin
    /// only on the endpoints its browser app calls cross-origin.
    fn allows(&self, origin: &HeaderValue, path: &str) -> bool {
        match &self.origins {
            Some(Origins::Any) => true,
            Some(Origins::List(origins))
                if origins
                    .iter()
                    .any(|allowed| allowed.header_value() == *origin) =>
            {
                true
            }
            _ => console_cors_path(path) && self.console.header_value() == *origin,
        }
    }
}

/// The Console origin is admitted only on its own cross-origin paths, never
/// on another IAM route: the two OAuth endpoints, the message catalog, and
/// the profile preference.
fn console_cors_path(path: &str) -> bool {
    CONSOLE_CORS_PATHS.contains(&path)
        || path
            .strip_prefix("/i18n/")
            .is_some_and(|locale| !locale.is_empty() && !locale.contains('/'))
}

/// The origins a browser may call the IAM planes from, plus the Console's
/// origin on its own cross-origin paths.
fn settings() -> &'static CorsSettings {
    SETTINGS
        .get_or_init(|| CorsSettings::from_env().expect("CORS settings are validated at startup"))
}

fn parse_origins(raw: &str) -> Result<Option<Origins>, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    if raw == "*" {
        return Ok(Some(Origins::Any));
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
        Ok(None)
    } else {
        Ok(Some(Origins::List(origins)))
    }
}

/// Reads and validates `CORS_ORIGINS` and `CONSOLE_PUBLIC_URL` once at
/// startup, so an invalid entry stops the server instead of silently
/// disappearing from the allow-list.
pub(crate) fn init() -> Result<(), String> {
    let settings = CorsSettings::from_env()?;
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
    tracing::info!(
        "CORS admits the Console origin {} on its token, revocation, catalog, and profile endpoints ({CONSOLE_PUBLIC_URL_ENV})",
        settings.console.to_url()
    );
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
    layer(settings())
}

fn layer(settings: &CorsSettings) -> CorsLayer {
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

    match &settings.origins {
        Some(Origins::Any) => layer.allow_origin(Any),
        Some(Origins::List(_)) => layer
            .allow_origin(origin_predicate(settings))
            .allow_credentials(true),
        None => layer.allow_origin(origin_predicate(settings)),
    }
}

fn origin_predicate(settings: &CorsSettings) -> AllowOrigin {
    let settings = settings.clone();
    AllowOrigin::predicate(move |origin, parts| settings.allows(origin, parts.uri.path()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::get;
    use http::{Request, header};
    use tower::ServiceExt;

    const CONSOLE: &str = "https://console.example.com";
    const PUBLIC: &str = "https://identity.example.com";

    fn parsed(raw: &str) -> CorsSettings {
        CorsSettings::parse(raw, console_origin()).unwrap()
    }

    fn console_origin() -> Origin {
        crate::web::origin::origin_of_url(CONSOLE).unwrap()
    }

    fn header(value: &str) -> HeaderValue {
        value.parse().unwrap()
    }

    #[test]
    fn empty_and_blank_values_disable_cors() {
        assert_eq!(parsed("").origins, None);
        assert_eq!(parsed(" , ").origins, None);
    }

    #[test]
    fn wildcard_is_only_valid_on_its_own() {
        assert_eq!(parsed("*").origins, Some(Origins::Any));
        assert!(CorsSettings::parse("*,https://app.example.com", console_origin()).is_err());
        assert!(CorsSettings::parse("https://app.example.com,*", console_origin()).is_err());
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
            assert!(CorsSettings::parse(raw, console_origin()).is_err(), "{raw}");
        }
        assert!(crate::web::origin::origin_of_url("not-a-console-url").is_err());
    }

    #[test]
    fn configured_origins_are_allowed_on_every_path() {
        let settings = parsed("https://app.example.com");
        for path in [
            "/admin/me",
            "/oauth/token",
            "/account/profile",
            "/account/consents",
        ] {
            assert!(
                settings.allows(&header("https://app.example.com"), path),
                "{path}"
            );
        }
        for path in ["/admin/me", "/account/consents", "/oauth/authorize"] {
            assert!(!settings.allows(&header(CONSOLE), path), "{path}");
        }
    }

    #[test]
    fn the_console_origin_is_allowed_only_on_its_paths() {
        let settings = parsed("");
        for path in [
            "/oauth/token",
            "/oauth/revoke",
            "/i18n/en",
            "/i18n/it",
            "/account/profile",
        ] {
            assert!(settings.allows(&header(CONSOLE), path), "{path}");
        }
        for path in [
            "/i18n/",
            "/i18n/en/extra",
            "/admin/me",
            "/account/consents",
            "/oauth/authorize",
            "/oauth/login",
        ] {
            assert!(!settings.allows(&header(CONSOLE), path), "{path}");
        }
        assert!(!settings.allows(&header("https://evil.example.com"), "/oauth/token"));
    }

    #[test]
    fn wildcard_allows_any_origin_on_any_path() {
        let settings = parsed("*");
        for path in ["/admin/me", "/oauth/token"] {
            assert!(settings.allows(&header("https://evil.example.com"), path));
        }
    }

    #[tokio::test]
    async fn the_layer_answers_the_console_on_its_paths_and_preflights() {
        let settings = parsed("");
        let app = axum::Router::new()
            .route("/oauth/token", get(|| async { "token" }))
            .route("/i18n/{locale}", get(|| async { "catalog" }))
            .route(
                "/account/profile",
                get(|| async { "profile" }).put(|| async { "profile" }),
            )
            .route("/account/consents", get(|| async { "consents" }))
            .layer(layer(&settings));

        for (method, path) in [
            ("GET", "/oauth/token"),
            ("GET", "/i18n/en"),
            ("GET", "/account/profile"),
            ("PUT", "/account/profile"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header(header::ORIGIN, CONSOLE)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
                CONSOLE,
                "{method} {path}"
            );
        }

        for (path, requested) in [
            ("/oauth/token", "POST"),
            ("/oauth/revoke", "POST"),
            ("/i18n/en", "GET"),
            ("/account/profile", "PUT"),
        ] {
            let preflight = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("OPTIONS")
                        .uri(path)
                        .header(header::ORIGIN, CONSOLE)
                        .header(header::ACCESS_CONTROL_REQUEST_METHOD, requested)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                preflight.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
                CONSOLE,
                "{path}"
            );
            assert!(
                preflight
                    .headers()
                    .contains_key(header::ACCESS_CONTROL_ALLOW_METHODS),
                "{path}"
            );
        }

        let denied = app
            .oneshot(
                Request::get("/account/consents")
                    .header(header::ORIGIN, CONSOLE)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            !denied
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        );
    }

    #[test]
    fn the_console_origin_defaults_to_the_public_base_url() {
        let public = crate::web::origin::origin_of_url(PUBLIC).unwrap();
        let parsed = CorsSettings::parse("", public.clone()).unwrap();
        assert_eq!(parsed.console, public);
    }
}
