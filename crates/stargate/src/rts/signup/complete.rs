use super::SignupCompleteRequestBody;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::msg::MessageResponse;
use actix_web::{HttpResponse, put, web};
use db::Transaction;
use db::ent::{CredentialType, Profile};
use pw::Hash;
use pw::{PasswordPolicy, PasswordPolicyValidator};

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

    let jwt = crate::etc::jwt::jwt_config();
    let claim = match jwt.validate_token(&body.token) {
        Ok(claim) => claim,
        Err(e) => return Err(ErrorResponse::from(HttpError::BadRequest(e.to_string()))),
    };

    let uuid = match claim.sub_id {
        Some(uuid) => uuid,
        None => {
            return Err(ErrorResponse::from(HttpError::BadRequest(
                "Invalid token".to_string(),
            )));
        }
    };

    let service = crate::etc::db::service();
    let signup = match service.get_action_by_value(&uuid).await {
        Ok(signup) => signup,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if signup.is_none() {
        return Err(ErrorResponse::from(HttpError::NotFound(
            "Signup request not found".to_string(),
        )));
    }

    let signup = signup.unwrap();
    if signup.exp < chrono::Utc::now().timestamp() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Signup request expired".to_string(),
        )));
    }

    if claim.email.is_some() && signup.sub != claim.email.unwrap() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Email does not match".to_string(),
        )));
    }

    // Check if a user already exists with this email
    let user = match service.get_user_by_username(&signup.sub).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if user.is_some() {
        // If a user already exists with this email, we return a conflict error
        return Err(ErrorResponse::from(HttpError::Conflict(
            "User already exists".to_string(),
        )));
    }

    let user = match service.get_user_by_username(&body.nickname).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if user.is_some() {
        // If a user already exists with this nickname, we return a conflict error
        return Err(ErrorResponse::from(HttpError::Conflict(
            "Nickname already exists".to_string(),
        )));
    }

    let password_policies = PasswordPolicy::standard();
    let validate_password = password_policies.validate(&body.password);
    if validate_password.is_err() {
        let message = validate_password.unwrap_err();
        return Err(ErrorResponse::from(HttpError::BadRequest(message)));
    }

    let user = Profile::new(signup.sub)
        .first_name(Some(body.first_name.clone()))
        .last_name(Some(body.last_name.clone()))
        .nickname(Some(body.nickname.clone()))
        .phone_number(body.phone_number.clone())
        .picture(None);

    let password = Hash::encode(&body.password).unwrap();
    match service
        .create_user(user, CredentialType::Password, &password)
        .await
    {
        Ok(_) => {
            log::info!("User created successfully");

            let message = MessageResponse::new(
                "User created successfully".to_string(),
                "signup_completed".to_string(),
            );

            Ok(HttpResponse::Ok().json(web::Json(message)))
        }
        Err(e) => Err(ErrorResponse::from(HttpError::InternalServerError(
            e.to_string(),
        ))),
    }
}
