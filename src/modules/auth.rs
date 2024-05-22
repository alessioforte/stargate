use std::env;

use jsonwebtoken::{encode, DecodingKey, EncodingKey, Header};
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

pub fn create_token(claims: Claims) -> Result<String, jsonwebtoken::errors::Error> {
    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());

    let header = Header::default();
    let encoding_key = EncodingKey::from_secret(secret.as_ref());
    encode(&header, &claims, &encoding_key)
}

pub fn validate_token(token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    if token == "" {
        return Err(jsonwebtoken::errors::Error::from(
            jsonwebtoken::errors::ErrorKind::InvalidToken,
        ));
    }
    let secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());
    let encoding_key = DecodingKey::from_secret(secret.as_ref());

    jsonwebtoken::decode::<Claims>(token, &encoding_key, &jsonwebtoken::Validation::default())
        .map(|data| data.claims)
}
