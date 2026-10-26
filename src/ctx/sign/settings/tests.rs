use super::*;

#[test]
fn no_environment_is_an_absent_runtime() {
    let loaded = RuntimeSettings::from_lookup(|_| None).unwrap();
    assert!(loaded.is_none());
}

#[test]
fn rejects_non_rs256_algorithm() {
    let error = RuntimeSettings::from_lookup(|name| match name {
        ALGORITHM_ENV => Some("ES256".to_owned()),
        ISSUER_ENV | KEY_ID_ENV | PRIVATE_KEY_PATH_ENV | JWKS_PATH_ENV => {
            Some("configured".to_owned())
        }
        _ => None,
    })
    .unwrap_err();
    assert!(matches!(error, InternalContextError::UnsupportedAlgorithm));
}

#[test]
fn missing_private_key_fails_loading() {
    let error = load_with(
        |name| match name {
            ISSUER_ENV => Some("https://stargate.test/internal-context".to_owned()),
            KEY_ID_ENV => Some("stargate-internal-test".to_owned()),
            PRIVATE_KEY_PATH_ENV => Some("missing-private.pem".to_owned()),
            JWKS_PATH_ENV => Some("jwks.json".to_owned()),
            _ => None,
        },
        |_| Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
    )
    .unwrap_err();
    assert!(matches!(error, InternalContextError::ReadFile { .. }));
}
