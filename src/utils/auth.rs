use std::env;

use chrono::{Duration, Utc};
use jsonwebtoken::{encode, DecodingKey, EncodingKey, Header};
use serde::{Deserialize, Serialize};

use crate::models::users::User;

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

// TODO: verify expiration
pub fn create_token(
    user: &User,
    minutes_until_expire: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let expiration = Utc::now() + Duration::minutes(minutes_until_expire);

    let claims = Claims {
        sub: user.email.to_owned(),
        sub_id: user.id.to_owned(),
        name: Some(user.name.clone()),
        nickname: user.nickname.clone(),
        email_verified: true,
        iat: Utc::now().timestamp() as usize,
        exp: expiration.timestamp() as usize,
    };

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
