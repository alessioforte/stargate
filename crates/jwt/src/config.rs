use crate::claims::Claims;
use crate::keyring::{KeyError, KeyMaterial};
use chrono::Duration;
use jsonwebtoken::Algorithm;

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Header, Validation, decode, decode_header, encode};
use std::fs;
use std::{error::Error, fmt};

const TOKEN_TYPE_BEARER: &str = "bearer";
const TOKEN_TYPE_REFRESH: &str = "refresh";
const TOKEN_TYPE_ID_TOKEN: &str = "id_token";

/// Represents the key material needed for a given JWT algorithm.
pub enum KeySource {
    /// HMAC-based algorithms (HS256, HS384, HS512)
    Secret(String),
    /// RSA-based algorithms (RS256, RS384, RS512) — paths to PEM files
    Rsa {
        private_key_path: String,
        public_key_path: String,
    },
    /// EC-based algorithms (ES256, ES384) — paths to PEM files
    Ec {
        private_key_path: String,
        public_key_path: String,
    },
}

/// JWT Configuration struct to hold preloaded keys and algorithm
pub struct JwtConfig {
    /// The active signing key first, then verification-only keys. Tokens
    /// are verified with the key named by their `kid` header.
    keys: Vec<KeyMaterial>,
    /// Index in `keys` of the key that signs OIDC ID tokens: the active key
    /// unless another one is designated (see [`Self::with_id_token_key`]).
    id_token_key: usize,
    /// Expected `iss` claim, enforced on every validation. Captured once at
    /// construction from the same source (`issuer_from_env`) that token
    /// generation uses, so self-issued tokens validate and tokens minted for
    /// a different issuer are rejected.
    issuer: String,
    pub access_exp: Duration,
    pub refresh_exp: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JwtValidationError {
    Decode(String),
    InvalidType {
        expected: &'static str,
        actual: Option<String>,
    },
    MissingClaim(&'static str),
    UnexpectedClaim(&'static str),
    AudienceMismatch {
        expected: String,
        actual: Option<String>,
    },
    NonceMismatch {
        expected: String,
        actual: Option<String>,
    },
}

impl fmt::Display for JwtValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JwtValidationError::Decode(error) => write!(f, "JWT decode failed: {error}"),
            JwtValidationError::InvalidType { expected, actual } => {
                write!(f, "invalid JWT type: expected {expected}, got {actual:?}")
            }
            JwtValidationError::MissingClaim(claim) => write!(f, "missing JWT claim: {claim}"),
            JwtValidationError::UnexpectedClaim(claim) => {
                write!(f, "unexpected JWT claim: {claim}")
            }
            JwtValidationError::AudienceMismatch { expected, actual } => {
                write!(
                    f,
                    "invalid JWT audience: expected {expected}, got {actual:?}"
                )
            }
            JwtValidationError::NonceMismatch { expected, actual } => {
                write!(f, "invalid JWT nonce: expected {expected}, got {actual:?}")
            }
        }
    }
}

impl Error for JwtValidationError {}

impl From<jsonwebtoken::errors::Error> for JwtValidationError {
    fn from(error: jsonwebtoken::errors::Error) -> Self {
        JwtValidationError::Decode(error.to_string())
    }
}

impl JwtConfig {
    pub fn new(
        algorithm: Algorithm,
        key_source: KeySource,
        access_exp: Duration,
        refresh_exp: Duration,
    ) -> Self {
        Self::new_with_key_id(algorithm, key_source, access_exp, refresh_exp, None)
    }

    pub fn new_with_key_id(
        algorithm: Algorithm,
        key_source: KeySource,
        access_exp: Duration,
        refresh_exp: Duration,
        key_id: Option<String>,
    ) -> Self {
        let key = match key_source {
            KeySource::Secret(secret) => {
                KeyMaterial::from_secret(key_id, algorithm, secret.as_bytes())
                    .expect("Invalid JWT secret configuration")
            }
            KeySource::Rsa {
                private_key_path,
                public_key_path,
            }
            | KeySource::Ec {
                private_key_path,
                public_key_path,
            } => {
                let private_pem =
                    fs::read(&private_key_path).expect("Failed to read JWT private key");
                let public_pem = fs::read(&public_key_path).expect("Failed to read JWT public key");
                KeyMaterial::from_pem_parts(
                    key_id.as_deref(),
                    algorithm,
                    Some(&private_pem),
                    &public_pem,
                )
                .expect("Invalid JWT signing key")
            }
        };

        Self::from_keyring(key, Vec::new(), access_exp, refresh_exp)
            .expect("Invalid JWT signing key")
    }

