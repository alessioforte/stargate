use crate::err::{ErrorResponse, HttpError};
use crate::etc::msg::MessageResponse;
use crate::etc::{ext::RequestExt, jwt::jwt_config, store::use_store};
use actix_web::{HttpRequest, HttpResponse, delete, web};
use store::Store;

#[utoipa::path(
    context_path = "/account",
    path = "/logout",
    tags = ["Account"],
    summary = "User Logout",
    description = "Log out the currently authenticated user by invalidating their session token.",
    responses(
        (status = 200, description = "OK"),
        (status = 401, description = "Unauthorized - Invalid Token"),
        (status = 500, description = "Internal Server Error")
    )
)]
#[delete("/logout")]
pub async fn handler(req: HttpRequest) -> Result<HttpResponse, ErrorResponse> {
    let token = match req.get_token() {
        Some(t) => Some(t),
        None => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Token not found".to_string(),
            )));
        }
    };

    let jwt = jwt_config();
    let claims = match jwt.validate_token(&token.unwrap()) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )));
        }
    };

    let store = use_store();
    let sid = claims.sid.clone().unwrap_or_default();

    match store.delete(&sid).await {
        Ok(_) => (),
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    let message = MessageResponse::new(
        "User logged out successfully".to_string(),
        "logout_success".to_string(),
    );

    Ok(HttpResponse::Ok().json(web::Json(message)))
}
