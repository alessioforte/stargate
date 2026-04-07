use crate::err::{ErrorResponse, HttpError};
use actix_web::{HttpResponse, post, web};
use serde::Serialize;

pub mod github;
pub mod google;

#[derive(Serialize)]
struct StateResponse {
    state: String,
}

#[post("/state")]
async fn generate_state() -> Result<HttpResponse, ErrorResponse> {
    let state = crate::act::oauth_state::create_oauth_state()
        .await
        .map_err(|e| {
            tracing::error!("Failed to create OAuth state: {}", e);
            ErrorResponse::from(HttpError::InternalServerError(
                "failed to generate oauth state".to_string(),
            ))
        })?;

    Ok(HttpResponse::Ok().json(StateResponse { state }))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/oauth")
        .service(generate_state)
        .service(google::routes())
        .service(github::routes())
}
