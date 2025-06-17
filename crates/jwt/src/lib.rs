use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::{env, fs};

pub use jsonwebtoken::errors::Error as JwtError;
pub use jsonwebtoken::Algorithm;

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshTokenClaims {
    pub exp: usize,    // expiration
    pub iat: usize,    // issued at
    pub jti: String,   // JWT ID
    pub iss: String,   // issuer
    pub aud: String,   // audience
    pub sub: String,   // subject
    pub typ: String,   // type
    pub azp: String,   // authorized party
    pub sid: String,   // session ID
    pub scope: String, // scope
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,              // subject
    pub sub_id: Option<String>,   // subject id
    pub email: String,            // email
    pub name: Option<String>,     // name
    pub email_verified: bool,     // email_verified
    pub nickname: Option<String>, // nickname
    pub uuid: Option<String>,     // UUID for the user
    pub iat: usize,               // issued at
    pub iss: String,              // issuer
    pub exp: usize,               // expiration
}

impl Default for Claims {
    fn default() -> Self {
        let now = Utc::now();
        let iss = env::var("JWT_ISSUER").unwrap_or_else(|_| "issuer".to_string());

        Claims {
            iss,
            sub: "".to_string(),
            email: "".to_string(),
            sub_id: None,
            name: None,
            email_verified: false,
            nickname: None,
            uuid: None,
            iat: now.timestamp() as usize,
            exp: (now + Duration::minutes(60)).timestamp() as usize,
        }
    }
}

impl Claims {
    pub fn sub(mut self, sub: String) -> Self {
        self.sub = sub;
        self
    }

    pub fn sub_id(mut self, sub_id: String) -> Self {
        self.sub_id = Some(sub_id);
        self
    }

    pub fn email(mut self, email: String) -> Self {
        self.email = email;
        self
    }

    pub fn name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    pub fn email_verified(mut self, email_verified: bool) -> Self {
        self.email_verified = email_verified;
        self
    }

    pub fn nickname(mut self, nickname: Option<String>) -> Self {
        self.nickname = nickname;
        self
    }

    pub fn uuid(mut self, uuid: Option<String>) -> Self {
        self.uuid = uuid;
        self
    }

    pub fn iat(mut self, iat: usize) -> Self {
        self.iat = iat;
        self
    }

    pub fn exp(mut self, exp: usize) -> Self {
        self.exp = exp;
        self
    }
}

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
