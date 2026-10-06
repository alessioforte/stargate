use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::auth::jwt::jwt_config;
use crate::etc::auth::subject::{Subject, SubjectType};
use store::Store;

use super::types::AuthenticatedUser;

pub(crate) async fn authenticated_user(
    token: Option<String>,
) -> Result<AuthenticatedUser, ErrorResponse> {
    let token = token.ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenMissing))?;

    let claims = jwt_config()
        .validate_session_access_token(&token)
        .map_err(|_| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;

    if crate::act::token_revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }

    let sid = claims
        .sid
        .clone()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
    let subject = crate::etc::store::use_store()
        .get::<Subject>(&sid)
        .await
        .map_err(|_| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;

    if subject.sub_type != SubjectType::User {
        return Err(ErrorResponse::new(ErrorCode::AuthUserSessionRequired));
    }

    if claims.sub_id.as_deref() != Some(&subject.id) {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }

    let user = crate::db::get_user_by_id(&subject.id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;

    Ok(AuthenticatedUser { user, sid })
}
