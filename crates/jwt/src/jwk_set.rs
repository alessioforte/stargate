//! JWTs signed with someone else's keys, published as a JWK Set: OAuth clients
//! that authenticate with `private_key_jwt` (OIDC Core 9, RFC 7523).

use jsonwebtoken::errors::{Error, ErrorKind};
use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, Jwk, JwkSet, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::de::DeserializeOwned;

/// Signature algorithms accepted from a JWK Set: asymmetric only, so a
/// public key can never be used as an HMAC secret.
pub const JWK_SET_ALGORITHMS: [Algorithm; 8] = [
    Algorithm::RS256,
    Algorithm::RS384,
    Algorithm::RS512,
    Algorithm::PS256,
    Algorithm::PS384,
    Algorithm::PS512,
    Algorithm::ES256,
    Algorithm::ES384,
];

/// JWK members that only private keys (or symmetric secrets) have.
const PRIVATE_MEMBERS: [&str; 8] = ["d", "p", "q", "dp", "dq", "qi", "oth", "k"];

/// Verify `token` with a key of `jwks` and decode its claims. The key is the
/// one named by the header's `kid`, else every signing key that fits the
/// header's algorithm is tried. `exp` is required and checked, `nbf` too when
/// present; issuer and audience are the caller's to check.
pub fn decode_with_jwk_set<T: DeserializeOwned>(token: &str, jwks: &JwkSet) -> Result<T, Error> {
    let header = decode_header(token)?;
    if !JWK_SET_ALGORITHMS.contains(&header.alg) {
        return Err(ErrorKind::InvalidAlgorithm.into());
    }
    let mut validation = Validation::new(header.alg);
    validation.validate_aud = false;
    validation.validate_nbf = true;

    for key in jwks
        .keys
        .iter()
        .filter(|key| key_fits(key, header.alg, header.kid.as_deref()))
    {
        let Ok(decoding) = DecodingKey::from_jwk(key) else {
            continue;
        };
        match decode::<T>(token, &decoding, &validation) {
            Ok(data) => return Ok(data.claims),
            // Another key may have signed it.
            Err(error) if wrong_key(&error) => continue,
            // Signed by this key, but the claims are not valid (expired, ...).
            Err(error) => return Err(error),
        }
    }
    Err(ErrorKind::InvalidSignature.into())
}

fn wrong_key(error: &Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::InvalidSignature
            | ErrorKind::InvalidKeyFormat
            | ErrorKind::InvalidRsaKey(_)
            | ErrorKind::InvalidEcdsaKey
            | ErrorKind::InvalidAlgorithm
    )
}

/// A signing key of the right type (and `kid`, `alg` when given).
fn key_fits(key: &Jwk, algorithm: Algorithm, kid: Option<&str>) -> bool {
    if kid.is_some() && key.common.key_id.as_deref() != kid {
        return false;
    }
    if matches!(key.common.public_key_use, Some(ref use_) if *use_ != PublicKeyUse::Signature) {
        return false;
    }
    if let Some(declared) = key.common.key_algorithm
        && format!("{declared:?}") != format!("{algorithm:?}")
    {
        return false;
    }
    match (&key.algorithm, algorithm) {
        (
            AlgorithmParameters::RSA(_),
            Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::PS256
            | Algorithm::PS384
            | Algorithm::PS512,
        ) => true,
        (AlgorithmParameters::EllipticCurve(params), Algorithm::ES256) => {
            params.curve == EllipticCurve::P256
        }
        (AlgorithmParameters::EllipticCurve(params), Algorithm::ES384) => {
            params.curve == EllipticCurve::P384
        }
        _ => false,
    }
}

