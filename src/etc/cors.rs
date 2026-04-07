pub mod middleware {
    pub fn configure() -> actix_cors::Cors {
        let origins = std::env::var("CORS_ORIGINS").unwrap_or_default();

        let mut cors = actix_cors::Cors::default()
            .allowed_methods(vec!["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"])
            .allowed_headers(vec![
                actix_web::http::header::AUTHORIZATION,
                actix_web::http::header::CONTENT_TYPE,
            ])
            .max_age(3600);

        if origins.is_empty() || origins == "*" {
            cors = cors.allow_any_origin();
        } else {
            for origin in origins.split(',') {
                let origin = origin.trim();
                if !origin.is_empty() {
                    cors = cors.allowed_origin(origin);
                }
            }
        }

        cors
    }
}
