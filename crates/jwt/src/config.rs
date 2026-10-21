use crate::claims::Claims;
use crate::keyring::{KeyError, KeyMaterial};
use chrono::Duration;
use jsonwebtoken::Algorithm;

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Header, Validation, decode, decode_header, encode};
use serde::Serialize;
use std::{error::Error, fmt};

const TOKEN_TYPE_BEARER: &str = "bearer";
const TOKEN_TYPE_REFRESH: &str = "refresh";
const TOKEN_TYPE_ID_TOKEN: &str = "id_token";
/// JOSE `typ` of Back-Channel Logout tokens (OIDC Back-Channel Logout 1.0 §2.4).
const LOGOUT_TOKEN_HEADER_TYPE: &str = "logout+jwt";

/// JWT configuration: signing and verification keys plus token lifetimes.
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
    /// A configuration that signs with `active` and also accepts tokens
    /// signed by `additional` keys (published in the JWKS). Key ids must be
    /// unique.
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
            if !seen.insert(key.key_id()) {
                return Err(KeyError::new(format!(
                    "duplicate JWT key id '{}'",
                    key.key_id()
                )));
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
            .position(|key| key.key_id() == key_id)
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

    fn sign<T: Serialize>(
        key: &KeyMaterial,
        claims: &T,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        Self::sign_with_type(key, None, claims)
    }

    fn sign_with_type<T: Serialize>(
        key: &KeyMaterial,
        typ: Option<&str>,
        claims: &T,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let mut header = Header::new(key.algorithm);
        header.kid = Some(key.key_id.clone());
        if let Some(typ) = typ {
            header.typ = Some(typ.to_string());
        }
        let encoding = key
            .encoding
            .as_ref()
            .expect("signing keys are checked to have a private key");
        encode(&header, claims, encoding)
    }

    /// The key named by the token's `kid`. Stargate always sets `kid`, so a
    /// token without one, or with an unknown one, is rejected.
    fn verification_key(&self, token: &str) -> Result<&KeyMaterial, jsonwebtoken::errors::Error> {
        let header = decode_header(token)?;
        header
            .kid
            .as_deref()
            .and_then(|key_id| self.keys.iter().find(|key| key.key_id() == key_id))
            .ok_or_else(|| jsonwebtoken::errors::ErrorKind::InvalidSignature.into())
    }

    fn decode_claims(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        self.decode_claims_checking_expiry(token, true)
    }

    fn decode_claims_checking_expiry(
        &self,
        token: &str,
        validate_exp: bool,
    ) -> Result<Claims, jsonwebtoken::errors::Error> {
        let key = self.verification_key(token)?;
        // The algorithm comes from the key, never from the token header.
        let mut validation = Validation::new(key.algorithm);
        validation.validate_aud = false;
        validation.validate_exp = validate_exp;
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

    /// Sign an OIDC Back-Channel Logout token with the ID token key; the
    /// claims (`events`, no `nonce`) are built by the caller.
    pub fn generate_logout_token<T: Serialize>(
        &self,
        claims: &T,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        Self::sign_with_type(
            self.id_token_signer(),
            Some(LOGOUT_TOKEN_HEADER_TYPE),
            claims,
        )
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

    /// Validate an `id_token_hint`: an ID token this provider issued, possibly
    /// expired (OIDC Core 3.1.2.1, RP-Initiated Logout 1.0 §2). Checks the
    /// signature, issuer, and ID token shape; callers compare `aud` and `sub`.
    pub fn validate_id_token_hint(&self, token: &str) -> Result<Claims, JwtValidationError> {
        let claims = self.decode_claims_checking_expiry(token, false)?;
        if claims.typ.is_some() {
            return Err(JwtValidationError::InvalidType {
                expected: TOKEN_TYPE_ID_TOKEN,
                actual: claims.typ.clone(),
            });
        }
        Self::require_non_empty_claim(Some(claims.sub.as_str()), "sub")?;
        Self::require_non_empty_claim(claims.aud.as_deref(), "aud")?;
        Ok(claims)
    }

    /// Algorithm of the active signing key.
    pub fn algorithm(&self) -> Algorithm {
        self.active().algorithm
    }

    /// Key id of the active signing key.
    pub fn key_id(&self) -> &str {
        self.active().key_id()
    }

    /// Algorithm OIDC ID tokens are signed with (for discovery metadata).
    pub fn id_token_algorithm(&self) -> Algorithm {
        self.id_token_signer().algorithm
    }

    /// Key id OIDC ID tokens are signed with.
    pub fn id_token_key_id(&self) -> &str {
        self.id_token_signer().key_id()
    }

    /// Ids of all keys tokens are accepted from, active first.
    pub fn key_ids(&self) -> Vec<&str> {
        self.keys.iter().map(KeyMaterial::key_id).collect()
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

    fn hmac_config(key_id: &str) -> JwtConfig {
        JwtConfig::from_keyring(
            KeyMaterial::from_secret(key_id, Algorithm::HS256, b"secret").unwrap(),
            Vec::new(),
            Duration::minutes(5),
            Duration::minutes(5),
        )
        .unwrap()
    }

    #[test]
    fn generated_token_includes_key_id() {
        let config = hmac_config("stargate-test");

        let claims = Claims::default().subject("user-1".to_string());
        let token = config.generate_token(&claims).unwrap();
        let header = decode_header(&token).unwrap();

        assert_eq!(header.kid.as_deref(), Some("stargate-test"));
    }

    #[test]
    fn hmac_public_jwks_does_not_expose_secret() {
        let config = hmac_config("stargate-test");

        let jwks = config.public_jwks().unwrap();

        assert!(jwks.keys.is_empty());
    }

    #[test]
    fn validates_token_with_audience_without_resource_audience_policy() {
        let config = hmac_config("stargate-test");

        let claims = Claims::default()
            .subject("client-1".to_string())
            .aud("gateway".to_string());
        let token = config.generate_token(&claims).unwrap();
        let decoded = config.validate_token(&token).unwrap();

        assert_eq!(decoded.aud.as_deref(), Some("gateway"));
    }

    #[test]
    fn rejects_token_with_wrong_issuer() {
        let config = hmac_config("stargate-test");

        let claims = Claims::default()
            .subject("client-1".to_string())
            .iss("https://attacker.example".to_string());
        let token = config.generate_token(&claims).unwrap();

        assert!(config.validate_token(&token).is_err());
    }

    #[test]
    fn session_access_validation_requires_bearer_with_sid() {
        let config = hmac_config("stargate-test");
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
        let config = hmac_config("stargate-test");
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
        let config = hmac_config("stargate-test");
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
        let config = hmac_config("stargate-test");
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
        let config = hmac_config("stargate-test");
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

    #[test]
    fn id_token_hint_accepts_expired_id_tokens_only() {
        let mut config = hmac_config("stargate-test");
        config.access_exp = Duration::minutes(-10);
        let id_token = config
            .generate_oidc_id_token(
                Claims::default()
                    .subject("user-1".to_string())
                    .aud("client-1".to_string()),
            )
            .unwrap();
        assert!(config.validate_token(&id_token).is_err(), "expired");

        let hint = config.validate_id_token_hint(&id_token).unwrap();
        assert_eq!(hint.sub, "user-1");
        assert_eq!(hint.aud.as_deref(), Some("client-1"));

        // Session tokens are not ID tokens.
        let session = config
            .generate_session_access_token(
                Claims::default()
                    .subject("user-1".to_string())
                    .sid("sid-1".to_string()),
            )
            .unwrap();
        assert!(matches!(
            config.validate_id_token_hint(&session),
            Err(JwtValidationError::InvalidType { .. })
        ));

        // Another signer's token is rejected.
        let other = JwtConfig::from_keyring(
            KeyMaterial::from_secret("stargate-test", Algorithm::HS256, b"other").unwrap(),
            Vec::new(),
            Duration::minutes(5),
            Duration::minutes(5),
        )
        .unwrap()
        .generate_oidc_id_token(
            Claims::default()
                .subject("user-1".to_string())
                .aud("client-1".to_string()),
        )
        .unwrap();
        assert!(config.validate_id_token_hint(&other).is_err());
    }

    #[test]
    fn logout_token_is_typed_and_signed_with_the_id_token_key() {
        let config = hmac_config("stargate-test");
        let token = config
            .generate_logout_token(&serde_json::json!({ "sub": "user-1" }))
            .unwrap();
        let header = decode_header(&token).unwrap();

        assert_eq!(header.typ.as_deref(), Some("logout+jwt"));
        assert_eq!(header.kid.as_deref(), Some(config.id_token_key_id()));
    }
}
