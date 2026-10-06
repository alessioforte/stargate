use super::*;
use http::{HeaderMap, HeaderValue};

#[test]
fn ingress_sanitizer_removes_all_spoofed_context_and_replaces_request_id() {
    let mut headers = HeaderMap::new();
    headers.append(
        INTERNAL_CONTEXT_HEADER.clone(),
        HeaderValue::from_static("attacker-one"),
    );
    headers.append(
        INTERNAL_CONTEXT_HEADER.clone(),
        HeaderValue::from_static("attacker-two"),
    );
    headers.append(
        REQUEST_ID_HEADER.clone(),
        HeaderValue::from_static("attacker-request-one"),
    );
    headers.append(
        REQUEST_ID_HEADER.clone(),
        HeaderValue::from_static("attacker-request-two"),
    );

    sanitize_ingress_headers(&mut headers, "01JZ000000000000000000000R").unwrap();

    assert!(
        headers
            .get_all(&INTERNAL_CONTEXT_HEADER)
            .iter()
            .next()
            .is_none()
    );
    assert_eq!(
        headers
            .get_all(&REQUEST_ID_HEADER)
            .iter()
            .collect::<Vec<_>>(),
        vec!["01JZ000000000000000000000R"]
    );
}
