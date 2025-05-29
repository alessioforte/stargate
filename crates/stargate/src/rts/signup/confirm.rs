use super::SignupConfirmParams;
use crate::err::{ErrorResponse, HttpError};
use actix_web::{get, web, HttpResponse};
use db::ent::signup::Signup;

#[utoipa::path(
    context_path = "/signup",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("")]
pub async fn handler(
    query: web::Query<SignupConfirmParams>,
) -> Result<HttpResponse, ErrorResponse> {
    let query = query.into_inner();
    let token = query.token.clone();
    let response = Signup::get_by_uuid(&token).await;
    match response {
        Ok(signup) => {
            if signup.is_none() {
                return Err(ErrorResponse::from(HttpError::NotFound(
                    "Signup not found".to_string(),
                )));
            }
            Ok(HttpResponse::Ok().json(web::Json(signup)))
        }
        Err(_) => Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not get signup request".to_string(),
        ))),
    }
}
