use http::{HeaderMap, HeaderName, HeaderValue, header::CONNECTION};
use std::time::Duration;

/// Fields every proxy consumes and never forwards (RFC 9110 §7.6.1).
const HOP_BY_HOP_FIELDS: [&str; 10] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "proxy-connection",
    "trailers",
];

/// Removes the fields a proxy consumes before dispatching an upstream request.
/// Only this fixed list is removed: fields a client nominated with `Connection`
/// were already consumed at ingress, so a value set by operator middleware
/// survives.
pub(crate) fn strip_hop_by_hop_headers(headers: &mut HeaderMap) {
    for name in HOP_BY_HOP_FIELDS {
        headers.remove(name);
    }
}

/// The ingress side of RFC 9110 §7.6.1: consumes the client's `Connection`
/// field, every field it nominates, and the fixed hop-by-hop list. A client can
/// no longer make a later `request_headers` value disappear by nominating it.
///
/// A WebSocket upgrade keeps `Connection: upgrade` and `Upgrade: websocket`,
/// which the downstream handshake still needs; other nominated fields and the
/// remaining hop-by-hop fields are removed.
pub(crate) fn strip_ingress_hop_by_hop_headers(headers: &mut HeaderMap) {
    let upgrade = is_websocket_upgrade(headers);
    let mut nominated = Vec::new();
    for value in headers.get_all(CONNECTION) {
        for field in value.as_bytes().split(|byte| *byte == b',') {
            if let Ok(name) = HeaderName::from_bytes(field.trim_ascii()) {
                nominated.push(name);
            }
        }
    }
    for name in nominated {
        if upgrade && name == http::header::UPGRADE {
            continue;
        }
        headers.remove(name);
    }
    if upgrade {
        headers.insert(CONNECTION, HeaderValue::from_static("upgrade"));
    }
    for name in HOP_BY_HOP_FIELDS {
        if upgrade && (name == "connection" || name == "upgrade") {
            continue;
        }
        headers.remove(name);
    }
}

fn is_websocket_upgrade(headers: &HeaderMap) -> bool {
    contains_token(headers, CONNECTION, b"upgrade")
        && contains_token(headers, http::header::UPGRADE, b"websocket")
}

fn contains_token(headers: &HeaderMap, name: http::header::HeaderName, token: &[u8]) -> bool {
    headers.get_all(name).iter().any(|value| {
        value
            .as_bytes()
            .split(|byte| *byte == b',')
            .any(|value| value.trim_ascii().eq_ignore_ascii_case(token))
    })
}

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
    use crate::web::request::RequestExt;
    use http::header;
    let https = req.is_https();
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
    if https {
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
    use http::header::{PROXY_AUTHORIZATION, TE, UPGRADE};

    #[test]
    fn ingress_consumes_connection_nominated_and_hop_by_hop_fields() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONNECTION,
            HeaderValue::from_static("keep-alive, x-tenant-id"),
        );
        headers.insert("keep-alive", HeaderValue::from_static("timeout=5"));
        headers.insert("x-tenant-id", HeaderValue::from_static("client"));
        headers.insert(TE, HeaderValue::from_static("trailers"));
        headers.insert(PROXY_AUTHORIZATION, HeaderValue::from_static("Basic x"));
        headers.insert("x-keep", HeaderValue::from_static("ok"));

        strip_ingress_hop_by_hop_headers(&mut headers);

        for name in [
            CONNECTION,
            "keep-alive".parse().unwrap(),
            TE,
            PROXY_AUTHORIZATION,
        ] {
            assert!(headers.get(name).is_none());
        }
        assert!(headers.get("x-tenant-id").is_none());
        assert_eq!(headers["x-keep"], "ok");
    }

    #[test]
    fn ingress_keeps_a_websocket_upgrade_and_nothing_else() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, HeaderValue::from_static("upgrade, x-secret"));
        headers.insert(UPGRADE, HeaderValue::from_static("websocket"));
        headers.insert("x-secret", HeaderValue::from_static("client"));
        headers.insert("keep-alive", HeaderValue::from_static("timeout=5"));

        strip_ingress_hop_by_hop_headers(&mut headers);

        assert_eq!(headers[CONNECTION], "upgrade");
        assert_eq!(headers[UPGRADE], "websocket");
        assert!(headers.get("x-secret").is_none());
        assert!(headers.get("keep-alive").is_none());
    }

    #[test]
    fn dispatch_removes_only_the_fixed_list() {
        let mut headers = HeaderMap::new();
        headers.insert(CONNECTION, HeaderValue::from_static("x-operator"));
        headers.insert("x-operator", HeaderValue::from_static("configured"));
        headers.insert(TE, HeaderValue::from_static("trailers"));

        strip_hop_by_hop_headers(&mut headers);

        assert!(headers.get(CONNECTION).is_none());
        assert!(headers.get(TE).is_none());
        assert_eq!(headers["x-operator"], "configured");
    }

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
