use super::RequestExt;
use http::Request;

fn req_with(headers: &[(&str, &str)], query: Option<&str>) -> Request<()> {
    let uri = match query {
        Some(q) => format!("http://x/?{}", q),
        None => "http://x/".to_string(),
    };
    let mut b = Request::builder().uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    b.body(()).unwrap()
}

#[test]
fn token_from_bearer_header() {
    let r = req_with(&[("authorization", "Bearer abc.def.ghi")], None);
    assert_eq!(r.get_token().as_deref(), Some("abc.def.ghi"));
}

#[test]
fn token_from_cookie_jwt() {
    let r = req_with(&[("cookie", "other=1; jwt=xyz.tok; more=2")], None);
    assert_eq!(r.get_token().as_deref(), Some("xyz.tok"));
}

#[test]
fn only_the_gateway_reads_query_credentials() {
    for key in super::TOKEN_QUERY_KEYS {
        let r = req_with(&[], Some(&format!("foo=1&{key}=qtok&bar=2")));
        assert_eq!(r.get_token(), None);
        assert_eq!(r.get_bearer_token(), None);
        assert_eq!(r.get_gateway_token().as_deref(), Some("qtok"));
    }
}

#[test]
fn header_credentials_take_precedence_over_cookies_and_query() {
    let r = req_with(
        &[("authorization", "Bearer header"), ("cookie", "jwt=cookie")],
        Some("token=query"),
    );
    assert_eq!(r.get_bearer_token().as_deref(), Some("header"));
    assert_eq!(r.get_token().as_deref(), Some("header"));
    assert_eq!(r.get_gateway_token().as_deref(), Some("header"));
    let r = req_with(&[("cookie", "jwt=cookie")], Some("token=query"));
    assert_eq!(r.get_bearer_token(), None);
    assert_eq!(r.get_token().as_deref(), Some("cookie"));
}

#[test]
fn an_authorization_header_is_never_replaced_by_the_session_cookie() {
    for value in ["Bearer ", "Basic dXNlcjpwYXNz", "not-a-scheme"] {
        let r = req_with(&[("authorization", value), ("cookie", "jwt=cookie")], None);
        assert_eq!(r.get_bearer_token(), None, "{value}");
        assert_eq!(r.get_token(), None, "{value}");
    }

    let r = req_with(&[("cookie", "jwt=cookie")], None);
    assert_eq!(r.get_token().as_deref(), Some("cookie"));
}

#[test]
fn api_key_header() {
    let r = req_with(&[("x-api-key", "ak_123")], None);
    assert_eq!(r.get_api_key().as_deref(), Some("ak_123"));
}

#[test]
fn protocol_ws_upgrade() {
    for headers in [
        vec![("upgrade", "websocket")],
        vec![("upgrade", "h2c, WebSocket")],
        vec![("upgrade", "h2c"), ("upgrade", "WebSocket")],
    ] {
        assert_eq!(req_with(&headers, None).get_protocol(), "ws");
    }
}

#[test]
fn extended_websocket_connect_is_identified_for_explicit_rejection() {
    let mut req = Request::builder()
        .method("CONNECT")
        .version(http::Version::HTTP_2)
        .uri("https://gateway.test/socket")
        .body(())
        .unwrap();
    assert_eq!(req.get_protocol(), "http");
    req.extensions_mut()
        .insert(hyper::ext::Protocol::from_static("websocket"));
    assert_eq!(req.get_protocol(), "ws");
}

#[test]
fn protocol_default_http() {
    let r = req_with(&[], None);
    assert_eq!(r.get_protocol(), "http");
}

#[test]
fn host_from_header_is_normalized() {
    let r = req_with(&[("host", "API.EXAMPLE.COM:8443")], None);
    assert_eq!(r.get_host().as_deref(), Some("api.example.com"));
}

#[test]
fn query_values_decode_repeated_keys() {
    let r = req_with(&[], Some("preview=true&preview=blue%20sky&other=1"));
    assert_eq!(
        r.get_query_values("preview"),
        vec!["true".to_string(), "blue sky".to_string()]
    );
}

#[test]
fn cookie_lookup_reads_named_cookie() {
    let r = req_with(&[("cookie", "other=1; canary=v2; more=2")], None);
    assert_eq!(r.get_cookie_value("canary").as_deref(), Some("v2"));
}

#[test]
fn client_ip_uses_the_ingress_resolution_and_falls_back_to_the_peer() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)), 55555);

    // A request a test builds itself has no ingress resolution: the peer is
    // the client, and a forwarding header never changes it.
    let mut r = req_with(&[("x-forwarded-for", "9.9.9.9")], None);
    r.extensions_mut().insert(crate::srv::test_connection(peer));
    assert_eq!(r.get_client_ip(), "1.2.3.4");

    // The server resolves the client once at ingress.
    r.extensions_mut()
        .insert(crate::srv::client_ip::ClientIp(IpAddr::V4(Ipv4Addr::new(
            203, 0, 113, 7,
        ))));
    assert_eq!(r.get_client_ip(), "203.0.113.7");
}

#[test]
fn user_agent_returns_header() {
    let r = req_with(&[("user-agent", "curl/8")], None);
    assert_eq!(r.get_user_agent().as_deref(), Some("curl/8"));
}
