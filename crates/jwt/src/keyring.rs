//! Key material for Stargate-issued JWTs: one active signing key plus any
//! number of verification-only keys (the next key before activation, retired
//! keys until their tokens expire). Every asymmetric key is published in the
//! JWKS and selected for verification by the token's `kid`.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use std::{error::Error, fmt};

/// Longest accepted key id; also used as a directory name by the app.
pub const MAX_KEY_ID_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyError(String);

impl KeyError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for KeyError {}

/// One signing or verification key.
pub struct KeyMaterial {
    pub(crate) key_id: Option<String>,
    pub(crate) algorithm: Algorithm,
    pub(crate) encoding: Option<EncodingKey>,
    pub(crate) decoding: DecodingKey,
    /// Public JWK; `None` for HMAC secrets, which are never published.
    pub(crate) jwk: Option<Jwk>,
}

impl KeyMaterial {
    /// An asymmetric key from PEM text. Without `private_pem` the key only
    /// verifies; with it, the pair is checked to match.
    pub fn from_pem(
        key_id: &str,
        algorithm: Algorithm,
        private_pem: Option<&[u8]>,
        public_pem: &[u8],
    ) -> Result<Self, KeyError> {
        validate_key_id(key_id)?;
        Self::from_pem_parts(Some(key_id), algorithm, private_pem, public_pem)
    }

    /// Like [`Self::from_pem`], also allowing a key without id (single-key
    /// configurations built from [`crate::KeySource`]).
    pub(crate) fn from_pem_parts(
        key_id: Option<&str>,
        algorithm: Algorithm,
        private_pem: Option<&[u8]>,
        public_pem: &[u8],
    ) -> Result<Self, KeyError> {
        let label = key_id.unwrap_or("default");
        let public = PublicKey::parse(public_pem)?;
        if !public.supports(algorithm) {
            return Err(KeyError::new(format!(
                "key '{label}' is a {} key and cannot use {algorithm:?}",
                public.describe()
            )));
        }

        let decoding = match public {
            PublicKey::Rsa(_) => DecodingKey::from_rsa_pem(public_pem),
            PublicKey::P256(_) | PublicKey::P384(_) => DecodingKey::from_ec_pem(public_pem),
        }
        .map_err(|error| KeyError::new(format!("key '{label}' public key: {error}")))?;

        let encoding = private_pem
            .map(|pem| match public {
                PublicKey::Rsa(_) => EncodingKey::from_rsa_pem(pem),
                PublicKey::P256(_) | PublicKey::P384(_) => EncodingKey::from_ec_pem(pem),
            })
            .transpose()
            .map_err(|error| KeyError::new(format!("key '{label}' private key: {error}")))?;
        if let Some(encoding) = &encoding {
            ensure_pair(label, algorithm, encoding, &decoding)?;
        }

        let jwk = public.jwk(key_id, algorithm)?;
        Ok(Self {
            key_id: key_id.map(str::to_string),
            algorithm,
            encoding,
            decoding,
            jwk: Some(jwk),
        })
    }

    /// An HMAC secret (HS256/384/512). Signs and verifies; never published.
    pub fn from_secret(
        key_id: Option<String>,
        algorithm: Algorithm,
        secret: &[u8],
    ) -> Result<Self, KeyError> {
        if !matches!(
            algorithm,
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512
        ) {
            return Err(KeyError::new(format!(
                "{algorithm:?} is not an HMAC algorithm"
            )));
        }
        Ok(Self {
            key_id,
            algorithm,
            encoding: Some(EncodingKey::from_secret(secret)),
            decoding: DecodingKey::from_secret(secret),
            jwk: None,
        })
    }

    pub fn key_id(&self) -> Option<&str> {
        self.key_id.as_deref()
    }

    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    pub fn can_sign(&self) -> bool {
        self.encoding.is_some()
    }
}

/// The algorithm a public key is used with when nothing else says so:
/// RS256 for RSA, ES256 for P-256, ES384 for P-384.
pub fn infer_algorithm(public_pem: &[u8]) -> Result<Algorithm, KeyError> {
    Ok(match PublicKey::parse(public_pem)? {
        PublicKey::Rsa(_) => Algorithm::RS256,
        PublicKey::P256(_) => Algorithm::ES256,
        PublicKey::P384(_) => Algorithm::ES384,
    })
}

/// Key ids are header values and directory names: printable ASCII without
/// path separators.
pub fn validate_key_id(key_id: &str) -> Result<(), KeyError> {
    let valid = !key_id.is_empty()
        && key_id.len() <= MAX_KEY_ID_BYTES
        && key_id != "."
        && key_id != ".."
        && key_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(KeyError::new(format!(
            "key id '{key_id}' must be 1-{MAX_KEY_ID_BYTES} characters of A-Z, a-z, 0-9, '-', '_', '.'"
        )))
    }
}

