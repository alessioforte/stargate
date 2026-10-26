use serde::{Deserialize, Serialize};

/// Authorization request parameters. Everything is optional so that a
/// missing or malformed value can be reported to the client's redirect_uri
/// (OIDC Core 3.1.2.6) instead of failing before the client is known.
/// Unrecognized parameters (`display`, `ui_locales`, ...) are ignored as OIDC
/// Core requires.
#[derive(Clone, Debug, Default, Serialize, Deserialize, utoipa::IntoParams)]
pub struct AuthorizationRequest {
    pub response_type: Option<String>,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub nonce: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub audience: Option<String>,
    pub prompt: Option<String>,
    pub max_age: Option<String>,
    pub login_hint: Option<String>,
    /// A previously issued ID token naming the expected user (may be expired).
    pub id_token_hint: Option<String>,
    /// Requested assurance levels, most preferred first (voluntary).
    pub acr_values: Option<String>,
    /// `query` (default) or `form_post`.
    pub response_mode: Option<String>,
    pub request: Option<String>,
    pub request_uri: Option<String>,
    /// Resumes a request parked while the user signed in.
    pub interaction: Option<String>,
}

impl AuthorizationRequest {
    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }

    pub fn redirect_uri(&self) -> Option<&str> {
        self.redirect_uri.as_deref()
    }
}
