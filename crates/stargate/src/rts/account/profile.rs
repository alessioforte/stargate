use crate::act::get_token_from_request;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use actix_web::{get, web, HttpRequest, HttpResponse};
use db::Transaction;
use etc::jwt::jwt_config;

#[utoipa::path(
    context_path = "/account",
    path = "/profile",
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("/profile")]
pub async fn handler(req: HttpRequest) -> Result<HttpResponse, ErrorResponse> {
    let token = get_token_from_request(&req);

    let jwt = jwt_config();
    let claims = match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )))
        }
    };

    let service = etc::db::service();
    let user = match service.get_user_by_username(&claims.sub).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();
    Ok(HttpResponse::Ok().json(web::Json(user)))
}
