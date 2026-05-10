pub mod types;

use http::StatusCode;
use serde::{Deserialize, Serialize};
use std::fmt;
use types::{Code, ErrorCode};
use utoipa::ToSchema;

#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("A Content-Type header is missing. Accepted values for the Content-Type header are: {}",
                .0.iter().map(|s| format!("`{}`", s)).collect::<Vec<_>>().join(", "))]
    MissingContentType(Vec<String>),
    #[error(
            "The Content-Type `{0}` is invalid. Accepted values for the Content-Type header are: {x}",
            x = .1.iter().map(|s| format!("`{}`", s)).collect::<Vec<_>>().join(", ")
        )]
    InvalidContentType(String, Vec<String>),
    #[error("Document `{0}` not found.")]
    DocumentNotFound(String),
    #[error("A {0} payload is missing.")]
    MissingPayload(String),
    #[error(transparent)]
    Payload(#[from] PayloadError),
    #[error("DB ERROR: {0}")]
    Db(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    PayloadTooLarge(String),
    #[error("{0}")]
    InternalServerError(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    BadGateway(String),
    #[error("{0}")]
    TooManyRequests(String),
    #[error("{0}")]
    ServiceUnavailable(String),
    #[error("{0}")]
    MethodNotAllowed(String),
}

impl ErrorCode for HttpError {
    fn error_code(&self) -> Code {
        match self {
            HttpError::MissingContentType(_) => Code::MissingContentType,
            HttpError::InvalidContentType(_, _) => Code::InvalidContentType,
            HttpError::DocumentNotFound(_) => Code::DocumentNotFound,
            HttpError::MissingPayload(_) => Code::MissingPayload,
            HttpError::Payload(e) => e.error_code(),
            HttpError::Db(_) => Code::DBError,
            HttpError::Unauthorized(_) => Code::Unauthorized,
            HttpError::Forbidden(_) => Code::Forbidden,
            HttpError::InternalServerError(_) => Code::InternalServerError,
            HttpError::NotFound(_) => Code::NotFound,
            HttpError::Conflict(_) => Code::Conflict,
            HttpError::BadRequest(_) => Code::BadRequest,
            HttpError::PayloadTooLarge(_) => Code::PayloadTooLarge,
            HttpError::BadGateway(_) => Code::BadGateway,
            HttpError::TooManyRequests(_) => Code::TooManyRequests,
            HttpError::ServiceUnavailable(_) => Code::ServiceUnavailable,
            HttpError::MethodNotAllowed(_) => Code::MethodNotAllowed,
        }
    }
}

#[allow(dead_code, clippy::enum_variant_names)]
#[derive(Debug, thiserror::Error)]
pub enum PayloadError {
    #[error("The json payload provided is malformed. `{0}`.")]
    MalformedPayload(serde_json::error::Error),
    #[error("A json payload is missing.")]
    MissingPayload,
    #[error("Error while receiving the playload. `{0}`.")]
    ReceivePayload(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl ErrorCode for PayloadError {
    fn error_code(&self) -> Code {
        match self {
            PayloadError::MissingPayload => Code::MissingPayload,
            PayloadError::MalformedPayload(_) => Code::MalformedPayload,
            PayloadError::ReceivePayload(_) => Code::Internal,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    #[serde(skip)]
    pub code: StatusCode,
    #[serde(skip)]
    pub headers: Vec<(String, String)>,
    pub message: String,
    #[serde(rename = "code")]
    error_code: Box<str>,
    #[serde(rename = "type")]
    error_type: Box<str>,
    #[serde(rename = "link")]
    error_link: Box<str>,
}

impl ErrorResponse {
    pub fn from_msg(message: String, code: Code) -> Self {
        tracing::error!("{}: {}", code.name(), message);
        Self {
            code: code.http(),
            message,
            headers: Vec::new(),
            error_code: code.name().into(),
            error_type: code.type_().into(),
            error_link: code.url().into(),
        }
    }

    pub fn internal(error: impl fmt::Display) -> Self {
        tracing::error!("internal_server_error: {}", error);
        Self::from_msg("internal error".to_string(), Code::InternalServerError)
    }

    pub fn insert_header(&mut self, key: &str, value: &str) -> &mut Self {
        self.headers.push((key.to_string(), value.to_string()));
        self
    }

    fn body_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| {
            br#"{"message":"internal error","code":"internal_server_error","type":"internal"}"#
                .to_vec()
        })
    }
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for ErrorResponse {}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct OAuthErrorResponse {
    #[serde(skip)]
    pub code: StatusCode,
    #[serde(skip)]
    pub headers: Vec<(String, String)>,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_uri: Option<String>,
}

impl OAuthErrorResponse {
    pub fn from_oauth_error(status: StatusCode, error: oidc::OAuthError) -> Self {
        Self {
            code: status,
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
        let mut err = Self::from_oauth_error(
            StatusCode::UNAUTHORIZED,
            oidc::OAuthError::invalid_client(description),
        );
        err.insert_header("WWW-Authenticate", "Basic realm=\"stargate-oauth-token\"");
        err
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
        tracing::error!("oauth server_error: {}", error);
        Self::from_oauth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            oidc::OAuthError::server_error(error),
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.description())
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
        let message = error.description().to_string();
        let mut response = match error.code {
            StatusCode::UNAUTHORIZED => ErrorResponse::from(HttpError::Unauthorized(message)),
            StatusCode::FORBIDDEN => ErrorResponse::from(HttpError::Forbidden(message)),
            StatusCode::INTERNAL_SERVER_ERROR => {
                ErrorResponse::from(HttpError::InternalServerError(message))
            }
            _ => ErrorResponse::from(HttpError::BadRequest(message)),
        };
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

impl<T> From<T> for ErrorResponse
where
    T: std::error::Error + ErrorCode,
{
    fn from(other: T) -> Self {
        Self::from_msg(other.to_string(), other.error_code())
    }
}

impl axum::response::IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        use http::header::{CONTENT_TYPE, HeaderName, HeaderValue, RETRY_AFTER};

        let json = self.body_json();
        let mut builder = http::Response::builder()
            .status(self.code)
            .header(CONTENT_TYPE, "application/json");

        if self.code == StatusCode::SERVICE_UNAVAILABLE {
            builder = builder.header(RETRY_AFTER, "10");
        }

        for (key, value) in &self.headers {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(key.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                builder = builder.header(name, val);
            }
        }

        builder
            .body(axum::body::Body::from(json))
            .unwrap_or_else(|_| {
                axum::response::Response::new(axum::body::Body::from(
                    br#"{"message":"internal error","code":"internal_server_error","type":"internal"}"#
                        .as_slice(),
                ))
            })
    }
}

impl axum::response::IntoResponse for OAuthErrorResponse {
    fn into_response(self) -> axum::response::Response {
        use http::header::{CONTENT_TYPE, HeaderName, HeaderValue};

        let json = self.body_json();
        let mut builder = http::Response::builder()
            .status(self.code)
            .header(CONTENT_TYPE, "application/json");

        for (key, value) in &self.headers {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(key.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                builder = builder.header(name, val);
            }
        }

        builder
            .body(axum::body::Body::from(json))
            .unwrap_or_else(|_| {
                axum::response::Response::new(axum::body::Body::from(
                    br#"{"error":"server_error","error_description":"server error"}"#.as_slice(),
                ))
            })
    }
}
