use crate::err::{ErrorResponse, HttpError};
use crate::etc::jwt::jwt_config;
use crate::etc::sub::{Subject, SubjectType};
use store::Store;

use super::types::AuthenticatedUser;

pub(crate) async fn authenticated_user(
    token: Option<String>,
) -> Result<AuthenticatedUser, ErrorResponse> {
    let token = token.ok_or_else(|| {
        ErrorResponse::from(HttpError::Unauthorized("Token not found".to_string()))
    })?;

    let claims = jwt_config()
        .validate_session_access_token(&token)
        .map_err(|_| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    if crate::act::token_revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid Token".to_string(),
        )));
    }

    let sid = claims
        .sid
        .clone()
        .ok_or_else(|| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;
    let subject = crate::etc::store::use_store()
        .get::<Subject>(&sid)
        .await
        .map_err(|_| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?
        .ok_or_else(|| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    if subject.sub_type != SubjectType::User {
        return Err(ErrorResponse::from(HttpError::Forbidden(
            "MFA is only available for user sessions".to_string(),
        )));
    }

    if claims.sub_id.as_deref() != Some(&subject.id) {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid Token".to_string(),
        )));
    }

    let user = crate::db::get_user_by_id(&subject.id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    Ok(AuthenticatedUser { user, sid })
}
