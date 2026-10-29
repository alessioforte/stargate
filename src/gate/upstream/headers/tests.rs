use super::*;

#[test]
fn forwarding_uses_the_connection_peer_and_original_host_or_authority() {
    for (host, uri, expected) in [
        (Some("Frontend.test:8443"), "/orders", "Frontend.test:8443"),
        (
            None,
            "https://frontend.test:8443/orders",
            "frontend.test:8443",
        ),
    ] {
        let mut req = http::Request::builder().uri(uri).body(()).unwrap();
        if let Some(host) = host {
            req.headers_mut()
                .insert(http::header::HOST, host.parse().unwrap());
        }
        req.extensions_mut().insert(axum::extract::ConnectInfo(
            "[2001:db8::10]:12345".parse::<SocketAddr>().unwrap(),
        ));
        req.headers_mut()
            .insert("x-forwarded-for", "1.2.3.4".parse().unwrap());
        let ingress = UpstreamHeaders::from_request(&req, true);
        *req.uri_mut() = "https://backend.test/rewritten".parse().unwrap();
        req.headers_mut()
            .insert(http::header::HOST, "rewritten.test".parse().unwrap());
        forward_from_peer(req.headers_mut(), &ingress, false);
        assert_eq!(req.headers()["x-forwarded-for"], "2001:db8::10");
        assert_eq!(req.headers()["x-forwarded-host"], expected);
        assert_eq!(req.headers()["x-forwarded-proto"], "https");
        assert!(!req.headers().contains_key("forwarded"));
        assert!(!req.headers().contains_key("x-real-ip"));
    }
}

#[test]
fn trusted_forwarding_merges_all_for_fields_in_order_and_keeps_proxy_metadata() {
    let ingress = UpstreamHeaders {
        peer: Some("2001:db8::10".parse().unwrap()),
        original_host: Some("gateway.local".parse().unwrap()),
        ..Default::default()
    };
    let mut headers = HeaderMap::new();
    headers.append("x-forwarded-for", "1.2.3.4, 5.6.7.8".parse().unwrap());
    headers.append("x-forwarded-for", "10.0.0.1".parse().unwrap());
    for (name, value) in [
        ("x-forwarded-host", "public.test:8443"),
        ("x-forwarded-proto", "https"),
        ("x-forwarded-port", "8443"),
        ("x-forwarded-prefix", "/api"),
        ("forwarded", "for=1.2.3.4;proto=https"),
        ("x-real-ip", "1.2.3.4"),
    ] {
        headers.insert(name, value.parse().unwrap());
    }
    forward_from_peer(&mut headers, &ingress, true);
    assert_eq!(headers.get_all("x-forwarded-for").iter().count(), 1);
    assert_eq!(
        headers["x-forwarded-for"],
        "1.2.3.4, 5.6.7.8, 10.0.0.1, 2001:db8::10"
    );
    assert_eq!(headers["x-forwarded-host"], "public.test:8443");
    assert_eq!(headers["x-forwarded-proto"], "https");
    assert_eq!(headers["x-forwarded-port"], "8443");
    assert_eq!(headers["x-forwarded-prefix"], "/api");
    assert_eq!(headers["forwarded"], "for=1.2.3.4;proto=https");
    assert_eq!(headers["x-real-ip"], "1.2.3.4");
}

#[test]
fn custom_forwarded_fields_require_a_trusted_peer() {
    let peer = "127.0.0.1".parse().unwrap();
    let cert = HeaderName::from_bytes(b"X-Forwarded-Client-Cert").unwrap();
    let ssl = HeaderName::from_bytes(b"X-Forwarded-Ssl").unwrap();
    for (peer, trusted) in [(Some(peer), false), (Some(peer), true), (None, false)] {
        let ingress = UpstreamHeaders {
            peer,
            ..Default::default()
        };
        let mut headers = HeaderMap::new();
        headers.append(&cert, "Subject=CN=client".parse().unwrap());
        headers.append(&cert, "Subject=CN=proxy".parse().unwrap());
        headers.insert(&ssl, "on".parse().unwrap());
        headers.insert("x-forwardedness", "keep".parse().unwrap());

        forward_from_peer(&mut headers, &ingress, trusted);

        if trusted {
            let values = headers.get_all(&cert).iter().collect::<Vec<_>>();
            assert_eq!(values, ["Subject=CN=client", "Subject=CN=proxy"]);
            assert_eq!(headers[&ssl], "on");
        } else {
            assert!(!headers.contains_key(&cert));
            assert!(!headers.contains_key(&ssl));
        }
        assert_eq!(headers["x-forwardedness"], "keep");
    }
}

