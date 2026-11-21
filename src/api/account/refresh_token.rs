use super::{AuthResponse, RefreshTokenRequestBody};
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::{auth::jwt::jwt_config, sess::registry};
use crate::web::cookies::build_jwt_cookie;
use axum::Json;
use axum::response::{IntoResponse, Response};
use http::header::SET_COOKIE;

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

    if crate::iam::auth::revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }

    let sid = claims
        .sid
        .as_deref()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
    let presented_jti = claims
        .jti
        .as_deref()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;

    // Use the session owner's id because their email may have changed.
    let user_id = claims
        .sub_id
        .as_deref()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
    let subject = crate::iam::sess::subjects::read(sid)
        .await
        .map_err(|_| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?
        .filter(|subject| subject.id == user_id)
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
    let user = crate::data::db::get_user_by_id(user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;

    if let Some(org_id) = subject.org_id.as_deref()
        && crate::data::db::get_user_organization(user_id, org_id)
            .await
            .map_err(ErrorResponse::internal)?
            .is_none()
    {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = crate::iam::user::profile::format_name(&given_name, &family_name);

    let auth_time = claims
        .auth_time
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
    let mut new_claims = jwt::Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name)
        .email(user.email.to_owned())
        .email_verified(true)
        .sid(sid.to_string());
    new_claims.auth_time = Some(auth_time);
    new_claims.org_id = subject.org_id.clone();

    let is_super_admin = crate::iam::authz::admin::is_super_admin_user_id(&user.id)
        .await
        .map_err(ErrorResponse::internal)?;
    if is_super_admin {
        new_claims = new_claims.role(crate::iam::authz::admin::SUPER_ADMIN_ROLE.to_string());
    }

    let tokens =
        crate::iam::sess::tokens::generate_tokens(new_claims).map_err(ErrorResponse::internal)?;

    let refresh_exp = jwt_config().refresh_exp;
    let access_exp = jwt_config().access_exp;
    let sttl: u64 = refresh_exp.as_seconds_f64() as u64;
    let cttl: i64 = access_exp.as_seconds_f64() as i64;

    let rotated = registry::rotate_refresh_token(sid, presented_jti, &tokens.refresh_jti, sttl)
        .await
        .map_err(ErrorResponse::internal)?;
    if !rotated {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }

    if !registry::renew_session(user_id, sid, sttl)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }

    let cookie = build_jwt_cookie(&tokens.access_token, cttl);
    let body = AuthResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        token_type: "Bearer".to_string(),
        org_id: subject.org_id,
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