    /// A configuration that signs with `active` and also accepts tokens
    /// signed by `additional` keys (published in the JWKS). Key ids must be
    /// unique; additional keys must have one.
    pub fn from_keyring(
        active: KeyMaterial,
        additional: Vec<KeyMaterial>,
        access_exp: Duration,
        refresh_exp: Duration,
    ) -> Result<Self, KeyError> {
        if !active.can_sign() {
            return Err(KeyError::new("the active JWT key needs its private key"));
        }
        let mut seen = std::collections::HashSet::new();
        for key in std::iter::once(&active).chain(&additional) {
            match key.key_id() {
                Some(key_id) if !seen.insert(key_id.to_string()) => {
                    return Err(KeyError::new(format!("duplicate JWT key id '{key_id}'")));
                }
                None if !additional.is_empty() => {
                    return Err(KeyError::new(
                        "every JWT key needs a key id when several keys are configured",
                    ));
                }
                _ => {}
            }
        }

        let mut keys = Vec::with_capacity(1 + additional.len());
        keys.push(active);
        keys.extend(additional);
        Ok(Self {
            keys,
            id_token_key: 0,
            issuer: crate::claims::issuer_from_env(),
            access_exp,
            refresh_exp,
        })
    }

    /// Sign OIDC ID tokens with the key `key_id` instead of the active key,
    /// e.g. an RS256 key (required by OpenID Connect) while access and
    /// session tokens use an EC key. The key must be loaded with its private
    /// key.
    pub fn with_id_token_key(mut self, key_id: &str) -> Result<Self, KeyError> {
        let index = self
            .keys
            .iter()
            .position(|key| key.key_id() == Some(key_id))
            .ok_or_else(|| KeyError::new(format!("unknown ID token key '{key_id}'")))?;
        if !self.keys[index].can_sign() {
            return Err(KeyError::new(format!(
                "ID token key '{key_id}' needs its private key"
            )));
        }
        self.id_token_key = index;
        Ok(self)
    }

    fn active(&self) -> &KeyMaterial {
        &self.keys[0]
    }

    fn id_token_signer(&self) -> &KeyMaterial {
        &self.keys[self.id_token_key]
    }

    /// Generate a JWT token
    pub fn generate_token(&self, claims: &Claims) -> Result<String, jsonwebtoken::errors::Error> {
        Self::sign(self.active(), claims)
    }

    fn sign(key: &KeyMaterial, claims: &Claims) -> Result<String, jsonwebtoken::errors::Error> {
        let mut header = Header::new(key.algorithm);
        header.kid = key.key_id.clone();
        let encoding = key
            .encoding
            .as_ref()
            .expect("signing keys are checked to have a private key");
        encode(&header, claims, encoding)
    }

    /// The key a token claims to be signed with. A token without `kid` is
    /// checked against the active key; an unknown `kid` is rejected.
    fn verification_key(&self, token: &str) -> Result<&KeyMaterial, jsonwebtoken::errors::Error> {
        let header = decode_header(token)?;
        let Some(key_id) = header.kid.as_deref() else {
            return Ok(self.active());
        };
        self.keys
            .iter()
            .find(|key| key.key_id() == Some(key_id))
            .or_else(|| {
                // A single unnamed key accepts any kid (legacy configurations).
                (self.keys.len() == 1 && self.active().key_id.is_none()).then(|| self.active())
            })
            .ok_or_else(|| jsonwebtoken::errors::ErrorKind::InvalidSignature.into())
    }

