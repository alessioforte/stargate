use crate::etc::jwt::jwt_config;
use chrono::{Duration, Utc};
use std::env;

pub fn generate_tokens(claims: jwt::Claims) -> Result<(String, String), jwt::JwtError> {
    let jwt = jwt_config();

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
    let jwt_access = jwt.generate_token(&jwt_access_claims)?;

    jwt_refresh_claims.iat = now.timestamp() as usize;
    jwt_refresh_claims.exp = (now + Duration::days(jwt_refresh_exp)).timestamp() as usize;
    let jwt_refresh = jwt.generate_token(&jwt_refresh_claims)?;

    Ok((jwt_access, jwt_refresh))
}
