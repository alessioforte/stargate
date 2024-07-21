use chrono::{Duration, Utc};
use std::env;

use jsonwebtoken::{decode, encode, errors, DecodingKey, EncodingKey, Header};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,              // subject (email)
    pub sub_id: String,           // subject id
    pub name: Option<String>,     // name
    pub email_verified: bool,     // email_verified
    pub nickname: Option<String>, // nickname
    pub iat: usize,               // issued at
    pub exp: usize,               // expiration
}

impl Default for Claims {
    fn default() -> Self {
        Claims {
            sub: "".to_string(),
            sub_id: "".to_string(),
            name: None,
            email_verified: false,
            nickname: None,
            iat: 0,
            exp: 0,
        }
    }
}

pub fn generate_token(claims: Claims) -> Result<String, errors::Error> {
    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());

    let header = Header::default();
    let encoding_key = EncodingKey::from_secret(secret.as_ref());
    encode(&header, &claims, &encoding_key)
}

pub fn validate_token(token: &str, secret: &str) -> Result<Claims, errors::Error> {
    if token == "" {
        return Err(errors::Error::from(errors::ErrorKind::InvalidToken));
    }
    let encoding_key = DecodingKey::from_secret(secret.as_ref());

    decode::<Claims>(token, &encoding_key, &jsonwebtoken::Validation::default())
        .map(|data| data.claims)
}

pub fn create_tokens(
    claims: Claims,
    access_token_expiration: i64,
    refresh_token_expiration: i64,
) -> Result<(String, String), errors::Error> {
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