enum PublicKey {
    Rsa(rsa::RsaPublicKey),
    P256(p256::PublicKey),
    P384(p384::PublicKey),
}

impl PublicKey {
    fn parse(pem: &[u8]) -> Result<Self, KeyError> {
        let text =
            std::str::from_utf8(pem).map_err(|_| KeyError::new("public key is not PEM text"))?;
        use p256::pkcs8::DecodePublicKey as _;
        use rsa::pkcs8::DecodePublicKey as _;
        if let Ok(key) = rsa::RsaPublicKey::from_public_key_pem(text) {
            return Ok(Self::Rsa(key));
        }
        if let Ok(key) = p256::PublicKey::from_public_key_pem(text) {
            return Ok(Self::P256(key));
        }
        if let Ok(key) = p384::PublicKey::from_public_key_pem(text) {
            return Ok(Self::P384(key));
        }
        Err(KeyError::new(
            "public key must be an RSA, P-256, or P-384 SubjectPublicKeyInfo PEM",
        ))
    }

    fn supports(&self, algorithm: Algorithm) -> bool {
        match self {
            Self::Rsa(_) => matches!(
                algorithm,
                Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512
            ),
            Self::P256(_) => algorithm == Algorithm::ES256,
            Self::P384(_) => algorithm == Algorithm::ES384,
        }
    }

    fn describe(&self) -> &'static str {
        match self {
            Self::Rsa(_) => "RSA",
            Self::P256(_) => "P-256",
            Self::P384(_) => "P-384",
        }
    }

    fn jwk(&self, key_id: Option<&str>, algorithm: Algorithm) -> Result<Jwk, KeyError> {
        use p256::elliptic_curve::sec1::ToSec1Point as _;
        use rsa::traits::PublicKeyParts as _;

        let alg = format!("{algorithm:?}");
        let value = match self {
            Self::Rsa(key) => serde_json::json!({
                "kty": "RSA",
                "use": "sig",
                "alg": alg,
                "kid": key_id,
                "n": URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),
                "e": URL_SAFE_NO_PAD.encode(key.e().to_bytes_be()),
            }),
            Self::P256(key) => ec_jwk(key_id, &alg, "P-256", key.to_sec1_point(false).as_bytes()),
            Self::P384(key) => ec_jwk(key_id, &alg, "P-384", key.to_sec1_point(false).as_bytes()),
        };
        serde_json::from_value(value).map_err(|error| {
            KeyError::new(format!(
                "key '{}' JWK: {error}",
                key_id.unwrap_or("default")
            ))
        })
    }
}

/// JWK of an uncompressed SEC1 point (`0x04 || X || Y`).
fn ec_jwk(key_id: Option<&str>, alg: &str, curve: &str, point: &[u8]) -> serde_json::Value {
    let coordinates = &point[1..];
    let (x, y) = coordinates.split_at(coordinates.len() / 2);
    serde_json::json!({
        "kty": "EC",
        "use": "sig",
        "alg": alg,
        "kid": key_id,
        "crv": curve,
        "x": URL_SAFE_NO_PAD.encode(x),
        "y": URL_SAFE_NO_PAD.encode(y),
    })
}

