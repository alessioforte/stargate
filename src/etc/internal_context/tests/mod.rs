use super::settings::test_support::settings;
use super::test_support::{
    PRIVATE_KEY, PUBLIC_JWK, ROTATION_NEXT_PRIVATE_KEY, anonymous_issue_request,
};
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ctx::VerificationKey;

fn jwks(jwk: Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({ "keys": [jwk] })).unwrap()
}

fn public_jwk(private_key_pem: &[u8], key_id: &str) -> Value {
    let private_key = parse_private_key(private_key_pem).unwrap();
    serde_json::json!({
        "kty": "RSA",
        "use": "sig",
        "key_ops": ["verify"],
        "alg": "RS256",
        "kid": key_id,
        "n": URL_SAFE_NO_PAD.encode(private_key.n().to_bytes_be()),
        "e": URL_SAFE_NO_PAD.encode(private_key.e().to_bytes_be()),
    })
}

fn verifier(jwks: &[Value]) -> ctx::ContextVerifier {
    let mut resolver = ctx::StaticKeyResolver::new();
    for jwk in jwks {
        let key_id = jwk["kid"].as_str().unwrap();
        let key = VerificationKey::from_jwk_json(&serde_json::to_vec(jwk).unwrap()).unwrap();
        resolver.insert(key_id, key);
    }
    ctx::ContextVerifier::new(
        ctx::VerifierConfig::new(
            "https://stargate.test/internal-context",
            "urn:stargate:service:orders",
            DEFAULT_CLOCK_SKEW_SECS,
        )
        .unwrap(),
        Arc::new(resolver),
    )
}

fn config(yaml: &str) -> RuntimeConfig {
    RuntimeConfig::from_yaml_str(yaml).expect("config should compile")
}

mod material;
mod preflight;
