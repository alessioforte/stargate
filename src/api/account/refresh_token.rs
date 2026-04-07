use super::AuthResponse;
use super::RefreshTokenRequestBody;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::{self, jwt::jwt_config, sub::Subject};
use actix_web::{HttpResponse, cookie::Cookie, put, web};
use store::Store;

#[utoipa::path(
    context_path = "/account",
    path = "/refresh-token",
    tags = ["Account"],
    summary = "Refresh Access Token",
    description = "Refresh the access token using a valid refresh token. This endpoint validates the provided refresh token and issues a new access token along with a new refresh token.",
    responses(
        (status = 200, description = "OK", body = AuthResponse),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[put("/refresh-token")]
pub async fn handler(
    body: web::Json<RefreshTokenRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();
    let refresh_token = body.refresh_token.clone();
    let jwt = jwt_config();
    let claims = match jwt.validate_token(&refresh_token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )));
        }
    };

    let sid = match claims.sid {
        Some(sid) => sid,
        None => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )));
        }
    };

    let store = etc::store::use_store();
    let session = match store.get::<Subject>(&sid).await {
        Ok(s) => s,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )));
        }
    };

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

    let user = match crate::db::get_user_by_username(&claims.sub).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::internal(e));
        }
    };

    let user = user.unwrap();

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = crate::fun::format_name(&given_name, &family_name);

    let sid = ulid::Ulid::new().to_string();
    let mut claims = jwt::Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name.clone())
        .email(user.email.to_owned())
        .email_verified(true)
        .sid(sid.clone());

    if user.nickname == crate::etc::consts::STARGATE_ADMIN {
        claims = claims.role(crate::etc::consts::STARGATE_ADMIN.to_string());
    }

    let (access_token, refresh_token) = match crate::fun::generate_tokens(claims) {
        Ok(tokens) => tokens,
        Err(e) => {
            return Err(ErrorResponse::internal(e));
        }
    };

    // Store the user ID in the session
    let store = etc::store::use_store();
    let refresh_exp = jwt_config().refresh_exp;
    let access_exp = jwt_config().access_exp;
    let sttl: u64 = refresh_exp.as_seconds_f64() as u64;
    let cttl: i64 = access_exp.as_seconds_f64() as i64;

    let subject = etc::sub::Subject::from(user.clone());

    match store.set(&sid, &subject, Some(sttl)).await {
        Ok(_) => {}
        Err(e) => {
            return Err(ErrorResponse::internal(e));
        }
    }

    let cookie = Cookie::build("jwt", access_token.clone())
        .path("/")
        .http_only(true)
        .same_site(actix_web::cookie::SameSite::Lax)
        .max_age(actix_web::cookie::time::Duration::seconds(cttl))
        .finish();

    Ok(HttpResponse::Ok()
        .cookie(cookie)
        .json(web::Json(AuthResponse {
            access_token,
            refresh_token,
            token_type: "Bearer".to_string(),
        })))
}
