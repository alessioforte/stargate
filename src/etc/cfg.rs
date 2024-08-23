use crate::errors::{HttpError, PayloadError};
use actix_web::error::JsonPayloadError;
use actix_web::http::header::CONTENT_TYPE;
use actix_web::web::ServiceConfig;
use actix_web::{web, HttpRequest};

pub fn app_data(cfg: &mut ServiceConfig) {
    cfg.app_data(
        web::JsonConfig::default()
            .content_type(|mime| mime == mime::APPLICATION_JSON)
            .error_handler(|err, req: &HttpRequest| match err {
                JsonPayloadError::ContentType => match req.headers().get(CONTENT_TYPE) {
                    Some(content_type) => HttpError::InvalidContentType(
                        content_type.to_str().unwrap_or("unknown").to_string(),
                        vec![mime::APPLICATION_JSON.to_string()],
                    )
                    .into(),
                    None => HttpError::MissingContentType(vec![mime::APPLICATION_JSON.to_string()])
                        .into(),
                },
                err => PayloadError::from(err).into(),
            }),
    );
}
