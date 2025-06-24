mod claims;

pub use claims::Claims;
pub use jsonwebtoken::errors::Error as JwtError;
pub use jsonwebtoken::Algorithm;

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use std::fs;

/// JWT Configuration struct to hold preloaded keys and algorithm
pub struct JwtConfig {
    algorithm: Algorithm,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtConfig {
    /// Load configuration once at startup
    pub fn new(
        algorithm: Algorithm,
        private_key_path: String,
        public_key_path: String,
        secret: Option<String>,
    ) -> Self {
        let encoding_key = match algorithm {
            Algorithm::RS256 | Algorithm::RS512 => {
                let key_data =
                    fs::read_to_string(&private_key_path).expect("Failed to read private key");
                EncodingKey::from_rsa_pem(key_data.as_bytes()).expect("Invalid RSA private key")
            }
            Algorithm::ES256 | Algorithm::ES384 => {
                let key_data =
                    fs::read_to_string(&private_key_path).expect("Failed to read EC private key");
                EncodingKey::from_ec_pem(key_data.as_bytes()).expect("Invalid EC private key")
            }
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
                let secret = secret.clone().expect("HS256 requires a secret key");
                EncodingKey::from_secret(secret.as_bytes())
            }
            _ => panic!("Unsupported algorithm: {:?}", algorithm),
        };

        let decoding_key = match algorithm {
            Algorithm::RS256 | Algorithm::RS512 => {
                let key_data =
                    fs::read_to_string(&public_key_path).expect("Failed to read public key");
                DecodingKey::from_rsa_pem(key_data.as_bytes()).expect("Invalid RSA public key")
            }
            Algorithm::ES256 | Algorithm::ES384 => {
                let key_data =
                    fs::read_to_string(&public_key_path).expect("Failed to read EC public key");
                DecodingKey::from_ec_pem(key_data.as_bytes()).expect("Invalid EC public key")
            }
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
                let secret = secret.expect("HS256 requires a secret key");
                DecodingKey::from_secret(secret.as_bytes())
            }
            _ => panic!("Unsupported algorithm: {:?}", algorithm),
        };

        Self {
            algorithm,
            encoding_key,
            decoding_key,
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
