use super::*;
use crate::etc::gate::test_support::{CLIENT_CERT, CLIENT_KEY, SERVER_KEY, TlsFiles};

#[test]
fn valid_mtls_and_default_transports_prepare() {
    let files = TlsFiles::new();
    let tls = build_mtls(&files.mtls()).unwrap();
    assert!(tls.alpn_protocols.is_empty());
    let config = RuntimeConfig::from_raw(files.config("https://localhost:8443")).unwrap();
    assert!(prepare(&config).unwrap().get("secure").is_some());
    assert!(prepare(&RuntimeConfig::from_raw(gate::cfg::Config::default()).unwrap()).is_ok());
}

#[test]
fn missing_empty_malformed_and_invalid_der_files_return_typed_errors() {
    let files = TlsFiles::new();
    let invalid_files = [
        files.path("missing.pem"),
        files.write("empty.pem", b""),
        files.write(
            "malformed.pem",
            b"-----BEGIN CERTIFICATE-----\n!invalid!\n-----END CERTIFICATE-----\n",
        ),
        files.write(
            "invalid-der.pem",
            b"-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n",
        ),
    ];
    for kind in ["CA", "client", "client key"] {
        for path in &invalid_files {
            let mut mtls = files.mtls();
            match kind {
                "CA" => mtls.ca_cert_path = path.clone(),
                "client" => mtls.client_cert_path = path.clone(),
                _ => mtls.client_key_path = path.clone(),
            }
            let error = build_mtls(&mtls)
                .err()
                .expect("invalid TLS material was accepted");
            assert!(matches!(
                error,
                TransportPreparationError::ReadFile { .. }
                    | TransportPreparationError::Pem { .. }
                    | TransportPreparationError::EmptyCertificates { .. }
                    | TransportPreparationError::Certificate { .. }
                    | TransportPreparationError::PrivateKeyCount
            ));
        }
    }
    let mut mtls = files.mtls();
    mtls.client_key_path = files.write(
        "invalid-key-der.pem",
        b"-----BEGIN PRIVATE KEY-----\nAQID\n-----END PRIVATE KEY-----\n",
    );
    assert!(matches!(
        build_mtls(&mtls),
        Err(TransportPreparationError::ClientIdentity(_))
    ));
}

#[test]
fn mismatched_keys_and_duplicate_keys_are_rejected() {
    let files = TlsFiles::new();
    let mut mtls = files.mtls();
    mtls.client_key_path = files.write("other-key.pem", SERVER_KEY);
    assert!(matches!(
        build_mtls(&mtls),
        Err(TransportPreparationError::ClientIdentity(
            rustls::Error::InconsistentKeys(_)
        ))
    ));
    mtls.client_key_path = files.write("duplicate-keys.pem", &[CLIENT_KEY, CLIENT_KEY].concat());
    assert!(matches!(
        build_mtls(&mtls),
        Err(TransportPreparationError::PrivateKeyCount)
    ));
}

#[test]
fn malformed_intermediate_certificate_is_rejected() {
    let files = TlsFiles::new();
    let mut mtls = files.mtls();
    mtls.client_cert_path = files.write(
        "bad-chain.pem",
        &[
            CLIENT_CERT,
            b"-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n",
        ]
        .concat(),
    );
    assert!(matches!(
        build_mtls(&mtls),
        Err(TransportPreparationError::Certificate { kind: "client", .. })
    ));
}

#[test]
fn duration_validation_rejects_zero_invalid_and_overflowing_values() {
    for value in [
        "",
        "invalid",
        "-1s",
        "0s",
        "0ms",
        "9223372036854775807s",
        "99999999999999999999999d",
    ] {
        assert!(matches!(
            connect_timeout("secure", Some(value), Duration::from_secs(5)),
            Err(TransportPreparationError::ConnectTimeout { .. })
        ));
    }
    assert_eq!(
        connect_timeout("secure", None, Duration::from_secs(5)).unwrap(),
        Duration::from_secs(5)
    );
    assert_eq!(
        connect_timeout("secure", Some("500ms"), Duration::from_secs(5)).unwrap(),
        Duration::from_millis(500)
    );
}

#[test]
fn durations_on_unused_upstreams_are_validated() {
    let config: gate::cfg::Config = serde_json::from_value(serde_json::json!({
        "schema": gate::cfg::SCHEMA,
        "http": { "upstreams": { "unused": { "targets": [{ "url": "http://localhost:8443" }], "transport": { "connect_timeout": "bad" } } } },
    })).unwrap();
    assert!(matches!(
        prepare(&RuntimeConfig::from_raw(config).unwrap()),
        Err(TransportPreparationError::ConnectTimeout { .. })
    ));
}

#[test]
fn unexpected_alpn_is_an_error_instead_of_a_connector_panic() {
    let mut tls = default_tls_config().unwrap();
    tls.alpn_protocols = vec![b"h2".to_vec()];
    assert!(matches!(
        build_hyper_client(Duration::from_secs(1), &tls),
        Err(TransportPreparationError::Alpn)
    ));
}
