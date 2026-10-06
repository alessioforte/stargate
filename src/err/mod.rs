pub mod types;

pub use types::{ErrorCode, ErrorType};

use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use utoipa::ToSchema;

#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct ErrorResponse {
    #[serde(skip)]
    #[schema(ignore)]
    pub status: StatusCode,
    #[serde(skip)]
    #[schema(ignore)]
    headers: Vec<(String, String)>,
    #[schema(example = "User not found")]
    pub message: String,
    #[schema(value_type = String, example = "user.not_found")]
    pub code: ErrorCode,
    #[serde(rename = "type")]
    pub error_type: ErrorType,
    #[schema(example = "https://docs.stargate.dev/errors/user.not_found")]
    pub link: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, Value>,
}

impl ErrorResponse {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            status: code.status(),
            headers: Vec::new(),
            message: code.message().to_string(),
            code,
            error_type: code.error_type(),
            link: code.url(),
            params: BTreeMap::new(),
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        if !self.status.is_server_error() {
            self.message = message.into();
        }
        self
    }

    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    pub fn internal(error: impl fmt::Display) -> Self {
        tracing::error!(error = %error, "internal request failure");
        Self::new(ErrorCode::SystemInternal)
    }

    pub fn insert_header(&mut self, key: &str, value: &str) -> &mut Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }

    fn body_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| {
            br#"{"message":"Internal server error","code":"system.internal","type":"internal","link":"https://docs.stargate.dev/errors/system.internal"}"#.to_vec()
        })
    }
}

impl From<ErrorCode> for ErrorResponse {
    fn from(code: ErrorCode) -> Self {
        Self::new(code)
    }
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for ErrorResponse {}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct OAuthErrorResponse {
    #[serde(skip)]
    #[schema(ignore)]
    pub status: StatusCode,
    #[serde(skip)]
    #[schema(ignore)]
    headers: Vec<(String, String)>,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_uri: Option<String>,
}

impl OAuthErrorResponse {
    pub fn from_oauth_error(status: StatusCode, error: oidc::OAuthError) -> Self {
        Self {
            status,
            headers: Vec::new(),
            error: error.code.as_str().to_string(),
            error_description: Some(error.description),
            error_uri: error.error_uri,
        }
    }

    pub fn from_code(
        status: StatusCode,
        code: oidc::OAuthErrorCode,
        description: impl Into<String>,
    ) -> Self {
        Self::from_oauth_error(status, oidc::OAuthError::new(code, description))
    }

    pub fn bad_request(description: impl Into<String>) -> Self {
        Self::from_oauth_error(
            StatusCode::BAD_REQUEST,
            oidc::OAuthError::invalid_request(description),
        )
    }

    pub fn invalid_client(description: impl Into<String>) -> Self {
        let mut error = Self::from_oauth_error(
            StatusCode::UNAUTHORIZED,
            oidc::OAuthError::invalid_client(description),
        );
        error.insert_header("WWW-Authenticate", "Basic realm=\"stargate-oauth-token\"");
        error
    }

    pub fn invalid_grant(description: impl Into<String>) -> Self {
        Self::from_oauth_error(
            StatusCode::BAD_REQUEST,
            oidc::OAuthError::invalid_grant(description),
        )
    }

    pub fn unsupported_grant_type(description: impl Into<String>) -> Self {
        Self::from_oauth_error(
            StatusCode::BAD_REQUEST,
            oidc::OAuthError::unsupported_grant_type(description),
        )
    }

    pub fn insufficient_scope(description: impl Into<String>) -> Self {
        Self::from_code(
            StatusCode::FORBIDDEN,
            oidc::OAuthErrorCode::InsufficientScope,
            description,
        )
    }

    pub fn internal(error: impl fmt::Display) -> Self {
        tracing::error!(error = %error, "OAuth request failure");
        Self::from_oauth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            oidc::OAuthError::server_error("Internal server error"),
        )
    }

    pub fn insert_header(&mut self, key: &str, value: &str) -> &mut Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }

    pub fn description(&self) -> &str {
        self.error_description
            .as_deref()
            .unwrap_or(self.error.as_str())
    }

    fn body_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| {
            br#"{"error":"server_error","error_description":"server error"}"#.to_vec()
        })
    }
}

impl fmt::Display for OAuthErrorResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.description())
    }
}

impl std::error::Error for OAuthErrorResponse {}

