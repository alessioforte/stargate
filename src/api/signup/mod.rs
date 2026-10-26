pub mod complete;
pub mod request;
pub mod verification;

use crate::conf::features::{Feature, SelfServiceRefusal, disabled, features, require_feature};
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::user::signup::{PendingSignupProfile, SignupOrigin};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignupCompleteRequestBody {
    pub token: String,
    pub given_name: String,
    pub family_name: String,
    pub nickname: String,
    pub password: String,
    pub phone_number: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignupVerificationParams {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SignupRequestBody {
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmailVerificationResponse {
    pub email: String,
    pub token: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: Option<String>,
    pub phone_number: Option<String>,
}

/// Self-registration needs `SIGNUP_MODE=open` and an allowed email domain
/// (`email` is canonical). Checked when it starts and again when it
/// completes, so a stricter mode applies to signups already under way.
pub(crate) fn ensure_self_service(email: &str) -> Result<(), ErrorResponse> {
    match features().self_service_signup(email) {
        Ok(()) => Ok(()),
        Err(SelfServiceRefusal::Disabled) => Err(disabled(Feature::Signup)),
        Err(SelfServiceRefusal::DomainNotAllowed(domain)) => {
            Err(ErrorResponse::new(ErrorCode::SignupDomainNotAllowed).with_param("domain", domain))
        }
    }
}

/// Whether a pending signup may still complete: invitations while the route
/// guard lets them (`SIGNUP_MODE` other than `closed`), self-registrations
/// per [`ensure_self_service`].
pub(crate) fn ensure_completable(profile: &PendingSignupProfile) -> Result<(), ErrorResponse> {
    match profile.origin {
        SignupOrigin::Invitation => Ok(()),
        SignupOrigin::SelfService => ensure_self_service(&profile.email),
    }
}

/// `POST /signup` needs `SIGNUP_MODE=open`; completing a pending signup
/// (`GET`/`PUT`) needs a mode other than `closed`.
pub fn router() -> axum::Router {
    use axum::middleware::{from_fn, from_fn_with_state};
    use axum::routing::{get, post};

    let complete = get(verification::get_signup)
        .put(complete::put_signup)
        .layer(from_fn_with_state(Feature::Invitations, require_feature));
    let start =
        post(request::post_signup).layer(from_fn_with_state(Feature::Signup, require_feature));
    axum::Router::new()
        .route("/signup", complete.merge(start))
        .layer(from_fn(crate::web::origin::trusted_origin_middleware))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests run with the default `SIGNUP_MODE=invite`.
    #[test]
    fn only_invitations_complete_outside_open_mode() {
        let mut profile = PendingSignupProfile::self_service("ada@example.com");
        let error = ensure_completable(&profile).unwrap_err();
        assert_eq!(error.code, ErrorCode::FeatureDisabled);

        profile.origin = SignupOrigin::Invitation;
        assert!(ensure_completable(&profile).is_ok());
    }
}
