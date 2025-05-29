use super::SignupRequestBody;
use crate::err::{ErrorResponse, HttpError};
use actix_web::{post, web, HttpResponse};
use db::{
    ent::signup::{Payload as SignupPayload, Signup},
    ent::user::User,
};
use smtp::{Smtp, Template};
use uuid::Uuid;

#[utoipa::path(
    context_path = "/signup",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[post("")]
pub async fn handler(body: web::Json<SignupRequestBody>) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();

    let user = User::get_by_email(&body.email).await.unwrap();
    if user.is_some() {
        return Err(ErrorResponse::from(HttpError::Conflict(
            "User already exists".to_string(),
        )));
    }
    let signup_request = Signup::get_by_email(&body.email).await.unwrap();
    if signup_request.is_some() {
        return Err(ErrorResponse::from(HttpError::Conflict(
            "Signup request already exists".to_string(),
        )));
    }

    let uuid = Uuid::new_v4().to_string();

    let sender = Smtp::new()
        .template(Template::SignupRequest)
        .to(body.email.clone())
        .token(uuid.clone())
        .build()
        .send();

    if let Err(e) = sender {
        let message = e.to_string();
        return Err(ErrorResponse::from(HttpError::InternalServerError(message)));
    }

    let signup = SignupPayload {
        email: body.email.clone(),
        uuid: uuid.clone(),
    };

    let response = Signup::save(signup).await;
    if response.is_err() {
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not create signup request".to_string(),
        )));
    }

    Ok(HttpResponse::Ok().json(web::Json("Signup request sent")))
}
