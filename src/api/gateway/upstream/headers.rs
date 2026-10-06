use crate::etc::request_context::{INTERNAL_CONTEXT_HEADER, REQUEST_ID_HEADER};
use ::http::{HeaderMap, HeaderName, HeaderValue};
use gate::graph::HeaderValueNode;
use http::header::{AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(in crate::api::gateway) enum InternalHeaderSanitizationError {
    #[error("the trusted request id is not a valid HTTP field value")]
    InvalidRequestId,
}

pub(in crate::api::gateway) fn apply_request_headers(
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

pub(in crate::api::gateway) fn append_header(headers: &mut HeaderMap, header: &HeaderValueNode) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(header.name.as_bytes()),
        HeaderValue::from_str(&header.value),
    ) {
        headers.append(name, value);
    }
}

pub(in crate::api::gateway) fn insert_header(headers: &mut HeaderMap, header: &HeaderValueNode) {
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
pub(in crate::api::gateway) fn sanitize_internal_request_headers(
    headers: &mut HeaderMap,
    request_id: &str,
) -> Result<(), InternalHeaderSanitizationError> {
    headers.remove(AUTHORIZATION);
    headers.remove(PROXY_AUTHORIZATION);
    headers.remove("x-api-key");
    headers.remove("baggage");
    headers.remove(&INTERNAL_CONTEXT_HEADER);
    headers.remove(&REQUEST_ID_HEADER);

    sanitize_cookies(headers);

    let request_id = HeaderValue::from_str(request_id)
        .map_err(|_| InternalHeaderSanitizationError::InvalidRequestId)?;
    headers.insert(&REQUEST_ID_HEADER, request_id);
    Ok(())
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
pub(in crate::api::gateway) fn strip_hop_by_hop_headers(headers: &mut http::HeaderMap) {
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
