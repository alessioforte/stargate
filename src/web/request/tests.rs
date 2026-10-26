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
fn token_from_query_access_token() {
    let r = req_with(&[], Some("foo=1&access_token=qtok&bar=2"));
    assert_eq!(r.get_token().as_deref(), Some("qtok"));
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
fn client_ip_uses_peer_when_no_proxy_trust() {
    use axum::extract::ConnectInfo;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    let mut r = req_with(&[("x-forwarded-for", "9.9.9.9")], None);
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)), 55555);
    r.extensions_mut().insert(ConnectInfo(peer));
    assert_eq!(r.get_client_ip(), "1.2.3.4");
}

#[test]
fn parse_single_ip_handles_bare_bracketed_and_socket() {
    use super::parse_single_ip;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    assert_eq!(
        parse_single_ip(" 203.0.113.7 "),
        Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)))
    );
    assert_eq!(
        parse_single_ip("203.0.113.7:443"),
        Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)))
    );
    assert_eq!(
        parse_single_ip("[2001:db8::1]"),
        Some(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)))
    );
    assert_eq!(
        parse_single_ip("[2001:db8::1]:8443"),
        Some(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)))
    );
    assert_eq!(parse_single_ip(""), None);
    assert_eq!(parse_single_ip("not-an-ip"), None);
}

#[test]
fn forwarded_client_ignores_spoofed_leftmost_entry() {
    use super::select_forwarded_client;
    use std::net::{IpAddr, Ipv4Addr};

    // 10.0.0.0/8 is the trusted proxy network; the last hop appended by a
    // conforming proxy is the real client. A client-prepended fake entry
    // to the left must be ignored.
    let trusted = |ip: &IpAddr| matches!(ip, IpAddr::V4(v4) if v4.octets()[0] == 10);

    // Spoofed left-most value, real client appended by the proxy.
    assert_eq!(
        select_forwarded_client("9.9.9.9, 203.0.113.7, 10.0.0.1", trusted),
        Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)))
    );

    // Single-proxy hop: client's real IP is the only forwarded entry.
    assert_eq!(
        select_forwarded_client("203.0.113.7", trusted),
        Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)))
    );

    // Whole chain trusted: fall back to the left-most known hop.
    assert_eq!(
        select_forwarded_client("10.0.0.9, 10.0.0.1", trusted),
        Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9)))
    );

    // Multiple untrusted entries: the right-most untrusted one wins.
    assert_eq!(
        select_forwarded_client("1.1.1.1, 203.0.113.7, 10.0.0.1", trusted),
        Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)))
    );
}

#[test]
fn user_agent_returns_header() {
    let r = req_with(&[("user-agent", "curl/8")], None);
    assert_eq!(r.get_user_agent().as_deref(), Some("curl/8"));
}
