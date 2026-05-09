use super::authorization_codes::AuthorizationCodeRecord;
use crate::err::ErrorResponse;

pub(super) fn verify(code: &AuthorizationCodeRecord, verifier: &str) -> Result<(), ErrorResponse> {
    oidc::pkce::verify(code, verifier).map_err(ErrorResponse::from)
}
