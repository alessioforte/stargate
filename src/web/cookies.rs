pub(crate) const SESSION_COOKIE: &str = "jwt";

/// `SameSite=Lax` (not `Strict`) so the session reaches `/oauth/authorize`
/// when a relying party on another site redirects the browser there; the
/// cookie still stays off cross-site subrequests and non-GET navigations.
pub fn build_jwt_cookie(access_token: &str, max_age_secs: i64) -> String {
    let secure = crate::srv::tls::served_over_https();
    let mut parts = vec![
        format!("{SESSION_COOKIE}={access_token}"),
        "Path=/".to_string(),
        "HttpOnly".to_string(),
        "SameSite=Lax".to_string(),
        format!("Max-Age={}", max_age_secs),
    ];
    if secure {
        parts.push("Secure".to_string());
    }
    parts.join("; ")
}
