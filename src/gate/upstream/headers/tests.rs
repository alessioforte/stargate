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
fn final_internal_sanitizer_removes_credentials_added_by_request_middleware() {
    let mut headers = HeaderMap::new();
    apply_request_headers(
        &mut headers,
        &[
            HeaderValueNode {
                name: "authorization".to_owned(),
                value: "Bearer middleware-secret".to_owned(),
            },
            HeaderValueNode {
                name: "x-api-key".to_owned(),
                value: "middleware-api-key".to_owned(),
            },
            HeaderValueNode {
                name: "baggage".to_owned(),
                value: "private=middleware".to_owned(),
            },
        ],
        &[
            HeaderValueNode {
                name: "cookie".to_owned(),
                value: "theme=dark; jwt=middleware-session".to_owned(),
            },
            HeaderValueNode {
                name: "x-request-id".to_owned(),
                value: "middleware-request-id".to_owned(),
            },
        ],
        &[],
    );

    sanitize_internal_request_headers(&mut headers, "01JZ000000000000000000000R").unwrap();

    assert!(headers.get(AUTHORIZATION).is_none());
    assert!(headers.get("x-api-key").is_none());
    assert!(headers.get("baggage").is_none());
    assert_eq!(headers[COOKIE], "theme=dark");
    assert_eq!(headers[REQUEST_ID_HEADER], "01JZ000000000000000000000R");
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
