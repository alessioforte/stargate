use crate::claims::Claims;
use chrono::Duration;
use jsonwebtoken::Algorithm;

use jsonwebtoken::jwk::{Jwk, JwkSet, PublicKeyUse};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
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
    algorithm: Algorithm,
    key_id: Option<String>,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
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
        let (encoding_key, decoding_key) = match key_source {
            KeySource::Secret(secret) => {
                let bytes = secret.as_bytes();
                (
                    EncodingKey::from_secret(bytes),
                    DecodingKey::from_secret(bytes),
                )
            }
            KeySource::Rsa {
                private_key_path,
                public_key_path,
            } => {
                let priv_pem =
                    fs::read_to_string(&private_key_path).expect("Failed to read RSA private key");
                let pub_pem =
                    fs::read_to_string(&public_key_path).expect("Failed to read RSA public key");
                (
                    EncodingKey::from_rsa_pem(priv_pem.as_bytes())
                        .expect("Invalid RSA private key"),
                    DecodingKey::from_rsa_pem(pub_pem.as_bytes()).expect("Invalid RSA public key"),
                )
            }
            KeySource::Ec {
                private_key_path,
                public_key_path,
            } => {
                let priv_pem =
                    fs::read_to_string(&private_key_path).expect("Failed to read EC private key");
                let pub_pem =
                    fs::read_to_string(&public_key_path).expect("Failed to read EC public key");
                (
                    EncodingKey::from_ec_pem(priv_pem.as_bytes()).expect("Invalid EC private key"),
                    DecodingKey::from_ec_pem(pub_pem.as_bytes()).expect("Invalid EC public key"),
                )
            }
        };

        Self {
            algorithm,
            key_id,
            encoding_key,
            decoding_key,
            issuer: crate::claims::issuer_from_env(),
            access_exp,
            refresh_exp,
        }
    }

    /// Generate a JWT token
    pub fn generate_token(&self, claims: &Claims) -> Result<String, jsonwebtoken::errors::Error> {
        let mut header = Header::new(self.algorithm);
        header.kid = self.key_id.clone();
        encode(&header, claims, &self.encoding_key)
    }

    fn timed_claims(&self, mut claims: Claims, token_type: &str, ttl: Duration) -> Claims {
        let now = chrono::Utc::now();
        claims.typ = Some(token_type.to_string());
        claims.iat = now.timestamp() as usize;
        claims.exp = (now + ttl).timestamp() as usize;
        if claims.jti.is_none() {
            claims.jti = Some(ulid::Ulid::new().to_string());
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
        let claims = self.timed_claims(claims, TOKEN_TYPE_ID_TOKEN, self.access_exp);
        self.generate_token(&claims)
    }

    fn decode_without_audience(&self, token: &str) -> Result<Claims, JwtValidationError> {
        let mut validation = Validation::new(self.algorithm);
        validation.validate_aud = false;
        validation.set_issuer(&[self.issuer.as_str()]);
        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)?;
        Ok(token_data.claims)
    }

    /// Validate a JWT token without enforcing caller intent.
    ///
    /// Prefer the intent-specific helpers for application handlers.
    pub fn validate_token(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        let mut validation = Validation::new(self.algorithm);
        validation.validate_aud = false;
        validation.set_issuer(&[self.issuer.as_str()]);
        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)?;
        Ok(token_data.claims)
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
        Ok(claims)
    }

    pub fn validate_session_refresh_token(
        &self,
        token: &str,
    ) -> Result<Claims, JwtValidationError> {
        let claims = self.decode_without_audience(token)?;
        Self::require_type(&claims, TOKEN_TYPE_REFRESH)?;
        Self::require_non_empty_claim(claims.sid.as_deref(), "sid")?;
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
        Self::require_type(&claims, TOKEN_TYPE_ID_TOKEN)?;
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

    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    pub fn key_id(&self) -> Option<&str> {
        self.key_id.as_deref()
    }

    pub fn public_jwks(&self) -> Result<JwkSet, jsonwebtoken::errors::Error> {
        if matches!(
            self.algorithm,
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512
        ) {
            return Ok(JwkSet { keys: Vec::new() });
        }

        let mut jwk = Jwk::from_encoding_key(&self.encoding_key, self.algorithm)?;
        jwk.common.key_id = self.key_id.clone();
        jwk.common.public_key_use = Some(PublicKeyUse::Signature);

        Ok(JwkSet { keys: vec![jwk] })
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
    fn session_access_validation_requires_bearer_with_session_id() {
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
