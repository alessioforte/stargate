use crate::ctx::{INTERNAL_CONTEXT_HEADER, REQUEST_ID_HEADER};
use ::http::{HeaderMap, HeaderName, HeaderValue};
use gate::graph::HeaderValueNode;
use http::header::{AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION};
use std::net::{IpAddr, SocketAddr};

/// Ingress facts survive URI/header rewrites and every replayed dispatch.
#[derive(Debug, Clone, Default)]
pub(in crate::gate) struct UpstreamHeaders {
    pub(in crate::gate) preserve_host: bool,
    pub(in crate::gate) peer: Option<IpAddr>,
    pub(in crate::gate) original_host: Option<HeaderValue>,
    pub(in crate::gate) listener_tls: bool,
    pub(in crate::gate) forward_credentials: bool,
}

impl UpstreamHeaders {
    pub(in crate::gate) fn from_request<B>(req: &http::Request<B>, listener_tls: bool) -> Self {
        Self {
            preserve_host: false,
            peer: req
                .extensions()
                .get::<axum::extract::ConnectInfo<SocketAddr>>()
                .map(|peer| peer.0.ip()),
            original_host: req.headers().get(http::header::HOST).cloned().or_else(|| {
                req.uri()
                    .authority()
                    .and_then(|authority| HeaderValue::from_str(authority.as_str()).ok())
            }),
            listener_tls,
            forward_credentials: false,
        }
    }
}

pub(in crate::gate) fn apply_forwarded_headers(headers: &mut HeaderMap, ingress: &UpstreamHeaders) {
    let trusted = ingress
        .peer
        .is_some_and(|peer| crate::web::trusted_proxy::is_trusted_proxy(&peer));
    forward_from_peer(headers, ingress, trusted);
}

fn forward_from_peer(headers: &mut HeaderMap, ingress: &UpstreamHeaders, trusted: bool) {
    if !trusted {
        let untrusted = headers
            .keys()
            .filter(|name| {
                matches!(name.as_str(), "forwarded" | "x-real-ip")
                    || name.as_str().starts_with("x-forwarded-")
            })
            .cloned()
            .collect::<Vec<_>>();
        for name in untrusted {
            headers.remove(name);
        }
    }

    if let Some(peer) = ingress.peer {
        let mut chain = Vec::new();
        for value in headers.get_all("x-forwarded-for") {
            chain.extend_from_slice(value.as_bytes());
            chain.extend_from_slice(b", ");
        }
        chain.extend_from_slice(peer.to_string().as_bytes());
        // Existing field values and an IP address contain no forbidden bytes.
        headers.insert("x-forwarded-for", HeaderValue::from_bytes(&chain).unwrap());
    }
    if !headers.contains_key("x-forwarded-proto") {
        headers.insert(
            "x-forwarded-proto",
            HeaderValue::from_static(if ingress.listener_tls {
                "https"
            } else {
                "http"
            }),
        );
    }
    if !headers.contains_key("x-forwarded-host")
        && let Some(host) = &ingress.original_host
    {
        headers.insert("x-forwarded-host", host.clone());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(in crate::gate) enum InternalHeaderSanitizationError {
    #[error("the trusted request id is not a valid HTTP field value")]
    InvalidRequestId,
}

pub(in crate::gate) fn apply_request_headers(
    headers: &mut HeaderMap,
    add: &[HeaderValueNode],
    set: &[HeaderValueNode],
    remove: &[String],
) {
    for name in remove {
        headers.remove(name);
    }
    for header in add {
        append_header(headers, header);
    }
    for header in set {
        insert_header(headers, header);
    }
}

pub(in crate::gate) fn append_header(headers: &mut HeaderMap, header: &HeaderValueNode) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(header.name.as_bytes()),
        HeaderValue::from_str(&header.value),
    ) {
        headers.append(name, value);
    }
}

pub(in crate::gate) fn insert_header(headers: &mut HeaderMap, header: &HeaderValueNode) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(header.name.as_bytes()),
        HeaderValue::from_str(&header.value),
    ) {
        headers.insert(name, value);
    }
}

/// Final context-enabled HTTP egress boundary. It runs after configurable
/// request middleware, so stripped credentials and gateway-owned fields cannot
/// be reintroduced by configuration.
pub(in crate::gate) fn sanitize_internal_request_headers(
    headers: &mut HeaderMap,
    request_id: &str,
) -> Result<(), InternalHeaderSanitizationError> {
    strip_credential_headers(headers);
    sanitize_cookies(headers);
    headers.remove("baggage");
    headers.remove(&INTERNAL_CONTEXT_HEADER);
    headers.remove(&REQUEST_ID_HEADER);

    let request_id = HeaderValue::from_str(request_id)
        .map_err(|_| InternalHeaderSanitizationError::InvalidRequestId)?;
    headers.insert(&REQUEST_ID_HEADER, request_id);
    Ok(())
}