    fn decode_claims(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        let key = self.verification_key(token)?;
        // The algorithm comes from the key, never from the token header.
        let mut validation = Validation::new(key.algorithm);
        validation.validate_aud = false;
        validation.set_issuer(&[self.issuer.as_str()]);
        Ok(decode::<Claims>(token, &key.decoding, &validation)?.claims)
    }

    fn timed_claims(&self, mut claims: Claims, token_type: &str, ttl: Duration) -> Claims {
        let now = chrono::Utc::now();
        claims.typ = Some(token_type.to_string());
        claims.iat = now.timestamp() as usize;
        claims.exp = (now + ttl).timestamp() as usize;
        if claims.jti.is_none() {
            claims.jti = Some(ulid::Ulid::generate().to_string());
        }
        claims
    }

    pub fn generate_session_access_token(
        &self,
        claims: Claims,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let claims = self.timed_claims(claims, TOKEN_TYPE_BEARER, self.access_exp);
        self.generate_token(&claims)
    }

    pub fn generate_session_refresh_token(
        &self,
        claims: Claims,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let claims = self.timed_claims(claims, TOKEN_TYPE_REFRESH, self.refresh_exp);
        self.generate_token(&claims)
    }

    pub fn generate_oauth_access_token(
        &self,
        claims: Claims,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let claims = self.timed_claims(claims, TOKEN_TYPE_BEARER, self.access_exp);
        self.generate_token(&claims)
    }

    pub fn generate_oidc_id_token(
        &self,
        claims: Claims,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        // ID tokens go to third-party relying parties, so they carry no
        // private `typ` claim. Typed validators still reject them because they
        // require `bearer` or `refresh`.
        let mut claims = self.timed_claims(claims, TOKEN_TYPE_ID_TOKEN, self.access_exp);
        claims.typ = None;
        Self::sign(self.id_token_signer(), &claims)
    }

    fn decode_without_audience(&self, token: &str) -> Result<Claims, JwtValidationError> {
        Ok(self.decode_claims(token)?)
    }

    /// Validate a JWT token without enforcing caller intent.
    ///
    /// Prefer the intent-specific helpers for application handlers.
    pub fn validate_token(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        self.decode_claims(token)
    }

    fn require_type(claims: &Claims, expected: &'static str) -> Result<(), JwtValidationError> {
        if claims.typ.as_deref() == Some(expected) {
            return Ok(());
        }

        Err(JwtValidationError::InvalidType {
            expected,
            actual: claims.typ.clone(),
        })
    }

    fn require_non_empty_claim(
        value: Option<&str>,
        claim: &'static str,
    ) -> Result<(), JwtValidationError> {
        if value.is_some_and(|value| !value.trim().is_empty()) {
            return Ok(());
        }
        Err(JwtValidationError::MissingClaim(claim))
    }

    pub fn validate_session_access_token(&self, token: &str) -> Result<Claims, JwtValidationError> {
        let claims = self.decode_without_audience(token)?;
        Self::require_type(&claims, TOKEN_TYPE_BEARER)?;
        Self::require_non_empty_claim(claims.sid.as_deref(), "sid")?;
        if claims.azp.is_some() {
            return Err(JwtValidationError::UnexpectedClaim("azp"));
        }
        Ok(claims)
    }

    pub fn validate_session_refresh_token(
        &self,
        token: &str,
    ) -> Result<Claims, JwtValidationError> {
        let claims = self.decode_without_audience(token)?;
        Self::require_type(&claims, TOKEN_TYPE_REFRESH)?;
        Self::require_non_empty_claim(claims.sid.as_deref(), "sid")?;
        if claims.azp.is_some() {
            return Err(JwtValidationError::UnexpectedClaim("azp"));
        }
        Ok(claims)
    }

