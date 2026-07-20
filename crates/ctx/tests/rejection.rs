mod common;

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ctx::{
    ActorType, AuthenticationKind, ClaimValidationError, ConfigError, ContextVerifier,
    ExpectedRequest, IssueError, KeyMaterialError, PresentationError, SignerConfig,
    StaticKeyResolver, VerificationError, VerificationKey, VerifierConfig, require_single_token,
    truncate_utf8,
};
use jsonwebtoken::{Algorithm, EncodingKey};

use common::*;

fn signed_parts(header: &[u8], payload: &[u8]) -> String {
    let header = URL_SAFE_NO_PAD.encode(header);
    let payload = URL_SAFE_NO_PAD.encode(payload);
    let message = format!("{header}.{payload}");
    let key = EncodingKey::from_rsa_pem(PRIVATE_KEY).unwrap();
    let signature = jsonwebtoken::crypto::sign(message.as_bytes(), &key, Algorithm::RS256).unwrap();
    format!("{message}.{signature}")
}

fn signed_json(header: &str, payload: &serde_json::Value) -> String {
    signed_parts(header.as_bytes(), &serde_json::to_vec(payload).unwrap())
}

fn valid_payload() -> serde_json::Value {
    let token = signer().issue(&user_request()).unwrap().into_compact();
    let encoded = token.split('.').nth(1).unwrap();
    let decoded = URL_SAFE_NO_PAD.decode(encoded).unwrap();
    serde_json::from_slice(&decoded).unwrap()
}

fn resign(payload: &serde_json::Value) -> String {
    signed_json(
        r#"{"alg":"RS256","kid":"stargate-internal-test","typ":"stargate-context+jwt"}"#,
        payload,
    )
}

#[test]
fn rejects_cross_audience_issuer_and_request_binding() {
    let token = signer().issue(&user_request()).unwrap().into_compact();
    let resolver = || Arc::new(StaticKeyResolver::from_rsa_pem(KEY_ID, PUBLIC_KEY).unwrap());
    let clock = || Arc::new(FixedClock(NOW));

    let wrong_audience = ContextVerifier::with_clock(
        VerifierConfig::new(ISSUER, "urn:stargate:service:billing", 5).unwrap(),
        resolver(),
        clock(),
    );
    assert_eq!(
        wrong_audience.verify(&token, expected()),
        Err(VerificationError::AudienceMismatch)
    );

    let wrong_issuer = ContextVerifier::with_clock(
        VerifierConfig::new("https://other.example/internal-context", AUDIENCE, 5).unwrap(),
        resolver(),
        clock(),
    );
    assert_eq!(
        wrong_issuer.verify(&token, expected()),
        Err(VerificationError::IssuerMismatch)
    );

    for (expected, error) in [
        (
            ExpectedRequest {
                method: "GET",
                ..expected()
            },
            VerificationError::MethodMismatch,
        ),
        (
            ExpectedRequest {
                encoded_path: "/v1/other",
                ..expected()
            },
            VerificationError::PathMismatch,
        ),
        (
            ExpectedRequest {
                request_id: "01JZ000000000000000000000S",
                ..expected()
            },
            VerificationError::RequestIdMismatch,
        ),
    ] {
        assert_eq!(verifier(NOW).verify(&token, expected), Err(error));
    }
}

#[test]
fn rejects_wrong_type_algorithm_and_protected_header_extensions() {
    let payload = valid_payload();
    let cases = [
        r#"{"alg":"RS256","kid":"stargate-internal-test","typ":"JWT"}"#,
        r#"{"alg":"HS256","kid":"stargate-internal-test","typ":"stargate-context+jwt"}"#,
        r#"{"alg":"none","kid":"stargate-internal-test","typ":"stargate-context+jwt"}"#,
        r#"{"alg":"RS256","kid":"stargate-internal-test","typ":"stargate-context+jwt","jku":"https://attacker.example/jwks"}"#,
        r#"{"alg":"RS256","alg":"RS256","kid":"stargate-internal-test","typ":"stargate-context+jwt"}"#,
        r#"{"alg":"RS256","kid":"bad id","typ":"stargate-context+jwt"}"#,
    ];

    for header in cases {
        let token = signed_json(header, &payload);
        assert_eq!(
            verifier(NOW).verify(&token, expected()),
            Err(VerificationError::InvalidProtectedHeader),
            "header was unexpectedly accepted: {header}"
        );
    }
}

