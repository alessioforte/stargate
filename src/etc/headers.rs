use std::time::Duration;

/// Format HTTP Retry-After delay-seconds, defaulting to 60 seconds when the
/// limiter supplies no retry duration. Round up so clients never retry early.
pub(crate) fn retry_after_header_value(retry_after: Option<Duration>) -> String {
    let duration = retry_after.unwrap_or(Duration::from_secs(60));
    // RFC 9110 §10.2.3: delay-seconds is a nonnegative decimal integer.
    // Widen before rounding to preserve even Duration::MAX without overflow.
    let seconds = u128::from(duration.as_secs()) + u128::from(duration.subsec_nanos() > 0);
    seconds.to_string()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_uses_integer_seconds_rounded_up() {
        for (duration, expected) in [
            (Duration::ZERO, "0"),
            (Duration::from_nanos(1), "1"),
            (Duration::from_millis(500), "1"),
            (Duration::from_secs(1), "1"),
            (Duration::from_millis(1001), "2"),
            (Duration::from_secs(60), "60"),
            (Duration::new(60, 1), "61"),
        ] {
            assert_eq!(retry_after_header_value(Some(duration)), expected);
        }
    }

    #[test]
    fn retry_after_defaults_to_sixty_seconds_and_preserves_the_full_duration_range() {
        assert_eq!(retry_after_header_value(None), "60");
        assert_eq!(
            retry_after_header_value(Some(Duration::from_secs(u64::MAX))),
            "18446744073709551615"
        );
        assert_eq!(
            retry_after_header_value(Some(Duration::MAX)),
            "18446744073709551616"
        );
    }
}
