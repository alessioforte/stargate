use super::{AuthResponse, UserCredentials};
use crate::act::login_guard;
use crate::api::account::credentials::expired::maybe_password_expired_response;
use crate::api::account::otp::{LoginMfaRequiredResponse, maybe_start_login_mfa};
use crate::api::account::session::issue_user_session;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::ext::RequestExt;
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
        (status = 202, description = "MFA Required (body: LoginMfaRequiredResponse) or Password Expired (body: PasswordExpiredResponse, carries a change-password resetToken)", body = LoginMfaRequiredResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid Credentials", body = ErrorResponse),
        (status = 503, description = "Service Unavailable", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn post_login(req: Request) -> Result<Response, ErrorResponse> {
    let client_ip = req.get_client_ip();

    let Json(credentials) = Json::<UserCredentials>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;

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
        let mut err = ErrorResponse::new(ErrorCode::AuthLoginRateLimited);
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
        return Err(ErrorResponse::new(ErrorCode::AuthInvalidCredentials));
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
        return Err(ErrorResponse::new(ErrorCode::AuthInvalidCredentials));
    };

    let password_check =
        crate::etc::pw::check_password(credentials.password.clone(), user_credential.value.clone())
            .await;
    if !password_check.valid {
        login_guard::record_failed_attempt(&throttle)
            .await
            .map_err(login_guard_unavailable)?;
        return Err(ErrorResponse::new(ErrorCode::AuthInvalidCredentials));
    }

    if let Err(error) = login_guard::clear_subject_attempts(&throttle).await {
        tracing::warn!(
            "Failed to clear login attempts after successful login: {}",
            error
        );
    }

    if password_check.needs_rehash {
        spawn_credential_rehash(
            user.id.clone(),
            credentials.password.clone(),
            user_credential.value.clone(),
        );
    }

    let auth_time = chrono::Utc::now().timestamp() as usize;

    if let Some(response) =
        maybe_start_login_mfa(&user, &client_ip, auth_time, credentials.org_id.as_deref()).await?
    {
        return Ok(response);
    }

    // For MFA users the expiry check runs after MFA verification instead, so
    // an expired password alone never yields a reset token.
    if let Some(response) = maybe_password_expired_response(&user, Some(&user_credential)).await? {
        return Ok(response);
    }

    issue_user_session(user, auth_time, credentials.org_id.as_deref()).await
}

/// Re-encode a verified password in the background when its stored hash
/// predates the current Argon2 configuration (cost change or new pepper).
/// Best-effort: failures only log, and the value-guarded update makes
/// concurrent logins race-safe.
fn spawn_credential_rehash(user_id: String, password: String, old_hash: String) {
    tokio::spawn(async move {
        let Some(new_hash) = crate::etc::pw::hash_password(password).await else {
            tracing::warn!("credential rehash skipped: hashing failed");
            return;
        };
        let audit_context = db::ent::TrustedAuditContext::background(
            db::ent::TrustedAuditBoundary::application(),
            db::ent::TrustedBackgroundActor::system(None),
        );
        match crate::db::rehash_credential(&user_id, &old_hash, &new_hash, audit_context).await {
            Ok(true) => tracing::info!("credential hash upgraded for user {user_id}"),
            Ok(false) => {
                tracing::debug!("credential rehash skipped: hash changed concurrently")
            }
            Err(error) => tracing::warn!("credential rehash failed: {error}"),
        }
    });
}

fn login_guard_unavailable(error: store::StoreError) -> ErrorResponse {
    tracing::error!("Login guard unavailable: {}", error);
    ErrorResponse::new(ErrorCode::AuthLoginUnavailable)
}
