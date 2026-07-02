use super::{AuthResponse, UserCredentials};
use crate::act::login_guard;
use crate::api::account::otp::{LoginMfaRequiredResponse, maybe_start_login_mfa};
use crate::api::account::session::issue_user_session;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::ext::RequestExt;
use crate::etc::reqctx::take_audit_context_from;
use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::response::Response;
use db::ent::CredentialType;

#[utoipa::path(
    post,
    path = "/account/login",
    tags = ["Account"],
    summary = "User Login",
    description = "Authenticate a user using their username and password. On successful authentication, an access token and a refresh token are issued.",
    request_body = UserCredentials,
    responses(
        (status = 200, description = "OK", body = AuthResponse),
        (status = 202, description = "MFA Required", body = LoginMfaRequiredResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid Credentials", body = ErrorResponse),
        (status = 503, description = "Service Unavailable", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn post_login(mut req: Request) -> Result<Response, ErrorResponse> {
    let mut audit_ctx = take_audit_context_from(req.extensions_mut());
    let client_ip = req.get_client_ip();

    let Json(credentials) = Json::<UserCredentials>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;

    let login_identifier = credentials.username.trim();

    let user = crate::db::get_user_by_username(login_identifier)
        .await
        .map_err(ErrorResponse::internal)?;

    let throttle = match user.as_ref() {
        Some(u) => login_guard::LoginThrottle::for_user(&client_ip, &u.id),
        None => login_guard::LoginThrottle::for_identifier(&client_ip, login_identifier),
    };

    let allowed = login_guard::check_login_allowed(&throttle)
        .await
        .map_err(login_guard_unavailable)?;

    if !allowed {
        let mut err = ErrorResponse::from(HttpError::TooManyRequests(
            "too many failed login attempts, try again later".to_string(),
        ));
        err.insert_header("Retry-After", &login_guard::lockout_seconds().to_string());
        return Err(err);
    }

    let Some(user) = user else {
        // Verify against a dummy hash so the unknown-user path takes about as
        // long as the valid-user path, preventing username enumeration via
        // response timing.
        let _ = crate::etc::pw::verify_password(
            credentials.password.clone(),
            crate::etc::pw::dummy_hash(),
        )
        .await;
        login_guard::record_failed_attempt(&throttle)
            .await
            .map_err(login_guard_unavailable)?;
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid credentials".to_string(),
        )));
    };

    let user_credential = crate::db::get_credential(&user.id, CredentialType::Password)
        .await
        .map_err(ErrorResponse::internal)?;

    let Some(user_credential) = user_credential else {
        let _ = crate::etc::pw::verify_password(
            credentials.password.clone(),
            crate::etc::pw::dummy_hash(),
        )
        .await;
        login_guard::record_failed_attempt(&throttle)
            .await
            .map_err(login_guard_unavailable)?;
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid credentials".to_string(),
        )));
    };

    let password_ok = crate::etc::pw::verify_password(
        credentials.password.clone(),
        user_credential.value.clone(),
    )
    .await;
    if !password_ok {
        login_guard::record_failed_attempt(&throttle)
            .await
            .map_err(login_guard_unavailable)?;
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid credentials".to_string(),
        )));
    }

    if let Err(error) = login_guard::clear_subject_attempts(&throttle).await {
        tracing::warn!(
            "Failed to clear login attempts after successful login: {}",
            error
        );
    }

    audit_ctx = audit_ctx.with_actor(db::ent::ActorType::User, Some(user.id.clone()));
    // Note: no downstream consumer in the handler chain (Stage D wires middleware);
    // drop audit_ctx after enrichment.
    let _ = audit_ctx;

    let auth_time = chrono::Utc::now().timestamp() as usize;

    if let Some(response) = maybe_start_login_mfa(&user, &client_ip, auth_time).await? {
        return Ok(response);
    }

    issue_user_session(user, auth_time).await
}

fn login_guard_unavailable(error: store::StoreError) -> ErrorResponse {
    tracing::error!("Login guard unavailable: {}", error);
    ErrorResponse::from(HttpError::ServiceUnavailable(
        "login temporarily unavailable".to_string(),
    ))
}
