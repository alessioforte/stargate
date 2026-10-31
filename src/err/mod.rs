pub mod oauth;
pub mod types;

pub use oauth::{OAuthErrorResponse, OAuthResult};
pub use types::{ErrorCode, ErrorType};

use http::StatusCode;
use serde::Serialize;
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

impl axum::response::IntoResponse for ErrorResponse {
    fn into_response(self) -> axum::response::Response {
        use http::header::{CONTENT_TYPE, HeaderName, HeaderValue, RETRY_AFTER};

        // Traffic limiter failures are counted on every check and logged only
        // on state transitions by the limiter runtime, including fail-closed checks.
        let limit_store_failure = matches!(
            self.code,
            ErrorCode::GatewayIngressUnavailable
                | ErrorCode::GatewayLimiterUnavailable
                | ErrorCode::IamLimiterUnavailable
        );
        let limit_store_failure = limit_store_failure && self.params.contains_key("point");
        if self.status.is_server_error() && !limit_store_failure {
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
                crate::web::headers::retry_after_header_value(Some(
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
    fn grpc_http2_required_has_catalog_metadata() {
        let code = ErrorCode::GatewayGrpcHttp2Required;
        assert!(ErrorCode::ALL.contains(&code));
        assert_eq!(code.as_str(), "gateway.grpc_http2_required");
        assert_eq!(code.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            code.message(),
            "Upstream does not allow HTTP/2, which gRPC requires"
        );
        assert_eq!(code.error_type(), ErrorType::Upstream);
    }

    #[test]
    fn limit_and_organization_availability_codes_have_distinct_catalog_entries() {
        for (code, name) in [
            (
                ErrorCode::GatewayLimiterUnavailable,
                "gateway.limiter_unavailable",
            ),
            (
                ErrorCode::GatewayOrganizationLimitsUnavailable,
                "gateway.organization_limits_unavailable",
            ),
        ] {
            assert_eq!(code.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(code.error_type(), ErrorType::Availability);
            assert_eq!(code.as_str(), name);
            assert_eq!(
                ErrorCode::ALL
                    .iter()
                    .filter(|entry| **entry == code)
                    .count(),
                1
            );
        }
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
}
