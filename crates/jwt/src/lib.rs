use chrono::{Duration, Utc};
use jsonwebtoken::{
    decode, encode, errors, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use once_cell::sync::Lazy;
use password::generate_password;
use serde::{Deserialize, Serialize};
use std::{env, fs};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,              // subject
    pub sub_id: String,           // subject id
    pub email: String,            // email
    pub name: Option<String>,     // name
    pub email_verified: bool,     // email_verified
    pub nickname: Option<String>, // nickname
    pub iat: usize,               // issued at
    pub iss: String,              // issuer
    pub exp: usize,               // expiration
}

impl Default for Claims {
    fn default() -> Self {
        let iss = env::var("JWT_ISSUER").unwrap_or_else(|_| "issuer".to_string());
        Claims {
            iss,
            sub: "".to_string(),
            sub_id: "".to_string(),
            email: "".to_string(),
            name: None,
            email_verified: false,
            nickname: None,
            iat: 0,
            exp: 0,
        }
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
    fn generate_token(&self, claims: &Claims) -> String {
        encode(&Header::new(self.algorithm), claims, &self.encoding_key)
            .expect("Token generation failed")
    }

    /// Validate a JWT token
    pub fn validate_token(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        let validation = Validation::new(self.algorithm);
        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)?;
        Ok(token_data.claims)
    }

    pub fn create_tokens(&self, claims: Claims) -> Result<(String, String), errors::Error> {
        let jwt_access_exp = env::var("JWT_ACCESS_EXPIRATION_MINUTES")
            .unwrap_or_else(|_| "60".to_string())
            .parse::<i64>()
            .unwrap();
        let jwt_refresh_exp = env::var("JWT_REFRESH_EXPIRATION_DAYS")
            .unwrap_or_else(|_| "1440".to_string())
            .parse::<i64>()
            .unwrap();

        let mut jwt_access_claims = claims.clone();
        let mut jwt_refresh_claims = claims.clone();
        let now = Utc::now();
        jwt_access_claims.iat = now.timestamp() as usize;
        jwt_access_claims.exp = (now + Duration::minutes(jwt_access_exp)).timestamp() as usize;
        let jwt_access = self.generate_token(&jwt_access_claims);

        jwt_refresh_claims.iat = now.timestamp() as usize;
        jwt_refresh_claims.exp = (now + Duration::days(jwt_refresh_exp)).timestamp() as usize;
        let jwt_refresh = self.generate_token(&jwt_refresh_claims);
        Ok((jwt_access, jwt_refresh))
    }
}

pub static JWT_CONFIG: Lazy<JwtConfig> = Lazy::new(|| {
    let algorithm = env::var("JWT_ALGORITHM")
        .unwrap_or_else(|_| "HS256".to_string()) // default algorithm
        .parse::<Algorithm>()
        .expect("Invalid JWT algorithm");
    let private_key_path =
        env::var("JWT_PRIVATE_KEY_PATH").unwrap_or_else(|_| ".stargate/private.pem".to_string());
    let public_key_path =
        env::var("JWT_PUBLIC_KEY_PATH").unwrap_or_else(|_| ".stargate/public.pem".to_string());
    let mut secret = env::var("JWT_SECRET").ok();
    log::info!("JWT Algorithm: {:?}", algorithm);
    if secret.is_none() && algorithm == Algorithm::HS256 {
        secret = Some(generate_password(512, false, true, true, false));
        log::warn!(
            "JWT_SECRET not set, generating a random secret key, {}",
            secret.as_ref().unwrap()
        );
    }
    JwtConfig::new(algorithm, private_key_path, public_key_path, secret)
});

pub fn init() {
    Lazy::force(&JWT_CONFIG);
}

pub fn jwt_config() -> &'static JwtConfig {
    &JWT_CONFIG
}
