use crate::claims::Claims;
use chrono::Duration;
use jsonwebtoken::Algorithm;

use jsonwebtoken::jwk::{Jwk, JwkSet, PublicKeyUse};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use std::fs;

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
    pub access_exp: Duration,
    pub refresh_exp: Duration,
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

    /// Validate a JWT token
    pub fn validate_token(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        let validation = Validation::new(self.algorithm);
        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)?;
        Ok(token_data.claims)
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
}
