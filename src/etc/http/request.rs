use http::Request;
use http::header::{AUTHORIZATION, COOKIE, HeaderMap, USER_AGENT};
use std::net::{IpAddr, SocketAddr};

/// Parse a single IP token that may be a bare IP, a bracketed IPv6 address,
/// or a socket address (`ip:port` / `[ip]:port`).
pub(crate) fn parse_single_ip(raw: &str) -> Option<IpAddr> {
    let token = raw.trim();
    if token.is_empty() {
        return None;
    }

    let unbracketed = token
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(token);

    if let Ok(ip) = unbracketed.parse::<IpAddr>() {
        return Some(ip);
    }

    if let Ok(sock) = token.parse::<SocketAddr>() {
        return Some(sock.ip());
    }

    None
}

pub(crate) const TOKEN_QUERY_KEYS: &[&str] = &["token", "access_token", "jwt"];

pub trait RequestExt {
    fn get_token(&self) -> Option<String>;
    fn get_api_key(&self) -> Option<String>;
    fn get_protocol(&self) -> &'static str;
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
fn client_from_forwarded_for(value: &str) -> Option<IpAddr> {
    select_forwarded_client(value, crate::etc::http::trusted_proxy::is_trusted_proxy)
}

fn select_forwarded_client<F>(value: &str, is_trusted: F) -> Option<IpAddr>
where
    F: Fn(&IpAddr) -> bool,
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
        if !is_trusted(&ip) {
            rightmost_untrusted = Some(ip);
        }
    }

    rightmost_untrusted.or(leftmost)
}

fn forwarded_ip(headers: &HeaderMap) -> Option<IpAddr> {
    if let Some(xff) = header_str(headers, "x-forwarded-for")
        && let Some(ip) = client_from_forwarded_for(xff)
    {
        return Some(ip);
    }
    if let Some(real) = header_str(headers, "x-real-ip")
        && let Some(ip) = parse_single_ip(real)
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

    fn get_protocol(&self) -> &'static str {
        // RFC 8441 section 5: identify extended CONNECT as WebSocket intent so
        // the HTTP/1-only proxy rejects it instead of forwarding it as HTTP.
        let extended_ws = self
            .extensions()
            .get::<hyper::ext::Protocol>()
            .is_some_and(|protocol| protocol.as_str() == "websocket");
        let is_ws = self
            .headers()
            .get_all(http::header::UPGRADE)
            .iter()
            .any(|value| {
                value
                    .as_bytes()
                    .split(|byte| *byte == b',')
                    .any(|protocol| protocol.trim_ascii().eq_ignore_ascii_case(b"websocket"))
            });
        if is_ws || extended_ws { "ws" } else { "http" }
    }

    fn get_client_ip(&self) -> String {
        self.get_client_ip_addr()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn get_client_ip_addr(&self) -> Option<IpAddr> {
        let peer_ip = peer_addr(self).map(|s| s.ip());
        if let Some(ref peer) = peer_ip
            && crate::etc::http::trusted_proxy::is_trusted_proxy(peer)
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
mod tests;
