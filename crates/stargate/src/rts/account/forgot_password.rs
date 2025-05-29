use crate::err::{ErrorResponse, HttpError};
use actix_web::{post, web, HttpResponse};
use chrono::{Duration, Utc};
use db::ent::reset_password::{PasswordReset, Payload as PasswordResetPayload};
use db::ent::user::User;
use serde::{Deserialize, Serialize};
use smtp::{Smtp, Template};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ForgotPasswordRequestBody {
    email: String,
}

#[utoipa::path(
    context_path = "/account",
    path = "/forgot-password",
    responses(
        (status = 200, description = "OK")
    )
)]
#[post("/forgot-password")]
pub async fn handler(
    body: web::Json<ForgotPasswordRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();
    let user = User::get_by_email(&body.email).await.unwrap();
    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();
    // TODO: use JWT instead of UUID
    let uuid = Uuid::new_v4().to_string();
    let reset = PasswordReset::save(PasswordResetPayload {
        email: user.email.clone(),
        uuid: uuid.clone(),
        issued_at: Utc::now().timestamp(),
        expires_at: (Utc::now() + Duration::days(1)).timestamp(),
    })
    .await;

    if reset.is_err() {
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not create reset request".to_string(),
        )));
    }

    let sender = Smtp::new()
        .template(Template::ChangePasswordRequest)
        .to(user.email.clone())
        .name(Some(user.name.clone()))
        .token(uuid.clone())
        .build()
        .send();

    match sender {
        Ok(_) => Ok(HttpResponse::Ok().json(web::Json("Email sent"))),
        Err(e) => Err(ErrorResponse::from(HttpError::InternalServerError(
            e.to_string(),
        ))),
    }
}
