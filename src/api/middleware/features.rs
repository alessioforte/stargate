//! `404 feature.disabled` for capabilities an operator switched off
//! (`conf::features`). Disabled routes stay mounted so their paths never fall
//! through to the gateway.

use crate::conf::features::{Feature, features};
use crate::err::{ErrorCode, ErrorResponse};
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The answer to a request for a disabled `feature`.
pub fn disabled(feature: Feature) -> ErrorResponse {
    ErrorResponse::new(ErrorCode::FeatureDisabled).with_param("feature", feature.name())
}

/// Route middleware: `404 feature.disabled` unless `feature` is on. Mount it
/// with `from_fn_with_state(feature, require_feature)`.
pub async fn require_feature(
    State(feature): State<Feature>,
    request: Request,
    next: Next,
) -> Response {
    if features().enabled(feature) {
        next.run(request).await
    } else {
        disabled(feature).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_features_answer_not_found() {
        let error = disabled(Feature::SocialGoogle);
        assert_eq!(error.code, ErrorCode::FeatureDisabled);
        assert_eq!(error.into_response().status(), http::StatusCode::NOT_FOUND);
    }
}
