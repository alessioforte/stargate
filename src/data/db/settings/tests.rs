use super::*;

fn settings(values: &[(&str, &str)]) -> Result<PostgresSettings> {
    PostgresSettings::from_lookup(|name| {
        Ok(values
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.to_string()))
    })
}

#[test]
fn endpoint_formats_preserve_host_and_port() {
    for (value, host, port) in [
        ("db.example.com", "db.example.com", 5432),
        ("db.example.com:6543", "db.example.com", 6543),
        ("[::1]", "::1", 5432),
        ("[::1]:6543", "::1", 6543),
        ("127.0.0.1:6543", "127.0.0.1", 6543),
    ] {
        assert_eq!(parse_endpoint(value).unwrap(), (host.to_string(), port));
    }
}

#[test]
fn malformed_endpoints_are_rejected() {
    for value in [
        "",
        " ",
        ":5432",
        "db:",
        "db:zero",
        "db:0",
        "db:65536",
        "db:+5432",
        "db:5432:6543",
        "postgres://db",
        "db/path",
        "user@db",
        "db?query",
        "db#fragment",
        "db name",
        "*.example.com",
        "-db",
        "db-",
        "db..example.com",
        "::1",
        "[::1",
        "[::1]extra",
        "[::1]:",
        "[::1]:65536",
        "[localhost]",
        "db]",
    ] {
        assert!(parse_endpoint(value).is_err(), "{value}");
    }
}

#[test]
fn defaults_preserve_existing_credentials_and_prefer_tls() {
    let settings = settings(&[]).unwrap();
    let options = settings.connect_options();
    assert_eq!(options.get_host(), "localhost");
    assert_eq!(options.get_port(), 5432);
    assert_eq!(options.get_username(), "root");
    assert_eq!(options.get_database(), Some("stargate"));
    assert!(matches!(options.get_ssl_mode(), PgSslMode::Prefer));
    assert!(settings.create_database);
}

#[test]
fn ssl_modes_are_selected_explicitly() {
    for (value, expected) in [
        ("disable", PgSslMode::Disable),
        ("prefer", PgSslMode::Prefer),
        ("require", PgSslMode::Require),
        ("verify-ca", PgSslMode::VerifyCa),
        ("verify-full", PgSslMode::VerifyFull),
    ] {
        let settings = settings(&[("POSTGRES_SSLMODE", value)]).unwrap();
        assert_eq!(
            format!("{:?}", settings.connect_options().get_ssl_mode()),
            format!("{expected:?}")
        );
    }
}

#[test]
fn unsupported_ssl_modes_are_rejected() {
    for value in ["allow", "", "invalid", "true"] {
        assert!(settings(&[("POSTGRES_SSLMODE", value)]).is_err(), "{value}");
    }
}

#[test]
fn database_creation_can_be_disabled() {
    for (value, expected) in [("true", true), ("false", false)] {
        assert_eq!(
            settings(&[("POSTGRES_CREATE_DATABASE", value)])
                .unwrap()
                .create_database,
            expected
        );
    }
}

#[test]
fn non_boolean_database_creation_is_rejected() {
    for value in ["yes", "1", "", "invalid"] {
        assert!(
            settings(&[("POSTGRES_CREATE_DATABASE", value)]).is_err(),
            "{value}"
        );
    }
}

#[test]
fn special_characters_are_passed_to_options_without_url_encoding() {
    let settings = settings(&[
        ("POSTGRES_USERNAME", "u@ser:/?#%"),
        ("POSTGRES_PASSWORD", "p@ss:w/rd?#%"),
        ("POSTGRES_DATABASE", "db/name?value#%"),
        ("POSTGRES_ENDPOINT", "[::1]:6543"),
        ("POSTGRES_SSLROOTCERT", "/certificates/private-ca.pem"),
    ])
    .unwrap();
    let options = settings.connect_options();
    assert_eq!(options.get_host(), "::1");
    assert_eq!(options.get_port(), 6543);
    assert_eq!(options.get_username(), "u@ser:/?#%");
    assert_eq!(options.get_database(), Some("db/name?value#%"));
    // sqlx has no password getter. Inspect Debug only for this synthetic-value test.
    let debug = format!("{options:?}");
    assert!(debug.contains("password: Some(\"p@ss:w/rd?#%\")"));
    assert!(debug.contains("/certificates/private-ca.pem"));
}