pub(in crate::gate) fn strip_credentials(headers: &mut HeaderMap) {
    strip_credential_headers(headers);
    let cookies = headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| {
            let retained = value
                .as_bytes()
                .split(|byte| *byte == b';')
                // Space and tab are the only ASCII whitespace allowed in a header value.
                .map(<[u8]>::trim_ascii)
                .filter(|item| !item.starts_with(b"jwt="))
                .collect::<Vec<_>>()
                .join(b"; ".as_slice());
            if retained.is_empty() {
                None
            } else {
                Some(HeaderValue::from_bytes(&retained).unwrap())
            }
        })
        .collect::<Vec<_>>();
    headers.remove(COOKIE);
    for value in cookies {
        headers.append(COOKIE, value);
    }
}

fn strip_credential_headers(headers: &mut HeaderMap) {
    headers.remove(AUTHORIZATION);
    headers.remove(PROXY_AUTHORIZATION);
    headers.remove("x-api-key");
}

fn sanitize_cookies(headers: &mut HeaderMap) {
    let mut sanitized = Vec::new();
    let mut ambiguous = false;

    for value in headers.get_all(COOKIE) {
        match sanitize_cookie_field(value) {
            CookieField::Keep(value) => sanitized.push(value),
            CookieField::Drop => {}
            CookieField::Ambiguous => {
                ambiguous = true;
                break;
            }
        }
    }

    headers.remove(COOKIE);
    if ambiguous {
        return;
    }
    for value in sanitized {
        headers.append(COOKIE, value);
    }
}

enum CookieField {
    Keep(HeaderValue),
    Drop,
    Ambiguous,
}

fn sanitize_cookie_field(value: &HeaderValue) -> CookieField {
    let Ok(value) = value.to_str() else {
        return CookieField::Ambiguous;
    };
    let mut retained = Vec::new();

    for pair in value.split(';') {
        let pair = pair.trim_matches([' ', '\t']);
        let Some((name, value)) = pair.split_once('=') else {
            return CookieField::Ambiguous;
        };
        if !valid_cookie_name(name) || !valid_cookie_value(value) {
            return CookieField::Ambiguous;
        }
        if name != "jwt" {
            retained.push(pair);
        }
    }

    if retained.is_empty() {
        return CookieField::Drop;
    }
    match HeaderValue::from_str(&retained.join("; ")) {
        Ok(value) => CookieField::Keep(value),
        Err(_) => CookieField::Ambiguous,
    }
}

fn valid_cookie_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_graphic()
                && !matches!(
                    byte,
                    b'(' | b')'
                        | b'<'
                        | b'>'
                        | b'@'
                        | b','
                        | b';'
                        | b':'
                        | b'\\'
                        | b'"'
                        | b'/'
                        | b'['
                        | b']'
                        | b'?'
                        | b'='
                        | b'{'
                        | b'}'
                )
        })
}

fn valid_cookie_value(value: &str) -> bool {
    let value = match value.as_bytes() {
        [b'"', inner @ .., b'"'] => inner,
        bytes if !bytes.contains(&b'"') => bytes,
        _ => return false,
    };
    value.iter().copied().all(is_cookie_octet)
}

fn is_cookie_octet(byte: u8) -> bool {
    matches!(byte, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e)
}

// RFC 9110 section 7.6.1: remove Connection-nominated fields before forwarding.
pub(in crate::gate) fn strip_hop_by_hop_headers(headers: &mut http::HeaderMap) {
    let mut connection_headers = Vec::new();
    for value in headers.get_all(http::header::CONNECTION) {
        for header in value.as_bytes().split(|byte| *byte == b',') {
            if let Ok(name) = http::header::HeaderName::from_bytes(header.trim_ascii()) {
                connection_headers.push(name);
            }
        }
    }

    for name in connection_headers {
        headers.remove(name);
    }

    headers.remove(http::header::CONNECTION);
    headers.remove("keep-alive");
    headers.remove(http::header::PROXY_AUTHENTICATE);
    headers.remove(PROXY_AUTHORIZATION);
    headers.remove(http::header::TE);
    headers.remove(http::header::TRAILER);
    headers.remove(http::header::TRANSFER_ENCODING);
    headers.remove(http::header::UPGRADE);
    headers.remove("proxy-connection");
    headers.remove("trailers");
}

#[cfg(test)]
mod tests;
