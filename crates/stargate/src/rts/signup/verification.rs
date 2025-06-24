use super::{EmailVerificationResponse, SignupVerificationParams};
use crate::err::{ErrorResponse, HttpError};
use actix_web::{get, web, HttpResponse};
use db::ent::{Action, ActionType};
use db::Transaction;
use uuid::Uuid;

#[utoipa::path(
    context_path = "/signup",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("")]
pub async fn handler(
    query: web::Query<SignupVerificationParams>,
) -> Result<HttpResponse, ErrorResponse> {
    let query = query.into_inner();
    let token = query.token.clone();
    let service = crate::etc::db::service();
    let jwt = crate::etc::jwt::jwt_config();
    let claim = match jwt.validate_token(&token) {
        Ok(claim) => claim,
        Err(e) => return Err(ErrorResponse::from(HttpError::BadRequest(e.to_string()))),
    };

    let uuid = match claim.sub_id {
        Some(uuid) => uuid,
        None => {
            return Err(ErrorResponse::from(HttpError::BadRequest(
                "Invalid token".to_string(),
            )))
        }
    };

    let request = match service.get_action_by_value(&uuid).await {
        Ok(request) => request,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    if request.is_none() {
        return Err(ErrorResponse::from(HttpError::NotFound(
            "Signup request not found".to_string(),
        )));
    }
    let request = request.unwrap();
    if request.exp < chrono::Utc::now().timestamp() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Signup request expired".to_string(),
        )));
    }

    if claim.email.is_some() && request.sub != claim.email.clone().unwrap() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Email does not match".to_string(),
        )));
    }

    let email = claim.email.unwrap_or_default();
    let uuid = Uuid::new_v4().to_string();

    let claim = jwt::Claims::default()
        .sub_id(uuid.clone())
        .email(Some(email.clone()));

    let jwt = crate::etc::jwt::jwt_config();
    let token = match jwt.generate_token(&claim) {
        Ok(token) => token,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    let action = Action::new(
        email.clone(),
        ActionType::Signup,
        60 * 60, // 1 hour expiration
        uuid.clone(),
    );

    match service.create_action(action).await {
        Ok(_) => {}
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    Ok(HttpResponse::Ok().json(web::Json(EmailVerificationResponse { token, email })))
}