/// Sign and verify a probe so mismatched private/public files fail at load.
fn ensure_pair(
    key_id: &str,
    algorithm: Algorithm,
    encoding: &EncodingKey,
    decoding: &DecodingKey,
) -> Result<(), KeyError> {
    let probe = serde_json::json!({ "probe": key_id });
    let token = encode(&Header::new(algorithm), &probe, encoding)
        .map_err(|error| KeyError::new(format!("key '{key_id}' cannot sign: {error}")))?;
    let mut validation = Validation::new(algorithm);
    validation.required_spec_claims.clear();
    validation.validate_exp = false;
    validation.validate_aud = false;
    decode::<serde_json::Value>(&token, decoding, &validation)
        .map(|_| ())
        .map_err(|_| {
            KeyError::new(format!(
                "key '{key_id}' private and public keys do not match"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Claims, JwtConfig};
    use chrono::Duration;

    fn rsa_pair() -> (String, String) {
        crate::generate_rsa_keys(2048).unwrap()
    }

    fn signing(key_id: &str, algorithm: Algorithm, pair: &(String, String)) -> KeyMaterial {
        KeyMaterial::from_pem(
            key_id,
            algorithm,
            Some(pair.0.as_bytes()),
            pair.1.as_bytes(),
        )
        .unwrap()
    }

    fn verifying(key_id: &str, algorithm: Algorithm, pair: &(String, String)) -> KeyMaterial {
        KeyMaterial::from_pem(key_id, algorithm, None, pair.1.as_bytes()).unwrap()
    }

    fn config(active: KeyMaterial, additional: Vec<KeyMaterial>) -> JwtConfig {
        JwtConfig::from_keyring(
            active,
            additional,
            Duration::minutes(5),
            Duration::minutes(5),
        )
        .unwrap()
    }

    fn token(config: &JwtConfig) -> String {
        config
            .generate_session_access_token(
                Claims::default()
                    .subject("user-1".to_string())
                    .sid("sid-1".to_string()),
            )
            .unwrap()
    }

    #[test]
    fn rotation_keeps_old_tokens_valid_until_the_old_key_is_removed() {
        let old = rsa_pair();
        let new = crate::generate_p256_keys().unwrap();

        let before = config(signing("k1", Algorithm::RS256, &old), vec![]);
        let old_token = token(&before);

        // Next key published first, then activated with the old one kept.
        let prepublished = config(
            signing("k1", Algorithm::RS256, &old),
            vec![verifying("k2", Algorithm::ES256, &new)],
        );
        let rotated = config(
            signing("k2", Algorithm::ES256, &new),
            vec![verifying("k1", Algorithm::RS256, &old)],
        );
        let new_token = token(&rotated);

        assert!(
            prepublished
                .validate_session_access_token(&old_token)
                .is_ok()
        );
        assert!(
            prepublished
                .validate_session_access_token(&new_token)
                .is_ok()
        );
        assert!(rotated.validate_session_access_token(&old_token).is_ok());
        assert!(before.validate_session_access_token(&new_token).is_err());

        let retired = config(signing("k2", Algorithm::ES256, &new), vec![]);
        assert!(retired.validate_session_access_token(&old_token).is_err());
        assert!(retired.validate_session_access_token(&new_token).is_ok());
        assert_eq!(rotated.key_id(), Some("k2"));
        assert_eq!(rotated.key_ids(), vec!["k2", "k1"]);
    }

    #[test]
    fn jwks_publishes_every_key_active_first() {
        let old = rsa_pair();
        let new = crate::generate_p384_keys().unwrap();
        let config = config(
            signing("k2", Algorithm::ES384, &new),
            vec![verifying("k1", Algorithm::RS256, &old)],
        );

        let jwks = serde_json::to_value(config.public_jwks().unwrap()).unwrap();
        let keys = jwks["keys"].as_array().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0]["kid"], "k2");
        assert_eq!(keys[0]["alg"], "ES384");
        assert_eq!(keys[0]["crv"], "P-384");
        assert_eq!(keys[1]["kid"], "k1");
        assert_eq!(keys[1]["alg"], "RS256");
        assert_eq!(keys[1]["use"], "sig");
        for key in keys {
            assert!(key.get("d").is_none(), "no private members");
        }
    }

    #[test]
    fn public_only_jwk_matches_the_private_key_jwk() {
        for (algorithm, pair) in [
            (Algorithm::RS256, rsa_pair()),
            (Algorithm::ES256, crate::generate_p256_keys().unwrap()),
            (Algorithm::ES384, crate::generate_p384_keys().unwrap()),
        ] {
            let ours = serde_json::to_value(verifying("k", algorithm, &pair).jwk.unwrap()).unwrap();
            let encoding = match algorithm {
                Algorithm::RS256 => EncodingKey::from_rsa_pem(pair.0.as_bytes()),
                _ => EncodingKey::from_ec_pem(pair.0.as_bytes()),
            }
            .unwrap();
            let reference =
                serde_json::to_value(Jwk::from_encoding_key(&encoding, algorithm).unwrap())
                    .unwrap();
            for member in ["n", "e", "x", "y", "crv", "kty"] {
                assert_eq!(
                    ours.get(member),
                    reference.get(member),
                    "{algorithm:?} {member}"
                );
            }
        }
    }

    #[test]
    fn unknown_kid_and_algorithm_confusion_are_rejected() {
        let pair = rsa_pair();
        let config = config(signing("k1", Algorithm::RS256, &pair), vec![]);

        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("other".to_string());
        let rsa = EncodingKey::from_rsa_pem(pair.0.as_bytes()).unwrap();
        let unknown = encode(&header, &Claims::default(), &rsa).unwrap();
        assert!(config.validate_token(&unknown).is_err());

        // HS256 keyed with the public PEM, claiming the RSA key's kid.
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some("k1".to_string());
        let forged = encode(
            &header,
            &Claims::default(),
            &EncodingKey::from_secret(pair.1.as_bytes()),
        )
        .unwrap();
        assert!(config.validate_token(&forged).is_err());
    }

    #[test]
    fn invalid_key_material_is_rejected() {
        let first = rsa_pair();
        let second = rsa_pair();
        let mismatched = KeyMaterial::from_pem(
            "k1",
            Algorithm::RS256,
            Some(first.0.as_bytes()),
            second.1.as_bytes(),
        );
        assert!(mismatched.is_err());

        let wrong_algorithm =
            KeyMaterial::from_pem("k1", Algorithm::ES256, None, first.1.as_bytes());
        assert!(wrong_algorithm.is_err());

        assert!(validate_key_id("../escape").is_err());
        assert!(validate_key_id("").is_err());
        assert!(validate_key_id("stargate-2026.10").is_ok());

        let public_only = verifying("k1", Algorithm::RS256, &first);
        assert!(
            JwtConfig::from_keyring(
                public_only,
                vec![],
                Duration::minutes(1),
                Duration::minutes(1)
            )
            .is_err()
        );

        let duplicate = JwtConfig::from_keyring(
            signing("k1", Algorithm::RS256, &first),
            vec![verifying("k1", Algorithm::RS256, &second)],
            Duration::minutes(1),
            Duration::minutes(1),
        );
        assert!(duplicate.is_err());
    }

    #[test]
    fn algorithm_is_inferred_from_the_public_key() {
        assert_eq!(
            infer_algorithm(rsa_pair().1.as_bytes()).unwrap(),
            Algorithm::RS256
        );
        assert_eq!(
            infer_algorithm(crate::generate_p256_keys().unwrap().1.as_bytes()).unwrap(),
            Algorithm::ES256
        );
        assert_eq!(
            infer_algorithm(crate::generate_p384_keys().unwrap().1.as_bytes()).unwrap(),
            Algorithm::ES384
        );
        assert!(infer_algorithm(b"not a key").is_err());
    }

    fn id_token(config: &JwtConfig) -> String {
        config
            .generate_oidc_id_token(
                Claims::default()
                    .subject("user-1".to_string())
                    .aud("client-1".to_string()),
            )
            .unwrap()
    }

    #[test]
    fn id_tokens_can_use_a_dedicated_rs256_key() {
        let ec = crate::generate_p256_keys().unwrap();
        let rsa = rsa_pair();
        let config = config(
            signing("ec-1", Algorithm::ES256, &ec),
            vec![signing("rsa-1", Algorithm::RS256, &rsa)],
        )
        .with_id_token_key("rsa-1")
        .unwrap();

        let id_header = jsonwebtoken::decode_header(id_token(&config)).unwrap();
        let access_header = jsonwebtoken::decode_header(token(&config)).unwrap();
        assert_eq!(id_header.alg, Algorithm::RS256);
        assert_eq!(id_header.kid.as_deref(), Some("rsa-1"));
        assert_eq!(access_header.alg, Algorithm::ES256);
        assert_eq!(access_header.kid.as_deref(), Some("ec-1"));

        assert_eq!(config.id_token_algorithm(), Algorithm::RS256);
        assert_eq!(config.id_token_key_id(), Some("rsa-1"));
        assert_eq!(config.algorithm(), Algorithm::ES256);
        assert!(
            config
                .validate_oidc_id_token(&id_token(&config), "client-1", None)
                .is_ok()
        );
        assert!(
            config
                .validate_session_access_token(&token(&config))
                .is_ok()
        );
        // The ID token is still not usable as an access token.
        assert!(
            config
                .validate_session_access_token(&id_token(&config))
                .is_err()
        );
    }

    #[test]
    fn hmac_sessions_can_issue_verifiable_id_tokens() {
        let rsa = rsa_pair();
        let config = config(
            KeyMaterial::from_secret(Some("hmac".to_string()), Algorithm::HS256, b"secret")
                .unwrap(),
            vec![signing("rsa-1", Algorithm::RS256, &rsa)],
        )
        .with_id_token_key("rsa-1")
        .unwrap();

        let id_header = jsonwebtoken::decode_header(id_token(&config)).unwrap();
        assert_eq!(id_header.alg, Algorithm::RS256);
        // Only the RSA key is published; the HMAC secret never is.
        let jwks = config.public_jwks().unwrap();
        assert_eq!(jwks.keys.len(), 1);
        assert_eq!(jwks.keys[0].common.key_id.as_deref(), Some("rsa-1"));
    }

    #[test]
    fn id_token_key_must_exist_and_be_able_to_sign() {
        let rsa = rsa_pair();
        let other = rsa_pair();
        let base = || {
            config(
                signing("k1", Algorithm::RS256, &rsa),
                vec![verifying("k2", Algorithm::RS256, &other)],
            )
        };

        assert!(base().with_id_token_key("missing").is_err());
        assert!(base().with_id_token_key("k2").is_err(), "public-only key");
        let same = base().with_id_token_key("k1").unwrap();
        assert_eq!(same.id_token_key_id(), Some("k1"));
        // Default: the active key.
        assert_eq!(base().id_token_key_id(), Some("k1"));
    }
}
