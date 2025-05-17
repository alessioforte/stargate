use super::ChangePasswordRequestBody;
use crate::ent::reset_password::PasswordReset;
use crate::ent::user::User;
use crate::err::{ErrorResponse, HttpError};
use crate::pks::hash::Hash;
use actix_web::{put, web, HttpResponse};
use chrono::Utc;

#[utoipa::path(
    context_path = "/account",
    path = "/reset-password",
    responses(
        (status = 200, description = "OK")
    )
)]
#[put("/reset-password")]
pub async fn handler(
    body: web::Json<ChangePasswordRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();
    let token = body.token.clone();
    let change_request = PasswordReset::get_by_uuid(&token).await.unwrap();
    if change_request.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "Change request not found".to_string(),
        )));
    }
    let change_request = change_request.unwrap();

    if change_request.expires_at < Utc::now().timestamp() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Change request expired".to_string(),
        )));
    }

    let user = User::get_by_email(&change_request.email).await.unwrap();
    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();
    let password = Hash::encode(&body.password).unwrap();
    let response = User::change_password(&user.id, &password).await;
    if response.is_err() {
        log::error!("Could not update password: {:?}", response.err());
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not update password".to_string(),
        )));
    }

    let response = PasswordReset::delete(&change_request.id).await;
    if response.is_err() {
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not delete change request".to_string(),
        )));
    }

    // send email to notify user of password change
    Ok(HttpResponse::Ok().json(web::Json("Change Password")))
}