    pub fn validate_oauth_access_token(
        &self,
        token: &str,
        audience: Option<&str>,
    ) -> Result<Claims, JwtValidationError> {
        let claims = self.decode_without_audience(token)?;
        Self::require_type(&claims, TOKEN_TYPE_BEARER)?;
        Self::require_non_empty_claim(claims.azp.as_deref(), "azp")?;
        if let Some(expected) = audience {
            match claims.aud.as_deref() {
                Some(actual) if actual == expected => {}
                actual => {
                    return Err(JwtValidationError::AudienceMismatch {
                        expected: expected.to_string(),
                        actual: actual.map(str::to_string),
                    });
                }
            }
        }
        Ok(claims)
    }

    pub fn validate_oidc_id_token(
        &self,
        token: &str,
        client_id: &str,
        nonce: Option<&str>,
    ) -> Result<Claims, JwtValidationError> {
        let claims = self.decode_without_audience(token)?;
        if claims.typ.is_some() {
            return Err(JwtValidationError::InvalidType {
                expected: TOKEN_TYPE_ID_TOKEN,
                actual: claims.typ.clone(),
            });
        }
        match claims.aud.as_deref() {
            Some(actual) if actual == client_id => {}
            actual => {
                return Err(JwtValidationError::AudienceMismatch {
                    expected: client_id.to_string(),
                    actual: actual.map(str::to_string),
                });
            }
        }
        if let Some(expected) = nonce {
            match claims.nonce.as_deref() {
                Some(actual) if actual == expected => {}
                actual => {
                    return Err(JwtValidationError::NonceMismatch {
                        expected: expected.to_string(),
                        actual: actual.map(str::to_string),
                    });
                }
            }
        }
        Ok(claims)
    }

    /// Algorithm of the active signing key.
    pub fn algorithm(&self) -> Algorithm {
        self.active().algorithm
    }

    /// Key id of the active signing key.
    pub fn key_id(&self) -> Option<&str> {
        self.active().key_id()
    }

    /// Algorithm OIDC ID tokens are signed with (for discovery metadata).
    pub fn id_token_algorithm(&self) -> Algorithm {
        self.id_token_signer().algorithm
    }

    /// Key id OIDC ID tokens are signed with.
    pub fn id_token_key_id(&self) -> Option<&str> {
        self.id_token_signer().key_id()
    }

    /// Ids of all keys tokens are accepted from, active first.
    pub fn key_ids(&self) -> Vec<&str> {
        self.keys.iter().filter_map(KeyMaterial::key_id).collect()
    }

