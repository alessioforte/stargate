use chrono::{Duration, Utc};
use std::env;

use jsonwebtoken::{decode, encode, errors, DecodingKey, EncodingKey, Header};
use serde::{Deserialize, Serialize};

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

// defautl algorithm is HS256
pub fn generate_token(claims: Claims) -> Result<String, errors::Error> {
    let jwt_secret_key = env::var("JWT_SECRET_KEY").unwrap_or_else(|_| "secret".to_string());
    let header = Header::default();
    let encoding_key = EncodingKey::from_secret(jwt_secret_key.as_ref());
    encode(&header, &claims, &encoding_key)
}

pub fn validate_token(token: &str) -> Result<Claims, errors::Error> {
    let jwt_secret_key = env::var("JWT_SECRET_KEY").unwrap_or_else(|_| "secret".to_string());
    if token.is_empty() {
        return Err(errors::Error::from(errors::ErrorKind::InvalidToken));
    }
    let encoding_key = DecodingKey::from_secret(jwt_secret_key.as_ref());

    decode::<Claims>(token, &encoding_key, &jsonwebtoken::Validation::default())
        .map(|data| data.claims)
}

pub fn create_tokens(
    claims: Claims,
    // access_token_expiration: i64,
    // refresh_token_expiration: i64,
    // secret_key: &str,
) -> Result<(String, String), errors::Error> {
    // let jwt_secret_key = env::var("JWT_SECRET_KEY").unwrap_or_else(|_| "secret".to_string());
    let access_token_expiration = env::var("ACCESS_TOKEN_EXPIRATION")
        .unwrap_or_else(|_| "60".to_string())
        .parse::<i64>()
        .unwrap();
    let refresh_token_expiration = env::var("REFRESH_TOKEN_EXPIRATION")
        .unwrap_or_else(|_| "1440".to_string())
        .parse::<i64>()
        .unwrap();

    let mut access_token_claims = claims.clone();
    let mut refresh_token_claims = claims.clone();
    let now = Utc::now();
    access_token_claims.iat = now.timestamp() as usize;
    access_token_claims.exp =
        (now + Duration::minutes(access_token_expiration)).timestamp() as usize;
    let access_token = generate_token(access_token_claims).unwrap();

    refresh_token_claims.iat = now.timestamp() as usize;
    refresh_token_claims.exp =
        (now + Duration::minutes(refresh_token_expiration)).timestamp() as usize;
    let refresh_token = generate_token(refresh_token_claims).unwrap();
    Ok((access_token, refresh_token))
}