/// Check a JWK Set a client registers: at least one key, public keys only
/// (no private or symmetric members), RSA or EC P-256/P-384 keys that load.
pub fn validate_public_jwk_set(value: &serde_json::Value) -> Result<JwkSet, String> {
    let keys = value
        .get("keys")
        .and_then(|keys| keys.as_array())
        .ok_or_else(|| "a JWK Set is an object with a `keys` array".to_string())?;
    if keys.is_empty() {
        return Err("the JWK Set has no keys".to_string());
    }
    for (index, key) in keys.iter().enumerate() {
        if let Some(member) = PRIVATE_MEMBERS
            .iter()
            .find(|member| key.get(**member).is_some())
        {
            return Err(format!(
                "key {index} has the private member `{member}`; register public keys only"
            ));
        }
    }
    let jwks: JwkSet = serde_json::from_value(value.clone())
        .map_err(|error| format!("not a valid JWK Set: {error}"))?;
    for (index, key) in jwks.keys.iter().enumerate() {
        match &key.algorithm {
            AlgorithmParameters::RSA(_) => {}
            AlgorithmParameters::EllipticCurve(params)
                if matches!(params.curve, EllipticCurve::P256 | EllipticCurve::P384) => {}
            _ => {
                return Err(format!("key {index} must be an RSA or EC P-256/P-384 key"));
            }
        }
        DecodingKey::from_jwk(key)
            .map_err(|error| format!("key {index} cannot be loaded: {error}"))?;
    }
    Ok(jwks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Claims, JwtConfig, KeyMaterial};
    use chrono::Duration;

    /// A signer (private key) and the JWK Set a client would register.
    fn signer(key_id: &str, algorithm: Algorithm) -> (JwtConfig, JwkSet) {
        let (private, public) = match algorithm {
            Algorithm::ES256 => crate::generate_p256_keys().unwrap(),
            Algorithm::ES384 => crate::generate_p384_keys().unwrap(),
            _ => crate::generate_rsa_keys(2048).unwrap(),
        };
        let key = KeyMaterial::from_pem(
            key_id,
            algorithm,
            Some(private.as_bytes()),
            public.as_bytes(),
        )
        .unwrap();
        let config =
            JwtConfig::from_keyring(key, Vec::new(), Duration::minutes(5), Duration::minutes(5))
                .unwrap();
        let jwks = config.public_jwks().unwrap();
        (config, jwks)
    }

    fn token(config: &JwtConfig) -> String {
        config
            .generate_token(&Claims::default().subject("client-1".to_string()))
            .unwrap()
    }

    #[test]
    fn verifies_with_the_registered_key() {
        for algorithm in [Algorithm::RS256, Algorithm::ES256, Algorithm::ES384] {
            let (config, jwks) = signer("client-key", algorithm);
            let claims: serde_json::Value = decode_with_jwk_set(&token(&config), &jwks).unwrap();
            assert_eq!(claims["sub"], "client-1");
        }
    }

    #[test]
    fn rejects_other_keys_and_symmetric_algorithms() {
        let (config, _) = signer("client-key", Algorithm::RS256);
        let (_, other) = signer("client-key", Algorithm::RS256);
        assert_eq!(
            decode_with_jwk_set::<serde_json::Value>(&token(&config), &other)
                .unwrap_err()
                .kind(),
            &ErrorKind::InvalidSignature
        );

        let hmac = JwtConfig::from_keyring(
            KeyMaterial::from_secret("client-key", Algorithm::HS256, b"secret").unwrap(),
            Vec::new(),
            Duration::minutes(5),
            Duration::minutes(5),
        )
        .unwrap();
        let (_, jwks) = signer("client-key", Algorithm::RS256);
        assert_eq!(
            decode_with_jwk_set::<serde_json::Value>(&token(&hmac), &jwks)
                .unwrap_err()
                .kind(),
            &ErrorKind::InvalidAlgorithm
        );
    }

    #[test]
    fn an_expired_token_fails_even_with_the_right_key() {
        let (mut config, jwks) = signer("client-key", Algorithm::RS256);
        config.access_exp = Duration::minutes(-10);
        let expired = config
            .generate_session_access_token(Claims::default().subject("client-1".to_string()))
            .unwrap();
        assert_eq!(
            decode_with_jwk_set::<serde_json::Value>(&expired, &jwks)
                .unwrap_err()
                .kind(),
            &ErrorKind::ExpiredSignature
        );
    }

    #[test]
    fn registered_sets_hold_public_keys_only() {
        let (_, jwks) = signer("client-key", Algorithm::ES256);
        let public = serde_json::to_value(&jwks).unwrap();
        assert!(validate_public_jwk_set(&public).is_ok());

        let mut private = public.clone();
        private["keys"][0]["d"] = serde_json::json!("secret");
        assert!(
            validate_public_jwk_set(&private)
                .unwrap_err()
                .contains("`d`")
        );

        let symmetric = serde_json::json!({"keys": [{"kty": "oct", "k": "c2VjcmV0"}]});
        assert!(validate_public_jwk_set(&symmetric).is_err());
        assert!(validate_public_jwk_set(&serde_json::json!({"keys": []})).is_err());
        assert!(validate_public_jwk_set(&serde_json::json!([])).is_err());
    }
}
