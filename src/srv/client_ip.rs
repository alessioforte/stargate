//! The client address behind a connection peer, resolved once per request.
//!
//! The listener's [`ProxyTrust`] decides whether a forwarding header counts:
//! a peer outside it is always the client, and its forwarding metadata is
//! removed before any plane or route sees it. Every later reader (logging,
//! ingress and admin limits, login guards, OTP, source-IP matchers, audit, and
//! the signed internal context) reads the result from the request extensions.

use crate::srv::{ConnectionInfo, proxy::ProxyTrust};
use http::{HeaderMap, Request, header::HeaderName};
use std::net::{IpAddr, SocketAddr};

/// The client address resolved at ingress with this listener's proxy trust.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientIp(pub IpAddr);

/// Resolves the client address and removes untrusted forwarding metadata.
/// `ConnectionInfo` must already be in the request's extensions; requests a
/// test builds without one keep the peer-only behavior of [`resolve`].
pub(crate) fn prepare<B>(request: &mut Request<B>) {
    let Some(connection) = request.extensions().get::<ConnectionInfo>() else {
        return;
    };
    let peer = connection.peer.ip();
    let trust = &connection.listener.trusted_proxies;
    let client = resolve(peer, trust, request.headers());
    if !trust.trusts(peer) {
        strip_forwarded(request.headers_mut());
    }
    request.extensions_mut().insert(ClientIp(client));
}

/// The client address: the peer itself, or the address a trusted proxy
/// appended to `X-Forwarded-For` (`X-Real-IP` when the chain has none).
pub(crate) fn resolve(peer: IpAddr, trust: &ProxyTrust, headers: &HeaderMap) -> IpAddr {
    if !trust.trusts(peer) {
        return peer;
    }
    forwarded_ip(headers, trust).unwrap_or(peer)
}

/// Remove the forwarding metadata a peer we do not trust may have written.
pub(crate) fn strip_forwarded(headers: &mut HeaderMap) {
    let forwarded: Vec<HeaderName> = headers
        .keys()
        .filter(|name| is_forwarded(name))
        .cloned()
        .collect();
    for name in forwarded {
        headers.remove(name);
    }
}

fn is_forwarded(name: &HeaderName) -> bool {
    matches!(name.as_str(), "forwarded" | "x-real-ip") || name.as_str().starts_with("x-forwarded-")
}

/// Parse a single IP token that may be a bare IP, a bracketed IPv6 address,
/// or a socket address (`ip:port` / `[ip]:port`).
pub(crate) fn parse_single_ip(raw: &str) -> Option<IpAddr> {
    let token = raw.trim();
    if token.is_empty() {
        return None;
    }

    let unbracketed = token
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(token);

    if let Ok(ip) = unbracketed.parse::<IpAddr>() {
        return Some(ip);
    }

    if let Ok(socket) = token.parse::<SocketAddr>() {
        return Some(socket.ip());
    }

    None
}

/// Resolve the real client IP from an `X-Forwarded-For` header value,
/// trusting only configured proxies.
///
/// A conforming proxy *appends* the peer it received the connection from to
/// the right of the list, so the right-most entries are the closest, most
/// trustworthy hops while the left-most entry is fully client-controlled. We
/// therefore return the right-most address that is not itself a trusted
/// proxy, which prevents a client from spoofing its address by prepending a
/// fake left-most entry. If every entry is a trusted proxy, we fall back to
/// the left-most parseable address (the outermost known hop).
fn client_from_forwarded_for(value: &str, trust: &ProxyTrust) -> Option<IpAddr> {
    select_forwarded_client(value, &|ip| trust.trusts(ip))
}

fn select_forwarded_client<F>(value: &str, is_trusted: &F) -> Option<IpAddr>
where
    F: Fn(IpAddr) -> bool,
{
    let mut leftmost = None;
    let mut rightmost_untrusted = None;

    for token in value.split(',') {
        let Some(ip) = parse_single_ip(token) else {
            continue;
        };
        if leftmost.is_none() {
            leftmost = Some(ip);
        }
        if !is_trusted(ip) {
            rightmost_untrusted = Some(ip);
        }
    }

    rightmost_untrusted.or(leftmost)
}

