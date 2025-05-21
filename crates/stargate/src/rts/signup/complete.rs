use super::SignupCompleteRequestBody;
use crate::err::{ErrorResponse, HttpError};
use crate::{ent::signup::Signup, ent::user::User};
use actix_web::{put, web, HttpResponse};
use password::Hash;
use password::{PasswordPolicy, PasswordPolicyValidator};

#[utoipa::path(
    context_path = "/signup",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[put("")]
pub async fn handler(
    body: web::Json<SignupCompleteRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();

    let signup = Signup::get_by_uuid(&body.token).await;
    if signup.is_err() {
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not get signup request".to_string(),
        )));
    }

    // verify nickname
    let user = User::get_by_nickname(&body.nickname).await;
    if user.is_ok() {
        return Err(ErrorResponse::from(HttpError::Conflict(
            "Nickname already exists".to_string(),
        )));
    }

    let password_policies = PasswordPolicy::default();
    let validate_password = password_policies.validate(&body.password);
    if validate_password.is_err() {
        let message = validate_password.unwrap_err();
        return Err(ErrorResponse::from(HttpError::BadRequest(message)));
    }

    match signup.unwrap() {
        Some(signup) => {
            let new_user = User::new()
                .email(signup.email.clone())
                .name(body.name.clone())
                .nickname(Some(body.nickname.clone()))
                .password(Some(Hash::encode(&body.password).unwrap()));
            // .build();

            let response = new_user.save().await;

            if response.is_err() {
                return Err(ErrorResponse::from(HttpError::InternalServerError(
                    response.err().unwrap().to_string(),
                )));
            }

            let response = Signup::delete(&signup.id).await;
            if response.is_err() {
                return Err(ErrorResponse::from(HttpError::InternalServerError(
                    "Could not delete signup request".to_string(),
                )));
            }

            Ok(HttpResponse::Ok().json(web::Json("Signup completed")))
        }
        None => Err(ErrorResponse::from(HttpError::NotFound(
            "Signup not found".to_string(),
        ))),
    }
}
