use super::authorization_codes::AuthorizationCodeRecord;
use super::shared::OAuthResult;

pub(super) fn verify(code: &AuthorizationCodeRecord, verifier: &str) -> OAuthResult<()> {
    oidc::pkce::verify(code, verifier).map_err(Into::into)
}
