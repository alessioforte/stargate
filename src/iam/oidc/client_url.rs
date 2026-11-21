//! URLs an OAuth client registers for Stargate to call.
//!
//! `initiate_login_uri`, `backchannel_logout_uri` and `jwks_uri` share one
//! rule: an absolute URL without a fragment, HTTPS except on loopback hosts
//! (OpenID Connect Core 1.0 §4, Back-Channel Logout 1.0 §2.2, RFC 7517 §5).
//! The admin API enforces it when a client is written, and the provider checks
//! it again before using a stored value, because a row can also come from an
//! import, a bootstrap, or direct SQL.

/// Whether `uri` may be used as a client endpoint.
pub fn is_client_endpoint_url(uri: &str) -> bool {
    if uri.contains('#') || uri.chars().any(char::is_whitespace) {
        return false;
    }
    match url::Url::parse(uri) {
        Ok(parsed) => match parsed.scheme() {
            "https" => true,
            "http" => is_loopback_host(parsed.host_str()),
            _ => false,
        },
        Err(_) => false,
    }
}

/// Whether plain HTTP is acceptable for this host: a loopback host, as for
/// redirect URIs.
pub fn is_loopback_host(host: Option<&str>) -> bool {
    matches!(host, Some("localhost" | "127.0.0.1" | "::1"))
}

#[cfg(test)]
mod tests {
    use super::is_client_endpoint_url;

    #[test]
    fn https_is_required_except_on_loopback_hosts() {
        assert!(is_client_endpoint_url("https://app.example/login"));
        assert!(is_client_endpoint_url("http://localhost:3000/login"));
        assert!(is_client_endpoint_url("http://127.0.0.1/login"));
        assert!(!is_client_endpoint_url("http://app.example/login"));
        assert!(!is_client_endpoint_url(
            "https://app.example/login#fragment"
        ));
        assert!(!is_client_endpoint_url("https://app.example/a b"));
        assert!(!is_client_endpoint_url("not a url"));
    }

    #[test]
    fn a_scheme_a_browser_would_execute_is_rejected() {
        for uri in [
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox(1)",
            "/relative/path",
        ] {
            assert!(!is_client_endpoint_url(uri), "{uri}");
        }
    }
}
