pub fn build_jwt_cookie(access_token: &str, max_age_secs: i64) -> String {
    let secure = crate::etc::tls::enabled().unwrap_or(false);
    let mut parts = vec![
        format!("jwt={}", access_token),
        "Path=/".to_string(),
        "HttpOnly".to_string(),
        "SameSite=Strict".to_string(),
        format!("Max-Age={}", max_age_secs),
    ];
    if secure {
        parts.push("Secure".to_string());
    }
    parts.join("; ")
}
