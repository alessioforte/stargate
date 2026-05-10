use crate::etc::jwt::jwt_config;

pub fn generate_tokens(claims: jwt::Claims) -> Result<(String, String), jwt::JwtError> {
    let jwt = jwt_config();

    let mut jwt_refresh_claims = jwt::Claims::default()
        .subject(claims.sub.clone())
        .sub_id(claims.sub_id.clone().unwrap_or_default())
        .sid(claims.sid.clone().unwrap_or_default());
    jwt_refresh_claims.auth_time = claims.auth_time;

    let jwt_access = jwt.generate_session_access_token(claims)?;
    let jwt_refresh = jwt.generate_session_refresh_token(jwt_refresh_claims)?;

    Ok((jwt_access, jwt_refresh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_refresh_token_preserves_auth_time() {
        crate::etc::tls::install_crypto_provider();
        let mut claims = jwt::Claims::default()
            .subject("alice@example.com".to_string())
            .sub_id("user-1".to_string())
            .sid("sid-1".to_string());
        claims.auth_time = Some(1_700_000_000);

        let (_, refresh_token) = generate_tokens(claims).unwrap();
        let refresh_claims = jwt_config()
            .validate_session_refresh_token(&refresh_token)
            .unwrap();

        assert_eq!(refresh_claims.auth_time, Some(1_700_000_000));
    }
}
