use actix_web::{HttpRequest, http::header::Header, web::Query};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use std::net::IpAddr;

const KEYS: &[&str] = &["token", "access_token", "jwt"];

pub trait RequestExt {
    fn get_token(&self) -> Option<String>;
    fn get_api_key(&self) -> Option<String>;
    fn get_protocol(&self) -> String;
    fn get_client_ip(&self) -> Option<IpAddr>;
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

    fn get_client_ip(&self) -> Option<IpAddr> {
        // Check X-Forwarded-For header first
        if let Some(forwarded_for) = self.headers().get("X-Forwarded-For") {
            if let Ok(forwarded_for_str) = forwarded_for.to_str() {
                if let Some(first_ip) = forwarded_for_str.split(',').next() {
                    if let Ok(ip) = first_ip.trim().parse() {
                        return Some(ip);
                    }
                }
            }
        }

        // Check Forwarded header (RFC 7239)
        if let Some(forwarded) = self.headers().get("Forwarded") {
            if let Ok(forwarded_str) = forwarded.to_str() {
                for part in forwarded_str.split(';') {
                    if part.trim_start().starts_with("for=") {
                        let for_part = part.trim_start().trim_start_matches("for=").trim();
                        let for_part = for_part.trim_matches('"');
                        if let Some(ip_str) = for_part.split(',').next() {
                            if let Ok(ip) = ip_str.trim().parse() {
                                return Some(ip);
                            }
                        }
                    }
                }
            }
        }

        let mut ip = self.peer_addr().map(|addr| addr.ip())?;

        // customers often get their own /56 prefix, apply rate-limiting per prefix instead of per
        // address for IPv6
        if let IpAddr::V6(ipv6) = ip {
            let mut octets = ipv6.octets();
            octets[7..16].fill(0);
            ip = IpAddr::V6(octets.into());
        }

        Some(ip)
    }
}
