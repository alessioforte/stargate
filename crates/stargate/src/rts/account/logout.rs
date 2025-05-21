use crate::act::get_token_from_request;
use crate::ent::token::Token;
use crate::err::{ErrorResponse, HttpError};
use actix_web::{delete, web, HttpRequest, HttpResponse};
use jwt::jwt_config;

#[utoipa::path(
    context_path = "/account",
    path = "/logout",
    responses(
        (status = 200, description = "OK")
    )
)]
#[delete("/logout")]
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

    let response = Token::delete(&claims.sub_id).await;
    if response.is_err() {
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not delete token".to_string(),
        )));
    }

    Ok(HttpResponse::Ok().json(web::Json("Logout User")))
}
