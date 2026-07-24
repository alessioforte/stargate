use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::internal_context;
use axum::Json;
use axum::response::IntoResponse;
use http::HeaderMap;
use http::header::CACHE_CONTROL;

#[utoipa::path(
    get,
    path = "/.well-known/stargate-context-jwks.json",
    tags = ["Well Known"],
    responses(
        (status = 200, description = "Internal-context public JSON Web Key Set"),
        (status = 503, description = "Internal-context signing is not configured", body = ErrorResponse)
    )
)]
pub async fn get_internal_context_jwks() -> Result<impl IntoResponse, ErrorResponse> {
    let runtime = internal_context::runtime()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::InternalContextUnavailable))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        CACHE_CONTROL,
        format!("public, max-age={}", runtime.cache_max_age_secs())
            .parse()
            .map_err(ErrorResponse::internal)?,
    );
    Ok((headers, Json(runtime.public_jwks().clone())))
}
