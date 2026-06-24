pub fn hyper_configure() -> tower_http::cors::CorsLayer {
    use http::{Method, header};
    use std::time::Duration;
    use tower_http::cors::CorsLayer;

    let origins_str = std::env::var("CORS_ORIGINS").unwrap_or_default();
    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .max_age(Duration::from_secs(3600));

    if origins_str == "*" {
        layer.allow_origin(tower_http::cors::Any)
    } else if !origins_str.is_empty() {
        let origins: Vec<http::HeaderValue> = origins_str
            .split(',')
            .filter_map(|o| o.trim().parse().ok())
            .collect();
        if origins.is_empty() {
            layer
        } else {
            layer.allow_origin(origins).allow_credentials(true)
        }
    } else {
        layer
    }
}