#[test]
fn missing_peer_discards_claimed_addresses_without_inventing_a_hop() {
    let mut headers = HeaderMap::new();
    for name in [
        "forwarded",
        "x-real-ip",
        "x-forwarded-for",
        "x-forwarded-host",
        "x-forwarded-proto",
        "x-forwarded-port",
        "x-forwarded-prefix",
        "x-forwarded-client-cert",
        "x-forwarded-ssl",
    ] {
        headers.insert(name, "spoofed".parse().unwrap());
    }
    apply_forwarded_headers(&mut headers, &UpstreamHeaders::default());
    assert_eq!(headers.len(), 1);
    assert_eq!(headers["x-forwarded-proto"], "http");
}

#[test]
fn trusted_peer_without_proxy_metadata_gets_listener_scheme_and_original_host() {
    for listener_tls in [false, true] {
        let ingress = UpstreamHeaders {
            peer: Some("127.0.0.1".parse().unwrap()),
            original_host: Some("public.test:8080".parse().unwrap()),
            listener_tls,
            ..Default::default()
        };
        let mut headers = HeaderMap::new();
        forward_from_peer(&mut headers, &ingress, true);
        assert_eq!(headers["x-forwarded-for"], "127.0.0.1");
        assert_eq!(headers["x-forwarded-host"], "public.test:8080");
        assert_eq!(
            headers["x-forwarded-proto"],
            if listener_tls { "https" } else { "http" }
        );
        assert_eq!(headers.len(), 3);
    }
}

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

#[test]
fn external_credentials_preserve_non_session_cookie_bytes() {
    let cases: &[(&[u8], Option<&[u8]>)] = &[
        (
            br#"consent=a,b; prefs={"x":1}; jwt=t; sid=1"#,
            Some(br#"consent=a,b; prefs={"x":1}; sid=1"#),
        ),
        (b"flag; jwt=t; sid=1", Some(b"flag; sid=1")),
        (b"odd=\xff; jwt=t", Some(b"odd=\xff")),
        (
            b" \tflag\t ; jwt=t; sid=has space ",
            Some(b"flag; sid=has space"),
        ),
        (b"JWT=keep; jwt =keep; jwt=t", Some(b"JWT=keep; jwt =keep")),
        (b"jwt=t", None),
        (b"\tjwt=t ; jwt=other\t", None),
    ];
    for (input, expected) in cases {
        let mut headers = HeaderMap::new();
        headers.insert(COOKIE, HeaderValue::from_bytes(input).unwrap());

        strip_credentials(&mut headers);

        assert_eq!(headers.get(COOKIE).map(HeaderValue::as_bytes), *expected);
    }
}

#[test]
fn external_credentials_strip_session_only_cookie_fields_independently() {
    let mut headers = HeaderMap::new();
    headers.append(COOKIE, HeaderValue::from_static("consent=a,b; jwt=t"));
    headers.append(COOKIE, HeaderValue::from_static("jwt=other"));
    headers.append(COOKIE, HeaderValue::from_static("flag; prefs={\"x\":1}"));

    strip_credentials(&mut headers);

    let values = headers
        .get_all(COOKIE)
        .iter()
        .map(HeaderValue::as_bytes)
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        [
            b"consent=a,b".as_slice(),
            b"flag; prefs={\"x\":1}".as_slice()
        ]
    );
}
