use crate::claims::Claims;
use chrono::Duration;
use jsonwebtoken::Algorithm;

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
            encoding_key,
            decoding_key,
            access_exp,
            refresh_exp,
        }
    }

    /// Generate a JWT token
    pub fn generate_token(&self, claims: &Claims) -> Result<String, jsonwebtoken::errors::Error> {
        encode(&Header::new(self.algorithm), claims, &self.encoding_key)
    }

    /// Validate a JWT token
    pub fn validate_token(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        let validation = Validation::new(self.algorithm);
        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)?;
        Ok(token_data.claims)
    }
}