fn forwarded_ip(headers: &HeaderMap, trust: &ProxyTrust) -> Option<IpAddr> {
    if let Some(xff) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        && let Some(ip) = client_from_forwarded_for(xff, trust)
    {
        return Some(ip);
    }
    headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(parse_single_ip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    fn headers(values: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in values {
            headers.append(HeaderName::try_from(*name).unwrap(), value.parse().unwrap());
        }
        headers
    }

    fn ip(value: &str) -> IpAddr {
        value.parse().unwrap()
    }

    #[test]
    fn an_untrusted_peer_is_the_client_and_keeps_no_forwarding_metadata() {
        let mut request = Request::builder()
            .header("x-forwarded-for", "198.51.100.1")
            .header("x-forwarded-proto", "https")
            .header("x-real-ip", "198.51.100.2")
            .header("forwarded", "for=198.51.100.3")
            .header("x-request-id", "keep")
            .body(())
            .unwrap();
        request.extensions_mut().insert(ConnectionInfo {
            peer: "203.0.113.5:1234".parse().unwrap(),
            listener: std::sync::Arc::new(crate::srv::ListenerInfo {
                name: "internal".to_string(),
                planes: crate::api::plane::PlaneSet::ALL,
                tls: false,
                trusted_proxies: ProxyTrust::default(),
            }),
        });

        prepare(&mut request);

        assert_eq!(
            request.extensions().get::<ClientIp>(),
            Some(&ClientIp(ip("203.0.113.5")))
        );
        assert!(!request.headers().contains_key("x-forwarded-for"));
        assert!(!request.headers().contains_key("x-forwarded-proto"));
        assert!(!request.headers().contains_key("x-real-ip"));
        assert!(!request.headers().contains_key("forwarded"));
        assert_eq!(request.headers()["x-request-id"], "keep");
    }

    #[test]
    fn a_trusted_proxy_preserves_the_forwarded_client_and_metadata() {
        let trust = ProxyTrust::parse("10.42.0.0/16").unwrap();
        let mut request = Request::builder()
            .header("x-forwarded-for", "203.0.113.7, 10.42.0.9")
            .header("x-forwarded-proto", "https")
            .body(())
            .unwrap();
        request.extensions_mut().insert(ConnectionInfo {
            peer: "10.42.0.9:1234".parse().unwrap(),
            listener: std::sync::Arc::new(crate::srv::ListenerInfo {
                name: "public".to_string(),
                planes: crate::api::plane::PlaneSet::ALL,
                tls: false,
                trusted_proxies: trust,
            }),
        });

        prepare(&mut request);

        assert_eq!(
            request.extensions().get::<ClientIp>(),
            Some(&ClientIp(ip("203.0.113.7")))
        );
        assert_eq!(
            request.headers()["x-forwarded-for"],
            "203.0.113.7, 10.42.0.9"
        );
        assert_eq!(request.headers()["x-forwarded-proto"], "https");
    }

    #[test]
    fn a_trusted_peer_without_headers_is_the_client() {
        let trust = ProxyTrust::parse("127.0.0.1/32").unwrap();
        assert_eq!(
            resolve(ip("127.0.0.1"), &trust, &HeaderMap::new()),
            ip("127.0.0.1")
        );
    }

    #[test]
    fn forwarded_client_ignores_spoofed_leftmost_entry() {
        let trusted = |ip: IpAddr| matches!(ip, IpAddr::V4(v4) if v4.octets()[0] == 10);

        // Spoofed left-most value, real client appended by the proxy.
        assert_eq!(
            select_forwarded_client("9.9.9.9, 203.0.113.7, 10.0.0.1", &trusted),
            Some(ip("203.0.113.7"))
        );
        // Every hop is a trusted proxy: keep the outermost known hop.
        assert_eq!(
            select_forwarded_client("10.0.0.1, 10.0.0.2", &trusted),
            Some(ip("10.0.0.1"))
        );
        // Malformed entries are skipped around the real client.
        assert_eq!(
            select_forwarded_client("unknown, 203.0.113.7:443, 10.0.0.1", &trusted),
            Some(ip("203.0.113.7"))
        );
        assert_eq!(
            select_forwarded_client("unknown, 10.0.0.1", &trusted),
            Some(ip("10.0.0.1"))
        );
    }

    #[test]
    fn x_forwarded_for_wins_over_x_real_ip_only_when_it_has_an_address() {
        let trust = ProxyTrust::parse("10.0.0.0/8").unwrap();
        let peer = ip("10.0.0.1");
        assert_eq!(
            resolve(
                peer,
                &trust,
                &headers(&[
                    ("x-forwarded-for", "203.0.113.7"),
                    ("x-real-ip", "203.0.113.8")
                ])
            ),
            ip("203.0.113.7")
        );
        assert_eq!(
            resolve(
                peer,
                &trust,
                &headers(&[("x-forwarded-for", "unknown"), ("x-real-ip", "203.0.113.8")])
            ),
            ip("203.0.113.8")
        );
    }

    #[test]
    fn parse_single_ip_handles_bare_bracketed_and_socket() {
        assert_eq!(parse_single_ip(" 203.0.113.7 "), Some(ip("203.0.113.7")));
        assert_eq!(parse_single_ip("203.0.113.7:443"), Some(ip("203.0.113.7")));
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
    fn stripping_covers_custom_forwarded_fields_and_leaves_other_headers() {
        let mut headers = headers(&[
            ("forwarded", "for=1.2.3.4"),
            ("x-forwarded-client-cert", "Subject=CN=proxy"),
            ("x-real-ip", "1.2.3.4"),
            ("x-request-id", "keep"),
            ("host", "identity.test"),
        ]);
        strip_forwarded(&mut headers);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers["x-request-id"], "keep");
        assert_eq!(headers["host"], "identity.test");
    }
}
