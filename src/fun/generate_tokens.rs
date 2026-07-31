use crate::etc::jwt::jwt_config;

pub struct SessionTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub refresh_jti: String,
}

pub fn generate_tokens(claims: jwt::Claims) -> Result<SessionTokens, jwt::JwtError> {
    let jwt = jwt_config();
    // RFC 7519 §4.1.7: `jti` uniquely identifies the refresh token, while
    // `sid` continues to identify the login session.
    let refresh_jti = ulid::Ulid::new().to_string();

    let mut jwt_refresh_claims = jwt::Claims::default()
        .subject(claims.sub.clone())
        .sub_id(claims.sub_id.clone().unwrap_or_default())
        .sid(claims.sid.clone().unwrap_or_default())
        .jti(refresh_jti.clone());
    jwt_refresh_claims.auth_time = claims.auth_time;

    let access_token = jwt.generate_session_access_token(claims)?;
    let refresh_token = jwt.generate_session_refresh_token(jwt_refresh_claims)?;

    Ok(SessionTokens {
        access_token,
        refresh_token,
        refresh_jti,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_refresh_token_preserves_session_context() {
        crate::etc::tls::install_crypto_provider();
        let mut claims = jwt::Claims::default()
            .subject("alice@example.com".to_string())
            .sub_id("user-1".to_string())
            .sid("sid-1".to_string());
        claims.auth_time = Some(1_700_000_000);

        let tokens = generate_tokens(claims).unwrap();
        let refresh_claims = jwt_config()
            .validate_session_refresh_token(&tokens.refresh_token)
            .unwrap();

        assert_eq!(refresh_claims.auth_time, Some(1_700_000_000));
        assert_eq!(refresh_claims.sid.as_deref(), Some("sid-1"));
        assert_eq!(
            refresh_claims.jti.as_deref(),
            Some(tokens.refresh_jti.as_str())
        );
    }
}
