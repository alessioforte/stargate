pub fn configure() -> actix_cors::Cors {
    actix_cors::Cors::default()
        .allow_any_origin() // Allow any origin for development purposes
        .allowed_methods(vec!["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"])
        .allowed_headers(vec![
            actix_web::http::header::AUTHORIZATION,
            actix_web::http::header::CONTENT_TYPE,
        ])
        .max_age(3600)
}
