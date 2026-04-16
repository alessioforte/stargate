pub fn configure() -> actix_cors::Cors {
    let origins = std::env::var("CORS_ORIGINS").unwrap_or_default();

    let mut cors = actix_cors::Cors::default()
        .allowed_methods(vec!["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"])
        .allowed_headers(vec![
            actix_web::http::header::AUTHORIZATION,
            actix_web::http::header::CONTENT_TYPE,
        ])
        .max_age(3600);

    if origins == "*" {
        cors = cors.allow_any_origin();
    } else if !origins.is_empty() {
        for origin in origins.split(',') {
            let origin = origin.trim();
            if !origin.is_empty() {
                cors = cors.allowed_origin(origin);
            }
        }
    }
    // empty CORS_ORIGINS = deny all cross-origin requests

    cors
}