#[test]
fn rejects_invalid_signature_and_compact_shape() {
    let mut token = signer().issue(&user_request()).unwrap().into_compact();
    let signature_start = token.rfind('.').unwrap() + 1;
    let replacement = if token.as_bytes()[signature_start] == b'A' {
        "B"
    } else {
        "A"
    };
    token.replace_range(signature_start..signature_start + 1, replacement);
    assert_eq!(
        verifier(NOW).verify(&token, expected()),
        Err(VerificationError::InvalidSignature)
    );

    for token in ["", "one", "one.two", "one.two.three.four", ".."] {
        assert_eq!(
            verifier(NOW).verify(token, expected()),
            Err(VerificationError::MalformedCompact)
        );
    }
    assert_eq!(
        verifier(NOW).verify(&"a".repeat(4_097), expected()),
        Err(VerificationError::TokenTooLarge)
    );
}

#[test]
fn rejects_expired_future_and_excessive_lifetime_tokens() {
    let token = signer().issue(&user_request()).unwrap().into_compact();
    assert_eq!(
        verifier(NOW + 36).verify(&token, expected()),
        Err(VerificationError::Expired)
    );
    assert_eq!(
        verifier(NOW - 6).verify(&token, expected()),
        Err(VerificationError::IssuedInFuture)
    );

    let mut payload = valid_payload();
    payload["exp"] = serde_json::json!(NOW + 61);
    assert_eq!(
        verifier(NOW).verify(&resign(&payload), expected()),
        Err(VerificationError::Claims(
            ClaimValidationError::LifetimeTooLong
        ))
    );

    payload["exp"] = serde_json::json!(NOW);
    assert_eq!(
        verifier(NOW).verify(&resign(&payload), expected()),
        Err(VerificationError::Claims(
            ClaimValidationError::InvalidTimeWindow
        ))
    );
}

#[test]
fn rejects_invalid_actor_organization_and_identifier_shapes() {
    let mut request = user_request();
    request.context.authentication.kind = AuthenticationKind::ApiKey;
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidActorMatrix))
    );

    let mut request = user_request();
    request.context.organization.as_mut().unwrap().role = None;
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidActorMatrix))
    );

    let mut request = user_request();
    request.context.organization.as_mut().unwrap().id = "not-an-org".to_owned();
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidUlid(
            "stg.organization.id"
        )))
    );

    let mut request = user_request();
    request.subject = Some(USER_ID.to_ascii_lowercase());
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidActorMatrix))
    );

    let mut request = anonymous_request();
    request.subject = Some(USER_ID.to_owned());
    assert_eq!(request.context.actor.actor_type, ActorType::Anonymous);
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidActorMatrix))
    );
}

#[test]
fn rejects_invalid_request_route_and_limits() {
    let mut request = user_request();
    request.context.request.trace_id = Some("ABC".to_owned());
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidTraceId))
    );

    let mut request = user_request();
    request.context.request.client_ip = Some("2001:0db8::1".to_owned());
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidClientIp))
    );

    let mut request = user_request();
    request.context.request.method = "POST request".to_owned();
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidMethod))
    );

    let mut request = user_request();
    request.context.request.path = "/v1/orders?admin=true".to_owned();
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidPath(
            "stg.request.path"
        )))
    );

    let mut request = user_request();
    request.context.route.router = "r".repeat(129);
    assert!(matches!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::TooLong {
            field: "stg.route.router",
            max: 128,
            actual: 129,
        }))
    ));

    let mut request = user_request();
    request.context.dispatch.attempt = 0;
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidAttempt))
    );
}

