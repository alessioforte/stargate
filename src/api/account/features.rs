use axum::Json;
use axum::response::IntoResponse;
use http::header::CACHE_CONTROL;
use serde::Serialize;
use utoipa::ToSchema;

/// Sign-in options this server offers, for the hosted apps to show only
/// those. They change only on restart.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeaturesResponse {
    /// Social sign-in providers with credentials: `google`, `github`.
    pub social_providers: Vec<&'static str>,
}

#[utoipa::path(
    get,
    path = "/account/features",
    tags = ["Account"],
    summary = "Available Features",
    description = "Sign-in options this server offers. Public; cacheable for a minute.",
    responses(
        (status = 200, description = "OK", body = FeaturesResponse),
    )
)]
pub async fn get_features() -> impl IntoResponse {
    let features = crate::etc::features::features();
    (
        [(CACHE_CONTROL, "public, max-age=60")],
        Json(FeaturesResponse {
            social_providers: features.social_providers(),
        }),
    )
}
