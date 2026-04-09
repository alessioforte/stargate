use crate::etc::tls;
use actix_web::{http::header, middleware::DefaultHeaders};

pub fn configure() -> DefaultHeaders {
    let tls_security_headers = tls::enabled().unwrap_or(false);
    let headers = if tls_security_headers {
        DefaultHeaders::new()
            .add((header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
            .add((header::X_FRAME_OPTIONS, "DENY"))
            .add((
                header::STRICT_TRANSPORT_SECURITY,
                "max-age=63072000; includeSubDomains",
            ))
    } else {
        DefaultHeaders::new()
            .add((header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
            .add((header::X_FRAME_OPTIONS, "DENY"))
    };

    headers
}
