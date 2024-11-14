use crate::errors::{ErrorResponse, HttpError};
use crate::modules::hash::Hash;
use crate::modules::password_policies::{PasswordPolicy, PasswordPolicyValidator};
use crate::services::smtp::{Smtp, Template};
use crate::{
    models::signup::{Payload as SignupPayload, Signup},
    models::users::User,
};
use actix_web::{get, post, put, web, HttpResponse};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct SignupRequestBody {
    email: String,
}

#[utoipa::path(
    context_path = "/signup",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[post("")]
pub async fn signup_request(
    body: web::Json<SignupRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
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

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize)]
struct SignupConfirmParams {
    token: String,
}

#[utoipa::path(
    context_path = "/signup",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("")]
pub async fn signup_confirm(
    query: web::Query<SignupConfirmParams>,
) -> Result<HttpResponse, ErrorResponse> {
    let query = query.into_inner();
    let token = query.token.clone();
    let response = Signup::get_by_uuid(&token).await;
    match response {
        Ok(signup) => {
            if signup.is_none() {
                return Err(ErrorResponse::from(HttpError::NotFound(
                    "Signup not found".to_string(),
                )));
            }
            Ok(HttpResponse::Ok().json(web::Json(signup)))
        }
        Err(_) => Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not get signup request".to_string(),
        ))),
    }
}

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct SignupCompleteRequestBody {
    token: String,
    name: String,
    nickname: String,
    password: String,
}

#[utoipa::path(
    context_path = "/signup",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[put("")]
pub async fn signup_complete(
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

pub fn routes() -> actix_web::Scope {
    web::scope("/signup")
        .service(signup_request)
        .service(signup_confirm)
        .service(signup_complete)
}
