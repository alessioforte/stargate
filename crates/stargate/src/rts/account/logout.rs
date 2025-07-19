use crate::err::{ErrorResponse, HttpError};
use crate::etc::ext::RequestExt;
use crate::etc::jwt::jwt_config;
// use actix_session::Session;
use actix_web::{delete, web, HttpRequest, HttpResponse};

#[utoipa::path(
    context_path = "/account",
    path = "/logout",
    responses(
        (status = 200, description = "OK")
    )
)]
#[delete("/logout")]
pub async fn handler(req: HttpRequest) -> Result<HttpResponse, ErrorResponse> {
    let token = req.get_token();

    let jwt = jwt_config();
    match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )))
        }
    };

    // TODO: Implement proper logout logic, such as invalidating the session or token.
    Ok(HttpResponse::Ok().json(web::Json("Logout User")))
}
