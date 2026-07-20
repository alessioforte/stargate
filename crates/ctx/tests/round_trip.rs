mod common;

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ctx::{
    ActorType, AuthenticationKind, DispatchKind, StaticKeyResolver, VerificationError,
    VerifierConfig,
};

use common::*;

#[test]
fn user_organization_primary_round_trip() {
    let issued = signer().issue(&user_request()).unwrap();
    let trusted = verifier(NOW).verify(issued.compact(), expected()).unwrap();

    assert_eq!(trusted.issuer(), ISSUER);
    assert_eq!(trusted.audience(), AUDIENCE);
    assert_eq!(trusted.issued_at(), NOW);
    assert_eq!(trusted.expires_at(), NOW + 30);
    assert_eq!(trusted.dispatch_id(), DISPATCH_ID);
    assert_eq!(trusted.subject(), Some(USER_ID));
    assert_eq!(trusted.context().actor.actor_type, ActorType::User);
    assert_eq!(
        trusted.context().authentication.kind,
        AuthenticationKind::Jwt
    );
    assert_eq!(
        trusted.context().organization.as_ref().unwrap().id,
        ORGANIZATION_ID
    );
    assert_eq!(trusted.context().dispatch.kind, DispatchKind::Primary);
}

#[test]
fn api_key_organizationless_shadow_round_trip() {
    let issued = signer().issue(&api_key_request()).unwrap();
    let trusted = verifier(NOW).verify(issued.compact(), expected()).unwrap();

    assert_eq!(trusted.subject(), Some(API_KEY_ID));
    assert_eq!(trusted.context().actor.actor_type, ActorType::ApiKey);
    assert_eq!(
        trusted.context().authentication.kind,
        AuthenticationKind::ApiKey
    );
    assert!(trusted.context().organization.is_none());
    assert_eq!(trusted.context().dispatch.kind, DispatchKind::Shadow);
    assert_eq!(trusted.context().dispatch.attempt, 2);
}

#[test]
fn anonymous_organizationless_round_trip() {
    let issued = signer().issue(&anonymous_request()).unwrap();
    let trusted = verifier(NOW).verify(issued.compact(), expected()).unwrap();

    assert_eq!(trusted.subject(), None);
    assert_eq!(trusted.context().actor.actor_type, ActorType::Anonymous);
    assert_eq!(
        trusted.context().authentication.kind,
        AuthenticationKind::None
    );
    assert!(trusted.context().organization.is_none());
}

#[test]
fn protected_header_is_exact_and_token_debug_is_redacted() {
    let issued = signer().issue(&user_request()).unwrap();
    let header = issued.compact().split('.').next().unwrap();
    let decoded = URL_SAFE_NO_PAD.decode(header).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&decoded).unwrap();

    assert_eq!(value["alg"], "RS256");
    assert_eq!(value["kid"], KEY_ID);
    assert_eq!(value["typ"], "stargate-context+jwt");
    assert_eq!(value.as_object().unwrap().len(), 3);
    assert_eq!(format!("{issued:?}"), "IssuedContext([redacted])");

    let trusted = verifier(NOW).verify(issued.compact(), expected()).unwrap();
    assert_eq!(format!("{trusted:?}"), "TrustedContext([redacted])");
}

#[test]
fn static_jwk_resolver_verifies_the_same_token() {
    let resolver =
        StaticKeyResolver::from_jwk_json(KEY_ID, include_bytes!("fixtures/public.jwk.json"))
            .unwrap();
    let verifier = ctx::ContextVerifier::with_clock(
        VerifierConfig::new(ISSUER, AUDIENCE, 5).unwrap(),
        Arc::new(resolver),
        Arc::new(FixedClock(NOW)),
    );
    let issued = signer().issue(&user_request()).unwrap();

    assert!(verifier.verify(issued.compact(), expected()).is_ok());
}

#[test]
fn unknown_key_never_fails_open() {
    let verifier = ctx::ContextVerifier::with_clock(
        VerifierConfig::new(ISSUER, AUDIENCE, 5).unwrap(),
        Arc::new(StaticKeyResolver::new()),
        Arc::new(FixedClock(NOW)),
    );
    let issued = signer().issue(&user_request()).unwrap();

    assert_eq!(
        verifier.verify(issued.compact(), expected()),
        Err(VerificationError::KeyResolution)
    );
}

#[test]
fn deterministic_tokens_and_decoded_payloads_match_golden_fixtures() {
    let cases = [
        (
            user_request(),
            include_str!("fixtures/user-org-primary.jwt"),
            include_str!("fixtures/user-org-primary.claims.json"),
        ),
        (
            api_key_request(),
            include_str!("fixtures/api-key-orgless-shadow.jwt"),
            include_str!("fixtures/api-key-orgless-shadow.claims.json"),
        ),
        (
            anonymous_request(),
            include_str!("fixtures/anonymous-orgless-primary.jwt"),
            include_str!("fixtures/anonymous-orgless-primary.claims.json"),
        ),
    ];

    for (request, expected_token, expected_claims) in cases {
        let token = signer().issue(&request).unwrap().into_compact();
        assert_eq!(token, expected_token.trim());

        let payload = token.split('.').nth(1).unwrap();
        let decoded = URL_SAFE_NO_PAD.decode(payload).unwrap();
        let actual: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        let expected_value: serde_json::Value = serde_json::from_str(expected_claims).unwrap();
        assert_eq!(actual, expected_value);
        assert!(verifier(NOW).verify(&token, expected()).is_ok());
    }
}
