use super::types::ResponseHeaderMutations;
use crate::etc::reqctx::{INTERNAL_CONTEXT_HEADER, REQUEST_ID_HEADER};
use ::http::{HeaderMap, HeaderName, HeaderValue};
use gate::graph::HeaderValueNode;
use http::header::{AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(super) enum InternalHeaderSanitizationError {
    #[error("the trusted request id is not a valid HTTP field value")]
    InvalidRequestId,
}

pub(super) fn apply_request_headers(
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

pub(super) fn apply_response_header_mutations(
    headers: &mut HeaderMap,
    mutations: &ResponseHeaderMutations,
) {
    for name in &mutations.remove {
        headers.remove(name);
    }
    for header in &mutations.add {
        append_header(headers, header);
    }
    for header in &mutations.set {
        insert_header(headers, header);
    }
}

fn append_header(headers: &mut HeaderMap, header: &HeaderValueNode) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(header.name.as_bytes()),
        HeaderValue::from_str(&header.value),
    ) {
        headers.append(name, value);
    }
}

pub(super) fn insert_header(headers: &mut HeaderMap, header: &HeaderValueNode) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(header.name.as_bytes()),
        HeaderValue::from_str(&header.value),
    ) {
        headers.insert(name, value);
    }
}

pub(super) fn apply_gateway_headers(headers: &mut HeaderMap, additions: &HeaderMap) {
    for (name, value) in additions {
        headers.insert(name, value.clone());
    }
}

/// Final context-enabled HTTP egress boundary. It runs after configurable
/// request middleware, so stripped credentials and gateway-owned fields cannot
/// be reintroduced by configuration.
pub(super) fn sanitize_internal_request_headers(
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

pub(super) fn strip_internal_context_response(headers: &mut HeaderMap) {
    headers.remove(&INTERNAL_CONTEXT_HEADER);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_sanitizer_removes_credentials_and_replaces_owned_headers() {
        let mut headers = HeaderMap::new();
        headers.append(AUTHORIZATION, HeaderValue::from_static("Bearer first"));
        headers.append(AUTHORIZATION, HeaderValue::from_static("Bearer second"));
        headers.insert(
            PROXY_AUTHORIZATION,
            HeaderValue::from_static("Basic secret"),
        );
        headers.insert("x-api-key", HeaderValue::from_static("api-secret"));
        headers.insert("baggage", HeaderValue::from_static("user=secret"));
        headers.append(
            &INTERNAL_CONTEXT_HEADER,
            HeaderValue::from_static("stale-one"),
        );
        headers.append(
            &INTERNAL_CONTEXT_HEADER,
            HeaderValue::from_static("stale-two"),
        );
        headers.append(&REQUEST_ID_HEADER, HeaderValue::from_static("attacker-one"));
        headers.append(&REQUEST_ID_HEADER, HeaderValue::from_static("attacker-two"));
        headers.append(
            COOKIE,
            HeaderValue::from_static("theme=dark; jwt=session-secret; cart=a%2Fb"),
        );

        sanitize_internal_request_headers(&mut headers, "01JZ000000000000000000000R").unwrap();

        for name in [
            AUTHORIZATION,
            PROXY_AUTHORIZATION,
            HeaderName::from_static("x-api-key"),
            HeaderName::from_static("baggage"),
            INTERNAL_CONTEXT_HEADER,
        ] {
            assert!(headers.get_all(name).iter().next().is_none());
        }
        assert_eq!(headers.get_all(REQUEST_ID_HEADER).iter().count(), 1);
        assert_eq!(headers[REQUEST_ID_HEADER], "01JZ000000000000000000000R");
        assert_eq!(headers[COOKIE], "theme=dark; cart=a%2Fb");
    }

    #[test]
    fn cookie_removal_is_exact_and_drops_an_empty_result() {
        let mut headers = HeaderMap::new();
        headers.append(COOKIE, HeaderValue::from_static("JWT=keep; jwt=remove"));
        headers.append(COOKIE, HeaderValue::from_static("jwt=remove-too"));

        sanitize_cookies(&mut headers);

        let values = headers
            .get_all(COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(values, vec!["JWT=keep"]);
    }

    #[test]
    fn any_ambiguous_cookie_omits_the_whole_cookie_header() {
        let mut headers = HeaderMap::new();
        headers.append(COOKIE, HeaderValue::from_static("safe=one"));
        headers.append(COOKIE, HeaderValue::from_static("jwt=secret; malformed"));

        sanitize_cookies(&mut headers);

        assert!(headers.get_all(COOKIE).iter().next().is_none());
    }
}
