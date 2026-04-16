use actix_web;
use actix_web::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy)]
enum ErrorType {
    Internal,
    InvalidRequest,
    Authentication,
}

impl ErrorType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::InvalidRequest => "invalid_request",
            Self::Authentication => "authentication",
        }
    }
}

impl fmt::Display for ErrorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str((*self).as_str())
    }
}

#[derive(Clone, Copy)]
struct ErrCode {
    status_code: StatusCode,
    error_name: &'static str,
    error_type: ErrorType,
}

impl ErrCode {
    const fn authentication(error_name: &'static str, status_code: StatusCode) -> ErrCode {
        ErrCode {
            status_code,
            error_name,
            error_type: ErrorType::Authentication,
        }
    }

    const fn internal(error_name: &'static str, status_code: StatusCode) -> ErrCode {
        ErrCode {
            status_code,
            error_name,
            error_type: ErrorType::Internal,
        }
    }

    const fn invalid(error_name: &'static str, status_code: StatusCode) -> ErrCode {
        ErrCode {
            status_code,
            error_name,
            error_type: ErrorType::InvalidRequest,
        }
    }
}

static MISSING_CONTENT_TYPE: ErrCode =
    ErrCode::invalid("missing_content_type", StatusCode::UNSUPPORTED_MEDIA_TYPE);
static INVALID_CONTENT_TYPE: ErrCode =
    ErrCode::invalid("invalid_content_type", StatusCode::UNSUPPORTED_MEDIA_TYPE);
static DOCUMENT_NOT_FOUND: ErrCode = ErrCode::invalid("document_not_found", StatusCode::NOT_FOUND);
static MISSING_PAYLOAD: ErrCode = ErrCode::invalid("missing_payload", StatusCode::BAD_REQUEST);
static BAD_REQUEST: ErrCode = ErrCode::invalid("bad_request", StatusCode::BAD_REQUEST);
static INTERNAL: ErrCode = ErrCode::internal("internal", StatusCode::INTERNAL_SERVER_ERROR);
static MALFORMED_PAYLOAD: ErrCode = ErrCode::invalid("malformed_payload", StatusCode::BAD_REQUEST);
static UNSUPPORTED_MEDIA_TYPE: ErrCode =
    ErrCode::invalid("unsupported_media_type", StatusCode::UNSUPPORTED_MEDIA_TYPE);
static PAYLOAD_TOO_LARGE: ErrCode =
    ErrCode::invalid("payload_too_large", StatusCode::PAYLOAD_TOO_LARGE);
static BAD_PARAMETER: ErrCode = ErrCode::invalid("bad_parameter", StatusCode::BAD_REQUEST);
static BAD_GATEWAY: ErrCode = ErrCode::invalid("bad_gateway", StatusCode::BAD_GATEWAY);
static UNAUTHORIZED: ErrCode = ErrCode::authentication("unauthorized", StatusCode::UNAUTHORIZED);
static FORBIDDEN: ErrCode = ErrCode::authentication("forbidden", StatusCode::FORBIDDEN);
static NOT_FOUND: ErrCode = ErrCode::invalid("not_found", StatusCode::NOT_FOUND);
static CONFLICT: ErrCode = ErrCode::invalid("conflict", StatusCode::CONFLICT);
static INTERNAL_SERVER_ERROR: ErrCode =
    ErrCode::internal("internal_server_error", StatusCode::INTERNAL_SERVER_ERROR);
static INVALID_TOKEN: ErrCode = ErrCode::authentication("invalid_api_key", StatusCode::FORBIDDEN);
static MISSING_PARAMETER: ErrCode = ErrCode::invalid("missing_parameter", StatusCode::BAD_REQUEST);
static DB_ERROR: ErrCode = ErrCode::internal("database", StatusCode::INTERNAL_SERVER_ERROR);
static TOO_MANY_REQUESTS: ErrCode =
    ErrCode::invalid("too_many_requests", StatusCode::TOO_MANY_REQUESTS);
static SERVICE_UNAVAILABLE: ErrCode =
    ErrCode::internal("service_unavailable", StatusCode::SERVICE_UNAVAILABLE);
static METHOD_NOT_ALLOWED: ErrCode =
    ErrCode::invalid("method_not_allowed", StatusCode::METHOD_NOT_ALLOWED);

#[allow(clippy::enum_variant_names)]
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum Code {
    MissingContentType,
    InvalidContentType,
    DocumentNotFound,
    MissingPayload,

    BadRequest,
    Internal,
    MalformedPayload,
    UnsupportedMediaType,
    PayloadTooLarge,

    BadParameter,
    BadGateway,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    InternalServerError,
    InvalidToken,
    MissingParameter,
    DBError,

    TooManyRequests,
    ServiceUnavailable,
    MethodNotAllowed,
}

impl Code {
    /// ascociate a `Code` variant to the actual ErrCode
    const fn err_code(&self) -> &'static ErrCode {
        use Code::*;

        match *self {
            MissingContentType => &MISSING_CONTENT_TYPE,
            InvalidContentType => &INVALID_CONTENT_TYPE,
            DocumentNotFound => &DOCUMENT_NOT_FOUND,
            MissingPayload => &MISSING_PAYLOAD,
            BadRequest => &BAD_REQUEST,
            Internal => &INTERNAL,
            MalformedPayload => &MALFORMED_PAYLOAD,
            UnsupportedMediaType => &UNSUPPORTED_MEDIA_TYPE,
            PayloadTooLarge => &PAYLOAD_TOO_LARGE,
            BadParameter => &BAD_PARAMETER,
            BadGateway => &BAD_GATEWAY,
            Unauthorized => &UNAUTHORIZED,
            Forbidden => &FORBIDDEN,
            NotFound => &NOT_FOUND,
            Conflict => &CONFLICT,
            InternalServerError => &INTERNAL_SERVER_ERROR,
            InvalidToken => &INVALID_TOKEN,
            MissingParameter => &MISSING_PARAMETER,
            DBError => &DB_ERROR,
            TooManyRequests => &TOO_MANY_REQUESTS,
            ServiceUnavailable => &SERVICE_UNAVAILABLE,
            MethodNotAllowed => &METHOD_NOT_ALLOWED,
        }
    }

    /// return the HTTP status code ascociated with the `Code`
    pub const fn http(&self) -> StatusCode {
        self.err_code().status_code
    }

    /// return error name, used as error code
    pub const fn name(&self) -> &'static str {
        self.err_code().error_name
    }

    /// return the error type
    pub const fn type_(&self) -> &'static str {
        self.err_code().error_type.as_str()
    }

    /// return the doc url associated with the error
    pub fn url(&self) -> String {
        let base =
            std::env::var("DOCS_URL").unwrap_or_else(|_| "https://docs.stargate.dev".to_string());
        format!("{}/errors#{}", base, self.name())
    }
}

pub trait ErrorCode {
    fn error_code(&self) -> Code;

    // /// returns the HTTP status code associated with the error
    // fn http_status(&self) -> StatusCode {
    //     self.error_code().http()
    // }

    // /// returns the doc url associated with the error
    // fn error_url(&self) -> String {
    //     self.error_code().url()
    // }

    // /// returns error name, used as error code
    // fn error_name(&self) -> String {
    //     self.error_code().name()
    // }

    // /// return the error type
    // fn error_type(&self) -> String {
    //     self.error_code().type_()
    // }
}
