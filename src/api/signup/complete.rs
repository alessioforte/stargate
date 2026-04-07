use super::SignupCompleteRequestBody;
use crate::act;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::msg::MessageResponse;
use crate::fun::format_name;
use actix_web::{HttpMessage, HttpRequest, HttpResponse, put, web};
use db::ent::{AuditContext, CredentialType, Profile};
use pw::Hash;
use pw::{PasswordPolicy, PasswordPolicyValidator};
use smtp::Smtp;
use tracing::error;

#[utoipa::path(
    context_path = "/signup",
    path = "",
    tags = ["Signup"],
    responses(
        (status = 200, description = "OK", body = MessageResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 404, description = "Not Found", body = ErrorResponse),
        (status = 409, description = "Conflict", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
#[put("")]
pub async fn handler(
    req: HttpRequest,
    body: web::Json<SignupCompleteRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = req
        .extensions_mut()
        .remove::<AuditContext>()
        .unwrap_or_else(AuditContext::anonymous);
    let body = body.into_inner();

    let jwt = crate::etc::jwt::jwt_config();
    let claim = match jwt.validate_token(&body.token) {
        Ok(claim) => claim,
        Err(e) => return Err(ErrorResponse::from(HttpError::BadRequest(e.to_string()))),
    };

    let sid = match claim.sub_id {
        Some(sid) => sid,
        None => {
            return Err(ErrorResponse::from(HttpError::BadRequest(
                "Invalid token".to_string(),
            )));
        }
    };

    let signup = match act::get_signup_request(&sid).await {
        Ok(signup) => signup,
        Err(e) => {
            return Err(ErrorResponse::internal(e));
        }
    };

    if signup.is_none() {
        return Err(ErrorResponse::from(HttpError::NotFound(
            "Signup request not found".to_string(),
        )));
    }

    let signup = signup.unwrap();

    if claim.email.is_some() && signup != claim.email.unwrap() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Email does not match".to_string(),
        )));
    }

    // Check if a user already exists with this email
    let user = match crate::db::get_user_by_username(&signup).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::internal(e));
        }
    };

    if user.is_some() {
        // If a user already exists with this email, we return a conflict error
        return Err(ErrorResponse::from(HttpError::Conflict(
            "User already exists".to_string(),
        )));
    }

    let user = match crate::db::get_user_by_username(&body.nickname).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::internal(e));
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

    let user = Profile::new(signup, body.nickname.clone())
        .given_name(Some(body.given_name.clone()))
        .family_name(Some(body.family_name.clone()))
        .phone_number(body.phone_number.clone())
        .picture(None);

    let password = Hash::encode(&body.password).unwrap();

    match crate::db::create_user(user, CredentialType::Password, &password, ctx).await {
        Ok(user) => {
            let _ = act::delete_signup_request(&sid).await;

            let given_name = user.given_name.clone().unwrap_or_default();
            let family_name = user.family_name.clone().unwrap_or_default();

            let sender = Smtp::new()
                .template(smtp::Template::SignupCompleted)
                .to(user.email)
                .name(Some(format_name(&given_name, &family_name)))
                .build()
                .send();

            match sender {
                Ok(_) => {}
                Err(e) => error!("Could not send email: {:?}", e),
            }

            let message = MessageResponse::new("User created successfully", "signup_completed");

            Ok(HttpResponse::Ok().json(web::Json(message)))
        }
        Err(e) => Err(ErrorResponse::internal(e)),
    }
}