#[test]
fn rejects_every_remaining_structural_claim_rule() {
    let mut request = user_request();
    request.audience = " ".to_owned();
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::Blank("aud")))
    );

    let mut request = user_request();
    request.audience = "a".repeat(257);
    assert!(matches!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::TooLong {
            field: "aud",
            max: 256,
            actual: 257,
        }))
    ));

    let invalid_id_signer = ctx::ContextSigner::with_sources(
        SignerConfig::new(ISSUER, KEY_ID, 30).unwrap(),
        PRIVATE_KEY,
        Arc::new(FixedClock(NOW)),
        Arc::new(FixedId("invalid-dispatch-id")),
    )
    .unwrap();
    assert_eq!(
        invalid_id_signer.issue(&user_request()),
        Err(IssueError::Claims(ClaimValidationError::InvalidUlid("jti")))
    );

    let mut request = user_request();
    request.context.v = 2;
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidVersion))
    );

    let mut request = user_request();
    request.context.authentication.auth_time = Some(-1);
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidActorMatrix))
    );

    let mut request = user_request();
    request.context.organization.as_mut().unwrap().role = Some(" ".to_owned());
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::Blank(
            "stg.organization.role"
        )))
    );

    let mut request = user_request();
    request.context.organization.as_mut().unwrap().role = Some("r".repeat(257));
    assert!(matches!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::TooLong {
            field: "stg.organization.role",
            max: 256,
            actual: 257,
        }))
    ));

    let mut request = user_request();
    request.context.request.id = "invalid-request-id".to_owned();
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidUlid(
            "stg.request.id"
        )))
    );

    let mut request = user_request();
    request.context.request.original_path = "api/orders".to_owned();
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidPath(
            "stg.request.original_path"
        )))
    );

    let mut request = user_request();
    request.context.request.method = "M".repeat(33);
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::InvalidMethod))
    );

    let mut request = user_request();
    request.context.route.service = " ".to_owned();
    assert_eq!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::Blank(
            "stg.route.service"
        )))
    );

    let mut request = user_request();
    request.context.route.policy_revision = Some("p".repeat(129));
    assert!(matches!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::TooLong {
            field: "stg.route.policy_revision",
            max: 128,
            actual: 129,
        }))
    ));
}

#[test]
fn enforces_utf8_and_final_compact_size_limits() {
    let source = "é".repeat(300);
    let truncated = truncate_utf8(&source, 511);
    assert_eq!(truncated.len(), 510);
    assert!(truncated.is_char_boundary(truncated.len()));

    let mut request = user_request();
    request.context.request.user_agent = Some("é".repeat(257));
    assert!(matches!(
        signer().issue(&request),
        Err(IssueError::Claims(ClaimValidationError::TooLong {
            field: "stg.request.user_agent",
            max: 512,
            actual: 514,
        }))
    ));

    let header = br#"{"alg":"RS256","kid":"stargate-internal-test","typ":"stargate-context+jwt"}"#;
    let malformed_utf8 = b"{\"iss\":\"\xFF\"}";
    assert_eq!(
        verifier(NOW).verify(&signed_parts(header, malformed_utf8), expected()),
        Err(VerificationError::InvalidPayload)
    );

    let mut request = user_request();
    request.context.request.path = format!("/{}", "p".repeat(2_047));
    request.context.request.original_path = format!("/{}", "o".repeat(2_047));
    request.context.request.user_agent = Some("u".repeat(512));
    assert_eq!(signer().issue(&request), Err(IssueError::TokenTooLarge));
}

#[test]
fn strict_payload_parser_rejects_duplicates_unknown_fields_null_and_arrays() {
    let payload = serde_json::to_string(&valid_payload()).unwrap();
    let duplicate = payload.replacen(
        '{',
        r#"{"iss":"https://auth.example.com/internal-context","#,
        1,
    );
    assert_eq!(
        verifier(NOW).verify(
            &signed_parts(
                br#"{"alg":"RS256","kid":"stargate-internal-test","typ":"stargate-context+jwt"}"#,
                duplicate.as_bytes(),
            ),
            expected(),
        ),
        Err(VerificationError::InvalidPayload)
    );

    let mut value = valid_payload();
    value["unexpected"] = serde_json::json!(true);
    assert_eq!(
        verifier(NOW).verify(&resign(&value), expected()),
        Err(VerificationError::InvalidPayload)
    );

    value = valid_payload();
    value["stg"]["request"]["trace_id"] = serde_json::Value::Null;
    assert_eq!(
        verifier(NOW).verify(&resign(&value), expected()),
        Err(VerificationError::InvalidPayload)
    );

    value = valid_payload();
    value["aud"] = serde_json::json!([AUDIENCE]);
    assert_eq!(
        verifier(NOW).verify(&resign(&value), expected()),
        Err(VerificationError::InvalidPayload)
    );

    value = valid_payload();
    value["iat"] = serde_json::json!(NOW as f64);
    assert_eq!(
        verifier(NOW).verify(&resign(&value), expected()),
        Err(VerificationError::InvalidPayload)
    );

    value = valid_payload();
    value["stg"]["dispatch"]["attempt"] = serde_json::json!(65_536);
    assert_eq!(
        verifier(NOW).verify(&resign(&value), expected()),
        Err(VerificationError::InvalidPayload)
    );
}

