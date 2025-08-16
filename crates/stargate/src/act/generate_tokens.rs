use crate::etc::jwt::jwt_config;
use chrono::Utc;

// TODO: the claims should be a struct with the necessary fields
//       and differentiate between access and refresh tokens
pub fn generate_tokens(claims: jwt::Claims) -> Result<(String, String), jwt::JwtError> {
    let jwt = jwt_config();
    let mut jwt_access_claims = claims.clone();
    let mut jwt_refresh_claims = claims.clone();
    let now = Utc::now();
    jwt_access_claims.iat = now.timestamp() as usize;
    jwt_access_claims.exp = (now + jwt.access_exp).timestamp() as usize;
    let jwt_access = jwt.generate_token(&jwt_access_claims)?;
    jwt_refresh_claims.iat = now.timestamp() as usize;
    jwt_refresh_claims.exp = (now + jwt.refresh_exp).timestamp() as usize;
    let jwt_refresh = jwt.generate_token(&jwt_refresh_claims)?;
    Ok((jwt_access, jwt_refresh))
}
