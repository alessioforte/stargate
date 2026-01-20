use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::ext::RequestExt;
use actix_web::{HttpRequest, HttpResponse, get, web};
use db::ent::User;
use etc::jwt::jwt_config;

#[utoipa::path(
    context_path = "/account",
    path = "/profile",
    tags = ["Account"],
    summary = "Get User Profile",
    description = "Retrieve the profile information of the currently authenticated user using their access token.",
    responses(
        (status = 200, description = "OK", body = User),
        (status = 401, description = "Unauthorized - Invalid Token"),
        (status = 500, description = "Internal Server Error")
    )
)]
#[get("/profile")]
pub async fn handler(req: HttpRequest) -> Result<HttpResponse, ErrorResponse> {
    let token = match req.get_token() {
        Some(t) => t,
        None => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Token not found".to_string(),
            )));
        }
    };

    let jwt = jwt_config();
    let claims = match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )));
        }
    };

    let user = match crate::db::get_user_by_username(&claims.sub).await {
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