#[test]
fn presentation_and_configuration_fail_closed() {
    assert_eq!(
        require_single_token(std::iter::empty()),
        Err(PresentationError::Missing)
    );
    assert_eq!(
        require_single_token(["one", "two"]),
        Err(PresentationError::Duplicate)
    );
    assert_eq!(require_single_token(["one"]), Ok("one"));

    assert_eq!(
        VerifierConfig::new(ISSUER, AUDIENCE, 31),
        Err(ConfigError::InvalidClockSkew)
    );
    assert_eq!(
        ctx::SignerConfig::new(ISSUER, KEY_ID, 61),
        Err(ConfigError::InvalidLifetime)
    );
    assert_eq!(
        ctx::SignerConfig::new(ISSUER, KEY_ID, 0),
        Err(ConfigError::InvalidLifetime)
    );
    assert_eq!(
        ctx::SignerConfig::new(" ", KEY_ID, 30),
        Err(ConfigError::Blank("issuer"))
    );
    assert_eq!(
        ctx::SignerConfig::new(ISSUER, "bad id", 30),
        Err(ConfigError::InvalidKeyId)
    );
    assert_eq!(
        VerifierConfig::new(ISSUER, " ", 5),
        Err(ConfigError::Blank("audience"))
    );

    let overflow_signer = ctx::ContextSigner::with_sources(
        SignerConfig::new(ISSUER, KEY_ID, 30).unwrap(),
        PRIVATE_KEY,
        Arc::new(FixedClock(i64::MAX)),
        Arc::new(FixedId(DISPATCH_ID)),
    )
    .unwrap();
    assert_eq!(
        overflow_signer.issue(&user_request()),
        Err(IssueError::TimeOverflow)
    );
}

#[test]
fn rejects_weak_or_incompatible_jwks() {
    let weak_modulus = URL_SAFE_NO_PAD.encode([0x80; 128]);
    let weak = serde_json::json!({
        "kty": "RSA",
        "use": "sig",
        "alg": "RS256",
        "n": weak_modulus,
        "e": "AQAB"
    });
    assert_eq!(
        VerificationKey::from_jwk_json(&serde_json::to_vec(&weak).unwrap()).unwrap_err(),
        KeyMaterialError::RsaTooSmall
    );

    let incompatible = serde_json::json!({
        "kty": "RSA",
        "use": "enc",
        "alg": "RS256",
        "n": URL_SAFE_NO_PAD.encode([0x80; 256]),
        "e": "AQAB"
    });
    assert_eq!(
        VerificationKey::from_jwk_json(&serde_json::to_vec(&incompatible).unwrap()).unwrap_err(),
        KeyMaterialError::IncompatibleJwk
    );

    assert_eq!(
        StaticKeyResolver::from_jwk_json(
            "different-kid",
            include_bytes!("fixtures/public.jwk.json")
        )
        .err(),
        Some(KeyMaterialError::IncompatibleJwk)
    );
}

#[test]
fn representative_property_matrix_round_trips() {
    for mut request in [user_request(), api_key_request(), anonymous_request()] {
        for (attempt, path) in [(1, "/v1/orders"), (2, "/v1/orders/a%2Fb"), (u16::MAX, "/")] {
            request.context.dispatch.attempt = attempt;
            request.context.request.path = path.to_owned();
            let token = signer().issue(&request).unwrap().into_compact();
            let expected = ExpectedRequest {
                encoded_path: path,
                ..expected()
            };
            assert!(verifier(NOW).verify(&token, expected).is_ok());
        }
    }
}