    /// Public JWKs of every asymmetric key, active first. HMAC secrets are
    /// never published.
    pub fn public_jwks(&self) -> Result<JwkSet, jsonwebtoken::errors::Error> {
        Ok(JwkSet {
            keys: self.keys.iter().filter_map(|key| key.jwk.clone()).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::decode_header;

    #[test]
    fn generated_token_includes_key_id() {
        let config = JwtConfig::new_with_key_id(
            Algorithm::HS256,
            KeySource::Secret("secret".to_string()),
            Duration::minutes(5),
            Duration::minutes(5),
            Some("stargate-test".to_string()),
        );

        let claims = Claims::default().subject("user-1".to_string());
        let token = config.generate_token(&claims).unwrap();
        let header = decode_header(&token).unwrap();

        assert_eq!(header.kid.as_deref(), Some("stargate-test"));
    }

    #[test]
    fn hmac_public_jwks_does_not_expose_secret() {
        let config = JwtConfig::new_with_key_id(
            Algorithm::HS256,
            KeySource::Secret("secret".to_string()),
            Duration::minutes(5),
            Duration::minutes(5),
            Some("stargate-test".to_string()),
        );

        let jwks = config.public_jwks().unwrap();

        assert!(jwks.keys.is_empty());
    }

    #[test]
    fn validates_token_with_audience_without_resource_audience_policy() {
        let config = JwtConfig::new_with_key_id(
            Algorithm::HS256,
            KeySource::Secret("secret".to_string()),
            Duration::minutes(5),
            Duration::minutes(5),
            Some("stargate-test".to_string()),
        );

        let claims = Claims::default()
            .subject("client-1".to_string())
            .aud("gateway".to_string());
        let token = config.generate_token(&claims).unwrap();
        let decoded = config.validate_token(&token).unwrap();

        assert_eq!(decoded.aud.as_deref(), Some("gateway"));
    }

    #[test]
    fn rejects_token_with_wrong_issuer() {
        let config = JwtConfig::new_with_key_id(
            Algorithm::HS256,
            KeySource::Secret("secret".to_string()),
            Duration::minutes(5),
            Duration::minutes(5),
            Some("stargate-test".to_string()),
        );

        let claims = Claims::default()
            .subject("client-1".to_string())
            .iss("https://attacker.example".to_string());
        let token = config.generate_token(&claims).unwrap();

        assert!(config.validate_token(&token).is_err());
    }

    #[test]
    fn session_access_validation_requires_bearer_with_sid() {
        let config = JwtConfig::new(
            Algorithm::HS256,
            KeySource::Secret("secret".to_string()),
            Duration::minutes(5),
            Duration::minutes(5),
        );
        let claims = Claims::default()
            .subject("alice@example.com".to_string())
            .sid("sid-1".to_string());
        let token = config.generate_session_access_token(claims).unwrap();

        let decoded = config.validate_session_access_token(&token).unwrap();

        assert_eq!(decoded.typ.as_deref(), Some("bearer"));
        assert_eq!(decoded.sid.as_deref(), Some("sid-1"));
    }

    #[test]
    fn session_access_validation_rejects_oauth_tokens() {
        let config = JwtConfig::new(
            Algorithm::HS256,
            KeySource::Secret("secret".to_string()),
            Duration::minutes(5),
            Duration::minutes(5),
        );
        let mut claims = Claims::default()
            .subject("user-1".to_string())
            .sid("sid-1".to_string());
        claims.azp = Some("client-1".to_string());
        let token = config.generate_oauth_access_token(claims).unwrap();

        assert!(matches!(
            config.validate_session_access_token(&token),
            Err(JwtValidationError::UnexpectedClaim("azp"))
        ));
    }

    #[test]
    fn oauth_access_validation_enforces_audience_when_requested() {
        let config = JwtConfig::new(
            Algorithm::HS256,
            KeySource::Secret("secret".to_string()),
            Duration::minutes(5),
            Duration::minutes(5),
        );
        let mut claims = Claims::default().subject("user-1".to_string());
        claims.azp = Some("client-1".to_string());
        claims.aud = Some("gateway".to_string());
        let token = config.generate_oauth_access_token(claims).unwrap();

        assert!(
            config
                .validate_oauth_access_token(&token, Some("gateway"))
                .is_ok()
        );
        assert!(matches!(
            config.validate_oauth_access_token(&token, Some("admin")),
            Err(JwtValidationError::AudienceMismatch { .. })
        ));
    }

    #[test]
    fn oidc_id_token_has_no_typ_and_is_not_an_access_token() {
        let config = JwtConfig::new(
            Algorithm::HS256,
            KeySource::Secret("secret".to_string()),
            Duration::minutes(5),
            Duration::minutes(5),
        );
        let claims = Claims::default()
            .subject("user-1".to_string())
            .aud("client-1".to_string());
        let token = config.generate_oidc_id_token(claims).unwrap();

        assert!(config.validate_token(&token).unwrap().typ.is_none());
        assert!(config.validate_oauth_access_token(&token, None).is_err());
        assert!(config.validate_session_access_token(&token).is_err());
    }

    #[test]
    fn oidc_id_token_validation_enforces_audience_and_nonce() {
        let config = JwtConfig::new(
            Algorithm::HS256,
            KeySource::Secret("secret".to_string()),
            Duration::minutes(5),
            Duration::minutes(5),
        );
        let mut claims = Claims::default()
            .subject("user-1".to_string())
            .aud("client-1".to_string());
        claims.nonce = Some("nonce-1".to_string());
        let token = config.generate_oidc_id_token(claims).unwrap();

        assert!(
            config
                .validate_oidc_id_token(&token, "client-1", Some("nonce-1"))
                .is_ok()
        );
        assert!(matches!(
            config.validate_oidc_id_token(&token, "client-1", Some("bad")),
            Err(JwtValidationError::NonceMismatch { .. })
        ));
    }
}
