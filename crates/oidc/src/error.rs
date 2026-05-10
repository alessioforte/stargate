use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthErrorCode {
    InvalidRequest,
    InvalidClient,
    InvalidGrant,
    UnauthorizedClient,
    UnsupportedGrantType,
    InvalidScope,
    InsufficientScope,
    ServerError,
}

impl OAuthErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidClient => "invalid_client",
            Self::InvalidGrant => "invalid_grant",
            Self::UnauthorizedClient => "unauthorized_client",
            Self::UnsupportedGrantType => "unsupported_grant_type",
            Self::InvalidScope => "invalid_scope",
            Self::InsufficientScope => "insufficient_scope",
            Self::ServerError => "server_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{description}")]
pub struct OAuthError {
    pub code: OAuthErrorCode,
    pub description: String,
    pub error_uri: Option<String>,
}

impl OAuthError {
    pub fn new(code: OAuthErrorCode, description: impl Into<String>) -> Self {
        Self {
            code,
            description: description.into(),
            error_uri: None,
        }
    }

    pub fn invalid_request(description: impl Into<String>) -> Self {
        Self::new(OAuthErrorCode::InvalidRequest, description)
    }

    pub fn invalid_client(description: impl Into<String>) -> Self {
        Self::new(OAuthErrorCode::InvalidClient, description)
    }

    pub fn invalid_grant(description: impl Into<String>) -> Self {
        Self::new(OAuthErrorCode::InvalidGrant, description)
    }

    pub fn unauthorized_client(description: impl Into<String>) -> Self {
        Self::new(OAuthErrorCode::UnauthorizedClient, description)
    }

    pub fn unsupported_grant_type(description: impl Into<String>) -> Self {
        Self::new(OAuthErrorCode::UnsupportedGrantType, description)
    }

    pub fn invalid_scope(description: impl Into<String>) -> Self {
        Self::new(OAuthErrorCode::InvalidScope, description)
    }

    pub fn insufficient_scope(description: impl Into<String>) -> Self {
        Self::new(OAuthErrorCode::InsufficientScope, description)
    }

    pub fn server_error(error: impl fmt::Display) -> Self {
        tracing::error!("oauth server_error: {error}");
        Self::new(OAuthErrorCode::ServerError, "server error")
    }

    pub fn with_uri(mut self, error_uri: impl Into<String>) -> Self {
        self.error_uri = Some(error_uri.into());
        self
    }
}

impl From<store::StoreError> for OAuthError {
    fn from(error: store::StoreError) -> Self {
        Self::server_error(error)
    }
}
