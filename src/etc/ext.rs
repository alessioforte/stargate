use actix_web::{HttpRequest, http::header::Header, web::Query};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use std::net::{IpAddr, SocketAddr};

/// Parse an IP address from a raw string that may contain a comma-separated
/// list, square brackets, or a socket address (ip:port).
fn parse_ip_str(raw: &str) -> Option<IpAddr> {
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

const KEYS: &[&str] = &["token", "access_token", "jwt"];

pub trait RequestExt {
    fn get_token(&self) -> Option<String>;
    fn get_api_key(&self) -> Option<String>;
    fn get_protocol(&self) -> String;
    fn get_client_ip(&self) -> String;
    fn get_client_ip_addr(&self) -> Option<IpAddr>;
    fn get_user_agent(&self) -> Option<String>;
}

impl RequestExt for HttpRequest {
    fn get_token(&self) -> Option<String> {
        // get token from Authorization header
        let mut token = match Authorization::<Bearer>::parse(self) {
            Ok(auth) => auth.into_scheme().token().to_string(),
            Err(_) => "".to_string(),
        };

        // get token from cookie
        if token.is_empty() {
            if let Some(cookie) = self.cookie("jwt") {
                token = cookie.value().to_string();
            }
        }

        // get token from query string
        if token.is_empty() {
            let query = self.query_string();
            let entries = Query::<std::collections::HashMap<String, String>>::from_query(query);
            if let Ok(entries) = entries {
                for key in KEYS {
                    if let Some(value) = entries.get(*key) {
                        token = value.to_string();
                        break;
                    }
                }
            }
        }

        if token.is_empty() { None } else { Some(token) }
    }

    fn get_api_key(&self) -> Option<String> {
        let api_key = self.headers().get("x-api-key");
        if let Some(header_value) = api_key {
            if let Ok(key) = header_value.to_str() {
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
        None
    }

    fn get_protocol(&self) -> String {
        let header = self.headers().get("Upgrade");
        let is_ws = header.is_some() && header.unwrap() == "websocket";
        if is_ws {
            "ws".to_string()
        } else {
            "http".to_string()
        }
    }

    fn get_client_ip(&self) -> String {
        self.get_client_ip_addr()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn get_client_ip_addr(&self) -> Option<IpAddr> {
        let conn = self.connection_info();

        // Resolve the TCP peer address first — this is the direct connection IP
        // and cannot be spoofed by headers.
        let peer_ip = parse_ip_str(conn.peer_addr()?);

        // Only trust forwarded headers (X-Forwarded-For, X-Real-IP, etc.)
        // when the direct peer is a configured trusted proxy.
        // When TRUSTED_PROXIES is unset, realip is never used — safe default.
        if let Some(ref peer) = peer_ip {
            if super::proxy::is_trusted_proxy(peer) {
                if let Some(forwarded) = conn.realip_remote_addr() {
                    if let Some(ip) = parse_ip_str(forwarded) {
                        return Some(ip);
                    }
                }
            }
        }

        // Fall back to the direct peer address
        peer_ip
    }

    fn get_user_agent(&self) -> Option<String> {
        let user_agent = self.headers().get("User-Agent");
        if let Some(header_value) = user_agent {
            if let Ok(ua) = header_value.to_str() {
                if !ua.is_empty() {
                    return Some(ua.to_string());
                }
            }
        }
        None
    }
}
