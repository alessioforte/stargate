use super::*;

#[test]
fn auth_query_sanitizer_decodes_only_keys_and_preserves_every_other_pair() {
    let query =
        "a=1&%74oken=secret&encoded=%2Fkeep%2F&access%5Ftoken=x&JWT=keep&j%77t=y&empty=&flag";

    let sanitized = sanitize_auth_query(query, QueryMode::Strict).unwrap();

    assert_eq!(sanitized, "a=1&encoded=%2Fkeep%2F&JWT=keep&empty=&flag");
}

#[test]
fn malformed_percent_encoding_in_a_query_key_fails_closed() {
    assert!(sanitize_auth_query("safe=1&tok%en=secret", QueryMode::Strict).is_err());
    assert!(sanitize_auth_query("safe=1&token%=secret", QueryMode::Strict).is_err());
}

#[test]
fn auth_uri_sanitizer_preserves_authority_path_and_safe_encoding() {
    let uri: http::Uri = "http://orders.test/v1/a%2Fb?z=%2F&jwt=secret&a=1"
        .parse()
        .unwrap();

    let sanitized = sanitize_auth_uri(&uri, QueryMode::Strict).unwrap();

    assert_eq!(sanitized, "http://orders.test/v1/a%2Fb?z=%2F&a=1");
}

#[test]
fn auth_uri_sanitizer_removes_an_empty_query_and_keeps_uris_without_tokens() {
    for (input, expected) in [
        (
            "http://orders.test/orders?access_token=a&page=2&jwt=b",
            "http://orders.test/orders?page=2",
        ),
        (
            "http://orders.test/orders?access%5Ftoken=a&page=2&jwt=b",
            "http://orders.test/orders?page=2",
        ),
        (
            "http://orders.test/orders?token=a&access_token=b&j%77t=c",
            "http://orders.test/orders",
        ),
        (
            "http://orders.test/orders?page=2&keep=%2F",
            "http://orders.test/orders?page=2&keep=%2F",
        ),
        ("ws://orders.test/socket", "ws://orders.test/socket"),
    ] {
        assert_eq!(
            sanitize_auth_uri(&input.parse().unwrap(), QueryMode::Strict).unwrap(),
            expected
        );
    }
}

#[test]
fn malformed_query_names_are_retained_only_in_tolerant_mode() {
    for malformed in ["50%=off", "a%ZZ=1", "tok%en=x", "token%=x"] {
        let uri = format!("http://orders.test/orders?{malformed}&token=x")
            .parse()
            .unwrap();
        assert_eq!(
            sanitize_auth_uri(&uri, QueryMode::Tolerant)
                .unwrap()
                .query(),
            Some(malformed)
        );
        assert!(sanitize_auth_uri(&uri, QueryMode::Strict).is_err());
    }
}
