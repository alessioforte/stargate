use crate::act::get_token_from_request;
use crate::ent::user::{Profile, User};
use crate::err::{ErrorResponse, HttpError};
use crate::pks::jwt::jwt_config;
use actix_web::{get, web, HttpRequest, HttpResponse};

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

    let user = User::get(&claims.sub_id).await.unwrap();
    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();
    Ok(HttpResponse::Ok().json(web::Json(Profile {
        id: user.id.clone(),
        name: user.name.clone(),
        email: user.email.clone(),
        nickname: user.nickname.clone(),
        picture: user.picture.clone(),
        phone_number: user.phone_number.clone(),
    })))
}
