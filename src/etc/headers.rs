pub async fn security_headers_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use http::header;
    let tls_enabled = crate::etc::tls::enabled().unwrap_or(false);
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        http::HeaderValue::from_static("DENY"),
    );
    if tls_enabled {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            http::HeaderValue::from_static("max-age=63072000; includeSubDomains"),
        );
    }
    response
}
