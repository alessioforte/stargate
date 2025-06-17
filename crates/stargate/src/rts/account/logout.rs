use crate::act::get_token_from_request;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::jwt::jwt_config;
use actix_web::{delete, web, HttpRequest, HttpResponse};
// use db::ent::Token;
// use jwt::jwt_config;

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
    match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )))
        }
    };

    // DEPRECATED: Token deletion is not recommended in this context.
    // let response = Token::delete(&claims.sub_id).await;
    // if response.is_err() {
    //     return Err(ErrorResponse::from(HttpError::InternalServerError(
    //         "Could not delete token".to_string(),
    //     )));
    // }

    // TODO: Implement proper logout logic, such as invalidating the session or token.

    Ok(HttpResponse::Ok().json(web::Json("Logout User")))
}
