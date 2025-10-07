use super::AuthResponse;
use super::RefreshTokenRequestBody;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::jwt::jwt_config;
use actix_web::{HttpResponse, put, web};
use db::Transaction;
use jwt::Claims;

#[utoipa::path(
    context_path = "/account",
    path = "/refresh-token",
    responses(
        (status = 200, description = "OK")
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

    let service = crate::etc::db::service();
    let user = match service.get_user_by_username(&claims.sub).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    let user = user.unwrap();

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = crate::act::format_name(&given_name, &family_name);

    let claims = Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name)
        .email(user.email.clone())
        .email_verified(true);

    let (access_token, refresh_token) = crate::act::generate_tokens(claims).unwrap();

    Ok(HttpResponse::Ok().json(web::Json(AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
    })))
}
