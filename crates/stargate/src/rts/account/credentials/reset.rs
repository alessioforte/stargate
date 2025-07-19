use super::ChangePasswordRequestBody;
use crate::act::format_name;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use actix_web::{put, web, HttpResponse};
use chrono::Utc;
use db::Transaction;
use password::Hash;
use smtp::{Smtp, Template};

#[utoipa::path(
    context_path = "/account",
    path = "/credentials",
    responses(
        (status = 200, description = "OK")
    )
)]
#[put("")]
pub async fn handler(
    body: web::Json<ChangePasswordRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();
    let token = body.token.clone();
    let jwt = etc::jwt::jwt_config();
    let claims = match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )))
        }
    };
    let uuid = claims.uuid.clone().unwrap_or_default();
    let service = etc::db::service();
    let pra = match service.get_action_by_value(&uuid).await {
        Ok(action) => action,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    if pra.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "Request not found".to_string(),
        )));
    }

    let pra = pra.unwrap();
    if pra.exp < Utc::now().timestamp() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Request expired".to_string(),
        )));
    }

    let user = match service.get_user_by_username(&pra.sub).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }
    let user = user.unwrap();
    let password = Hash::encode(&body.password).unwrap();

    match service.change_password(&user.id, &password).await {
        Ok(_) => {}
        Err(e) => {
            log::error!("Could not update password: {:?}", e);
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                "Could not update password".to_string(),
            )));
        }
    }

    let message = etc::msg::MessageResponse::new(
        "Change Password".to_string(),
        "change_password".to_string(),
    );

    let first_name = user.first_name.clone().unwrap_or_default();
    let last_name = user.last_name.clone().unwrap_or_default();
    let sender = Smtp::new()
        .template(Template::PasswordChangedNotification)
        .to(user.email.clone())
        .name(Some(format_name(&first_name, &last_name)))
        .token(token)
        .build()
        .send();

    match sender {
        Ok(_) => {}
        Err(e) => log::error!("Could not send email: {:?}", e),
    }

    Ok(HttpResponse::Ok().json(web::Json(message)))
}
