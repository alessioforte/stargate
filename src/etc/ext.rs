use http::Request;
use http::header::{AUTHORIZATION, COOKIE, HeaderMap, USER_AGENT};
use std::net::{IpAddr, SocketAddr};

/// Parse an IP address from a raw string that may contain a comma-separated
/// list, square brackets, or a socket address (ip:port).
pub(crate) fn parse_ip_str(raw: &str) -> Option<IpAddr> {
    let first = raw.split(',').next()?.trim();
    let unbracketed = first
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(first);

    if let Ok(ip) = unbracketed.parse::<IpAddr>() {
        return Some(ip);
    }

    if let Ok(sock) = first.parse::<SocketAddr>() {
        return Some(sock.ip());
    }

    None
}

pub(crate) const TOKEN_QUERY_KEYS: &[&str] = &["token", "access_token", "jwt"];

pub trait RequestExt {
    fn get_token(&self) -> Option<String>;
    fn get_api_key(&self) -> Option<String>;
    fn get_protocol(&self) -> String;
    fn get_client_ip(&self) -> String;
    fn get_client_ip_addr(&self) -> Option<IpAddr>;
    fn get_user_agent(&self) -> Option<String>;
    fn get_host(&self) -> Option<String>;
    fn get_query_values(&self, name: &str) -> Vec<String>;
    fn get_cookie_value(&self, name: &str) -> Option<String>;
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn normalize_host(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    if let Ok(authority) = raw.parse::<http::uri::Authority>() {
        return Some(authority.host().to_ascii_lowercase());
    }

    Some(raw.to_ascii_lowercase())
}

fn token_from_authorization(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn token_from_cookie(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(COOKIE)?.to_str().ok()?;
    for item in raw.split(';') {
        let item = item.trim();
        if let Some(rest) = item.strip_prefix("jwt=") {
            let value = rest.trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn token_from_query(query: Option<&str>) -> Option<String> {
    let query = query?;
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        if TOKEN_QUERY_KEYS.contains(&k) && !v.is_empty() {
            let decoded = percent_decode(v);
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    None
}

pub(crate) fn percent_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let mut bytes = s.as_bytes().iter().copied();
    while let Some(b) = bytes.next() {
        match b {
            b'+' => out.push(b' '),
            b'%' => {
                let h = bytes.next();
                let l = bytes.next();
                if let (Some(h), Some(l)) = (h, l)
                    && let (Some(hi), Some(lo)) = (from_hex(h), from_hex(l))
                {
                    out.push((hi << 4) | lo);
                    continue;
                }
                out.push(b'%');
            }
            _ => out.push(b),
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn peer_addr<B>(req: &Request<B>) -> Option<SocketAddr> {
    req.extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0)
}

fn forwarded_ip(headers: &HeaderMap) -> Option<IpAddr> {
    if let Some(xff) = header_str(headers, "x-forwarded-for")
        && let Some(ip) = parse_ip_str(xff)
    {
        return Some(ip);
    }
    if let Some(real) = header_str(headers, "x-real-ip")
        && let Some(ip) = parse_ip_str(real)
    {
        return Some(ip);
    }
    None
}

fn query_values(query: Option<&str>, name: &str) -> Vec<String> {
    let mut values = Vec::new();
    let Some(query) = query else {
        return values;
    };

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }

        let (raw_key, raw_value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };

        if percent_decode(raw_key) == name {
            values.push(percent_decode(raw_value));
        }
    }

    values
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(COOKIE)?.to_str().ok()?;
    for item in raw.split(';') {
        let item = item.trim();
        let Some((cookie_name, cookie_value)) = item.split_once('=') else {
            continue;
        };
        if cookie_name.trim() == name {
            return Some(cookie_value.trim().trim_matches('"').to_string());
        }
    }
    None
}

impl<B> RequestExt for Request<B> {
    fn get_token(&self) -> Option<String> {
        if let Some(t) = token_from_authorization(self.headers()) {
            return Some(t);
        }
        if let Some(t) = token_from_cookie(self.headers()) {
            return Some(t);
        }
        token_from_query(self.uri().query())
    }

    fn get_api_key(&self) -> Option<String> {
        let v = header_str(self.headers(), "x-api-key")?;
        if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        }
    }

    fn get_protocol(&self) -> String {
        let is_ws = self
            .headers()
            .get(http::header::UPGRADE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false);
        if is_ws { "ws".into() } else { "http".into() }
    }

    fn get_client_ip(&self) -> String {
        self.get_client_ip_addr()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn get_client_ip_addr(&self) -> Option<IpAddr> {
        let peer_ip = peer_addr(self).map(|s| s.ip());
        if let Some(ref peer) = peer_ip
            && crate::etc::proxy::is_trusted_proxy(peer)
            && let Some(ip) = forwarded_ip(self.headers())
        {
            return Some(ip);
        }
        peer_ip
    }

    fn get_user_agent(&self) -> Option<String> {
        let v = header_str(self.headers(), USER_AGENT.as_str())?;
        if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        }
    }

    fn get_host(&self) -> Option<String> {
        if let Some(host) = header_str(self.headers(), "host").and_then(normalize_host) {
            return Some(host);
        }

        self.uri()
            .authority()
            .map(|authority| authority.host().to_ascii_lowercase())
    }

    fn get_query_values(&self, name: &str) -> Vec<String> {
        query_values(self.uri().query(), name)
    }

    fn get_cookie_value(&self, name: &str) -> Option<String> {
        cookie_value(self.headers(), name)
    }
}

#[cfg(test)]
mod tests {
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
        let r = req_with(&[("upgrade", "websocket")], None);
        assert_eq!(r.get_protocol(), "ws");
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
    fn user_agent_returns_header() {
        let r = req_with(&[("user-agent", "curl/8")], None);
        assert_eq!(r.get_user_agent().as_deref(), Some("curl/8"));
    }
}
