//! Capabilities an operator can switch off, read once at startup.
//!
//! A disabled capability keeps its routes mounted and answers
//! `404 feature.disabled`: unmatched paths fall through to the gateway, which
//! could proxy them to an upstream.

use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::env::{EnvError, parse_bool};
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::fmt;
use std::sync::OnceLock;

/// A capability that can be off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    /// Serving the Console app (`CONSOLE_ENABLED`).
    Console,
    /// Social sign-in with any provider (`/oauth/state`).
    Social,
    SocialGoogle,
    SocialGithub,
}

impl Feature {
    /// The name reported in `feature.disabled` errors.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::Social => "social",
            Self::SocialGoogle => "social_google",
            Self::SocialGithub => "social_github",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Features {
    pub console: bool,
    /// On when both the client id and secret are set.
    pub social_google: bool,
    pub social_github: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum FeaturesError {
    #[error(transparent)]
    Env(#[from] EnvError),
    #[error("`{set}` is set but `{missing}` is not: set both to enable the provider, or neither")]
    IncompleteProvider {
        set: &'static str,
        missing: &'static str,
    },
}

impl Features {
    pub fn from_env() -> Result<Self, FeaturesError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, FeaturesError> {
        let var = |name: &str| {
            lookup(name)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let flag = |name: &'static str, default: bool| match var(name) {
            None => Ok(default),
            Some(value) => parse_bool(&value).ok_or_else(|| EnvError::Parse {
                name: name.to_string(),
                value,
                message: "expected true or false".to_string(),
            }),
        };
        let provider = |id: &'static str, secret: &'static str| match (var(id), var(secret)) {
            (Some(_), Some(_)) => Ok(true),
            (None, None) => Ok(false),
            (Some(_), None) => Err(FeaturesError::IncompleteProvider {
                set: id,
                missing: secret,
            }),
            (None, Some(_)) => Err(FeaturesError::IncompleteProvider {
                set: secret,
                missing: id,
            }),
        };
        Ok(Self {
            console: flag("CONSOLE_ENABLED", true)?,
            social_google: provider("GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET")?,
            social_github: provider("GITHUB_OAUTH_CLIENT_ID", "GITHUB_OAUTH_CLIENT_SECRET")?,
        })
    }

    pub fn enabled(&self, feature: Feature) -> bool {
        match feature {
            Feature::Console => self.console,
            Feature::Social => self.social_google || self.social_github,
            Feature::SocialGoogle => self.social_google,
            Feature::SocialGithub => self.social_github,
        }
    }

    /// The enabled social sign-in providers, as the hosted apps name them.
    pub fn social_providers(&self) -> Vec<&'static str> {
        [
            ("google", self.social_google),
            ("github", self.social_github),
        ]
        .into_iter()
        .filter_map(|(name, enabled)| enabled.then_some(name))
        .collect()
    }
}

impl fmt::Display for Features {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let on_off = |enabled: bool| if enabled { "on" } else { "off" };
        write!(
            f,
            "console={} social=[{}]",
            on_off(self.console),
            self.social_providers().join(", ")
        )
    }
}

static FEATURES: OnceLock<Features> = OnceLock::new();

/// Reads and validates the features; the server calls it once at startup, so
/// an invalid setting stops it before it serves anything.
pub fn init() -> Result<&'static Features, FeaturesError> {
    let features = Features::from_env()?;
    let features = FEATURES.get_or_init(|| features);
    tracing::info!("Features: {features}");
    Ok(features)
}

/// The features in effect. Outside the server (tests, CLI commands) they are
/// read on first use.
pub fn features() -> &'static Features {
    FEATURES
        .get_or_init(|| Features::from_env().expect("feature settings are validated at startup"))
}

pub fn disabled(feature: Feature) -> ErrorResponse {
    ErrorResponse::new(ErrorCode::FeatureDisabled).with_param("feature", feature.name())
}

/// Route middleware: `404 feature.disabled` unless `feature` is on. Mount it
/// with `from_fn_with_state(feature, require_feature)`.
pub async fn require_feature(
    State(feature): State<Feature>,
    request: Request,
    next: Next,
) -> Response {
    if features().enabled(feature) {
        next.run(request).await
    } else {
        disabled(feature).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn features_from(vars: &[(&str, &str)]) -> Result<Features, FeaturesError> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        Features::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn defaults_serve_the_console_without_social_providers() {
        let features = features_from(&[]).unwrap();
        assert_eq!(
            features,
            Features {
                console: true,
                social_google: false,
                social_github: false,
            }
        );
        assert!(!features.enabled(Feature::Social));
        assert_eq!(features.to_string(), "console=on social=[]");
    }

    #[test]
    fn flags_are_parsed_strictly() {
        for (value, expected) in [
            ("false", false),
            ("OFF", false),
            ("0", false),
            ("yes", true),
        ] {
            let features = features_from(&[("CONSOLE_ENABLED", value)]).unwrap();
            assert_eq!(features.console, expected, "{value}");
        }
        // Blank is unset, so the default applies.
        assert!(features_from(&[("CONSOLE_ENABLED", "  ")]).unwrap().console);

        let error = features_from(&[("CONSOLE_ENABLED", "flase")]).unwrap_err();
        assert!(error.to_string().contains("CONSOLE_ENABLED"), "{error}");
    }

    #[test]
    fn a_provider_needs_both_its_client_id_and_secret() {
        let features = features_from(&[
            ("GITHUB_OAUTH_CLIENT_ID", "id"),
            ("GITHUB_OAUTH_CLIENT_SECRET", "secret"),
        ])
        .unwrap();
        assert!(features.enabled(Feature::SocialGithub));
        assert!(!features.enabled(Feature::SocialGoogle));
        assert!(features.enabled(Feature::Social));
        assert_eq!(features.social_providers(), ["github"]);

        let error = features_from(&[("GOOGLE_OAUTH_CLIENT_ID", "id")]).unwrap_err();
        assert!(
            matches!(
                error,
                FeaturesError::IncompleteProvider {
                    set: "GOOGLE_OAUTH_CLIENT_ID",
                    missing: "GOOGLE_OAUTH_CLIENT_SECRET",
                }
            ),
            "{error}"
        );
        assert!(features_from(&[("GITHUB_OAUTH_CLIENT_SECRET", "secret")]).is_err());
    }

    #[test]
    fn disabled_features_answer_not_found() {
        let error = disabled(Feature::SocialGoogle);
        assert_eq!(error.code, ErrorCode::FeatureDisabled);
        assert_eq!(error.into_response().status(), http::StatusCode::NOT_FOUND);
    }
}
