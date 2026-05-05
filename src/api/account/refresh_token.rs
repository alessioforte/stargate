use super::{AuthResponse, RefreshTokenRequestBody, build_jwt_cookie};
use crate::err::{ErrorResponse, HttpError};
use crate::etc::{self, jwt::jwt_config, sub::Subject};
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
        .validate_token(&body.refresh_token)
        .map_err(|_| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    if claims.typ.as_deref() != Some("refresh") {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid Token".to_string(),
        )));
    }

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

    let store = etc::store::use_store();
    let session = store
        .get::<Subject>(&sid)
        .await
        .map_err(|_| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    match session {
        Some(_) => {
            let _ = store.delete(&sid).await;
        }
        None => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )));
        }
    };

    let user = crate::db::get_user_by_username(&claims.sub)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = crate::fun::format_name(&given_name, &family_name);

    let new_sid = ulid::Ulid::new().to_string();
    let mut new_claims = jwt::Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name)
        .email(user.email.to_owned())
        .email_verified(true)
        .sid(new_sid.clone());

    let is_super_admin = crate::fun::is_super_admin_user_id(&user.id)
        .await
        .map_err(ErrorResponse::internal)?;
    if is_super_admin {
        new_claims = new_claims.role(crate::fun::SUPER_ADMIN_ROLE.to_string());
    }

    let (access_token, refresh_token) =
        crate::fun::generate_tokens(new_claims).map_err(ErrorResponse::internal)?;

    let refresh_exp = jwt_config().refresh_exp;
    let access_exp = jwt_config().access_exp;
    let sttl: u64 = refresh_exp.as_seconds_f64() as u64;
    let cttl: i64 = access_exp.as_seconds_f64() as i64;

    let subject = Subject::from(user.clone());
    store
        .set(&new_sid, &subject, Some(sttl))
        .await
        .map_err(ErrorResponse::internal)?;

    let cookie = build_jwt_cookie(&access_token, cttl);
    let body = AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
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