impl From<oidc::OAuthError> for OAuthErrorResponse {
    fn from(error: oidc::OAuthError) -> Self {
        match error.code {
            oidc::OAuthErrorCode::InvalidClient => Self::invalid_client(error.description),
            oidc::OAuthErrorCode::ServerError => Self::internal(error.description),
            oidc::OAuthErrorCode::InvalidGrant
            | oidc::OAuthErrorCode::InvalidRequest
            | oidc::OAuthErrorCode::InvalidScope
            | oidc::OAuthErrorCode::UnauthorizedClient
            | oidc::OAuthErrorCode::UnsupportedGrantType => {
                Self::from_oauth_error(StatusCode::BAD_REQUEST, error)
            }
            oidc::OAuthErrorCode::InsufficientScope => {
                Self::from_oauth_error(StatusCode::FORBIDDEN, error)
            }
        }
    }
}

impl From<OAuthErrorResponse> for ErrorResponse {
    fn from(error: OAuthErrorResponse) -> Self {
        let code = match error.status {
            StatusCode::UNAUTHORIZED => ErrorCode::AuthTokenInvalid,
            StatusCode::FORBIDDEN => ErrorCode::AuthInsufficientPermissions,
            StatusCode::INTERNAL_SERVER_ERROR => ErrorCode::SystemInternal,
            _ => ErrorCode::RequestInvalid,
        };
        let mut response = Self::new(code).with_message(error.description());
        for (key, value) in error.headers {
            response.insert_header(&key, &value);
        }
        response
    }
}

impl From<oidc::OAuthError> for ErrorResponse {
    fn from(error: oidc::OAuthError) -> Self {
        OAuthErrorResponse::from(error).into()
    }
}

impl axum::response::IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        use http::header::{CONTENT_TYPE, HeaderName, HeaderValue, RETRY_AFTER};

        if self.status.is_server_error() {
            tracing::error!(code = %self.code, message = %self.message, "request failed");
        } else {
            tracing::debug!(code = %self.code, message = %self.message, "request rejected");
        }

        let body = self.body_json();
        let mut builder = http::Response::builder()
            .status(self.status)
            .header(CONTENT_TYPE, "application/json");

        if self.status == StatusCode::SERVICE_UNAVAILABLE {
            builder = builder.header(
                RETRY_AFTER,
                crate::etc::http::headers::retry_after_header_value(Some(
                    std::time::Duration::from_secs(10),
                )),
            );
        }

        for (key, value) in &self.headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(key.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                builder = builder.header(name, value);
            }
        }

        builder
            .body(axum::body::Body::from(body))
            .unwrap_or_else(|_| {
                let mut response = axum::response::Response::new(axum::body::Body::from(
                    br#"{"message":"Internal server error","code":"system.internal","type":"internal","link":"https://docs.stargate.dev/errors/system.internal"}"#.as_slice(),
                ));
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                response
                    .headers_mut()
                    .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                response
            })
    }
}

impl axum::response::IntoResponse for OAuthErrorResponse {
    fn into_response(self) -> axum::response::Response {
        use http::header::{CONTENT_TYPE, HeaderName, HeaderValue};

        let body = self.body_json();
        let mut builder = http::Response::builder()
            .status(self.status)
            .header(CONTENT_TYPE, "application/json");

        for (key, value) in &self.headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(key.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                builder = builder.header(name, value);
            }
        }

        builder
            .body(axum::body::Body::from(body))
            .unwrap_or_else(|_| {
                let mut response = axum::response::Response::new(axum::body::Body::from(
                    br#"{"error":"server_error","error_description":"server error"}"#.as_slice(),
                ));
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                response
                    .headers_mut()
                    .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                response
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn error_codes_are_unique_and_complete() {
        assert_eq!(ErrorCode::ALL.len(), ErrorCode::SystemInternal as usize + 1);
        let mut names = HashSet::new();
        for code in ErrorCode::ALL {
            assert!(names.insert(code.as_str()), "duplicate code: {code}");
            assert!(!code.message().is_empty(), "missing message: {code}");
        }
    }

    #[test]
    fn error_response_uses_code_metadata() {
        let response = ErrorResponse::new(ErrorCode::UserNotFound)
            .with_param("id", "usr_123")
            .with_message("User 'usr_123' not found");
        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["code"], "user.not_found");
        assert_eq!(json["type"], "not_found");
        assert_eq!(json["message"], "User 'usr_123' not found");
        assert_eq!(json["params"]["id"], "usr_123");
        assert!(
            json["link"]
                .as_str()
                .unwrap()
                .ends_with("/errors/user.not_found")
        );
    }

    #[test]
    fn internal_errors_do_not_expose_the_cause() {
        let response = ErrorResponse::internal("database password leaked")
            .with_message("database password leaked");
        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["code"], "system.internal");
        assert_eq!(json["message"], "Internal server error");
        assert!(!json.to_string().contains("database password leaked"));
    }

    #[test]
    fn oauth_internal_errors_do_not_expose_the_cause() {
        let response = OAuthErrorResponse::internal("database password leaked");
        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["error"], "server_error");
        assert_eq!(json["error_description"], "server error");
        assert!(!json.to_string().contains("database password leaked"));
    }
}
