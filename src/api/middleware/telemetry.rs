use crate::obs::telemetry::HttpRequestTelemetry;
use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};

/// Runs after Axum path matching and before the IAM route's admission/handler.
pub(crate) async fn record_route(req: Request, next: Next) -> Response {
    if let (Some(path), Some(telemetry)) = (
        req.extensions().get::<MatchedPath>(),
        req.extensions().get::<HttpRequestTelemetry>(),
    ) {
        telemetry.record_route(path.as_str());
    }
    next.run(req).await
}
