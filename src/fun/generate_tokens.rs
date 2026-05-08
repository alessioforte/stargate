use crate::etc::jwt::jwt_config;

pub fn generate_tokens(claims: jwt::Claims) -> Result<(String, String), jwt::JwtError> {
    let jwt = jwt_config();

    let jwt_refresh_claims = jwt::Claims::default()
        .subject(claims.sub.clone())
        .sub_id(claims.sub_id.clone().unwrap_or_default())
        .sid(claims.sid.clone().unwrap_or_default());

    let jwt_access = jwt.generate_session_access_token(claims)?;
    let jwt_refresh = jwt.generate_session_refresh_token(jwt_refresh_claims)?;

    Ok((jwt_access, jwt_refresh))
}
