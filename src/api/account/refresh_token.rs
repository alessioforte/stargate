use super::{AuthResponse, RefreshTokenRequestBody};
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::{self, jwt::jwt_config, sub::Subject};
use crate::fun::build_jwt_cookie;
use axum::Json;
use axum::response::{IntoResponse, Response};
use http::header::SET_COOKIE;
use store::Store;

#[utoipa::path(
    put,
    path = "/account/refresh-token",
    tags = ["Account"],
    summary = "Refresh Access Token",
    description = "Refresh the access token using a valid refresh token. This endpoint validates the provided refresh token and issues a new access token along with a new refresh token.",
    request_body = RefreshTokenRequestBody,
    responses(
        (status = 200, description = "OK", body = AuthResponse),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn put_refresh_token(
    Json(body): Json<RefreshTokenRequestBody>,
) -> Result<Response, ErrorResponse> {
    let claims = jwt_config()
        .validate_session_refresh_token(&body.refresh_token)
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
    let presented_jti = claims
        .jti
        .as_deref()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;

    let store = etc::store::use_store();
    let session = store
        .get::<Subject>(&sid)
        .await
        .map_err(|_| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;

    let old_subject = match session {
        Some(subject) => subject,
        None => {
            return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
        }
    };

    let user = crate::db::get_user_by_username(&claims.sub)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;

    // Re-validate the session's active org: a user removed from the org
    // cannot carry its context past the access-token lifetime. The role is
    // re-read so role changes propagate on rotation.
    let org = match old_subject.org_id.as_deref() {
        Some(org_id) => {
            let membership = crate::db::get_user_organization(&user.id, org_id)
                .await
                .map_err(ErrorResponse::internal)?
                .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
            Some(crate::act::sessions::OrgContext::from(&membership))
        }
        None => None,
    };

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = crate::fun::format_name(&given_name, &family_name);

    let auth_time = claims
        .auth_time
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
    let mut new_claims = jwt::Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name)
        .email(user.email.to_owned())
        .email_verified(true)
        .sid(sid.clone());
    new_claims.auth_time = Some(auth_time);
    new_claims.org_id = org.as_ref().map(|org| org.org_id.clone());

    let is_super_admin = crate::fun::is_super_admin_user_id(&user.id)
        .await
        .map_err(ErrorResponse::internal)?;
    if is_super_admin {
        new_claims = new_claims.role(crate::fun::SUPER_ADMIN_ROLE.to_string());
    }

    let tokens = crate::fun::generate_tokens(new_claims).map_err(ErrorResponse::internal)?;

    let refresh_exp = jwt_config().refresh_exp;
    let access_exp = jwt_config().access_exp;
    let sttl: u64 = refresh_exp.as_seconds_f64() as u64;
    let cttl: i64 = access_exp.as_seconds_f64() as i64;

    let mut subject = Subject::from(user.clone());
    subject.org_id = org.as_ref().map(|org| org.org_id.clone());
    subject.org_role = org.as_ref().map(|org| org.role.clone());
    let rotated =
        crate::act::sessions::rotate_refresh_token(&sid, presented_jti, &tokens.refresh_jti, sttl)
            .await
            .map_err(ErrorResponse::internal)?;
    if !rotated {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }

    store
        .set(&sid, &subject, Some(sttl))
        .await
        .map_err(ErrorResponse::internal)?;
    if let Err(error) = crate::act::sessions::renew_session(
        &user.id,
        &sid,
        subject.org_id.as_deref(),
        auth_time as u64,
        sttl,
    )
    .await
    {
        return Err(ErrorResponse::internal(error));
    }

    let cookie = build_jwt_cookie(&tokens.access_token, cttl);
    let body = AuthResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        token_type: "Bearer".to_string(),
        org_id: subject.org_id.clone(),
    };

    let mut resp = Json(body).into_response();
    resp.headers_mut().insert(
        SET_COOKIE,
        cookie
            .parse()
            .map_err(|_| ErrorResponse::internal("invalid cookie header"))?,
    );
    Ok(resp)
}
