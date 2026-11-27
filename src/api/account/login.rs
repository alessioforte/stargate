use super::{AuthResponse, UserCredentials};
use crate::api::account::credentials::expired::maybe_password_expired_response;
use crate::api::account::otp::{LoginMfaRequiredResponse, maybe_start_login_mfa};
use crate::api::account::session::issue_password_session;
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::auth::login_guard;
use crate::web::request::RequestExt;
use axum::extract::Request;
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
    let secure = req.is_https();

    let credentials = crate::web::body::json_body::<UserCredentials>(req).await?;

    let (user, user_credential) =
        verify_login_password(&client_ip, &credentials.username, &credentials.password).await?;

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

    issue_password_session(
        user,
        auth_time,
        credentials.org_id.as_deref(),
        vec![oidc::authentication::AMR_PASSWORD.to_string()],
        &client_ip,
        secure,
    )
    .await
}

/// Check a username/password pair under the login guard: throttling,
/// constant-time unknown-user handling, failed-attempt accounting, and a
/// background rehash when the stored hash is outdated. Shared by the JSON
/// login API and the OpenID Provider login page.
pub(crate) async fn verify_login_password(
    client_ip: &str,
    username: &str,
    password: &str,
) -> Result<(db::ent::User, db::ent::Credential), ErrorResponse> {
    let login_identifier = username.trim();

    let user = crate::data::db::get_user_by_login(login_identifier)
        .await
        .map_err(ErrorResponse::internal)?;

    let throttle = match user.as_ref() {
        Some(u) => login_guard::LoginThrottle::for_user(client_ip, &u.id),
        // Identifiers match case-insensitively, so casing never opens a
        // fresh bucket for an unknown one.
        None => {
            login_guard::LoginThrottle::for_identifier(client_ip, &login_identifier.to_lowercase())
        }
    };

    let attempt = match throttle.reserve().await.map_err(login_guard_unavailable)? {
        login_guard::Admission::Allowed(attempt) => attempt,
        login_guard::Admission::Limited { retry_after } => {
            let mut error = ErrorResponse::new(ErrorCode::AuthLoginRateLimited);
            error.insert_header("Retry-After", &retry_after.to_string());
            return Err(error);
        }
    };

    let checked = async {
        let credential = match user.as_ref() {
            Some(user) => crate::data::db::get_credential(&user.id, CredentialType::Password)
                .await
                .map_err(ErrorResponse::internal)?,
            None => None,
        };
        // Use the same Argon2 path for unknown users and users without a password.
        let hash = credential
            .as_ref()
            .map(|credential| credential.value.clone())
            .unwrap_or_else(crate::iam::pwd::hash::dummy_hash);
        let check = crate::iam::pwd::hash::check_password(password.to_string(), hash).await?;
        Ok::<_, ErrorResponse>((credential, check))
    }
    .await;
    let (credential, password_check) = match checked {
        Ok(checked) => checked,
        Err(error) => {
            attempt.refund().await;
            return Err(error);
        }
    };

    let (Some(user), Some(user_credential), true) = (user, credential, password_check.valid) else {
        return Err(ErrorResponse::new(ErrorCode::AuthInvalidCredentials));
    };
    attempt.refund().await;

    if password_check.needs_rehash {
        spawn_credential_rehash(
            user.id.clone(),
            password.to_string(),
            user_credential.value.clone(),
        );
    }

    Ok((user, user_credential))
}

/// Re-encode a verified password in the background when its stored hash
/// predates the current Argon2 configuration (cost change or new pepper).
/// Best-effort: failures only log, and the value-guarded update makes
/// concurrent logins race-safe.
fn spawn_credential_rehash(user_id: String, password: String, old_hash: String) {
    tokio::spawn(async move {
        let new_hash = match crate::iam::pwd::hash::try_rehash_password(password).await {
            Ok(Some(hash)) => hash,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(%error, "credential rehash skipped");
                return;
            }
        };
        let audit_context = db::ent::TrustedAuditContext::background(
            db::ent::TrustedAuditBoundary::application(),
            db::ent::TrustedBackgroundActor::system(None),
        );
        match crate::data::db::rehash_credential(&user_id, &old_hash, &new_hash, audit_context)
            .await
        {
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

#[cfg(all(test, feature = "sqlite", feature = "memory"))]
mod tests {
    use super::*;
    use crate::iam::pwd::hash::with_exhausted_budget;
    use ::db::ent::{Profile, TrustedAuditContext};
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn overload_is_identical_for_known_missing_and_passwordless_users_without_failed_attempts()
     {
        crate::data::db::with_test_database(async {
            let suffix = ulid::Ulid::generate().to_string();
            let mut usernames = vec![format!("missing-{suffix}")];
            for (nickname, kind) in [
                (format!("known-{suffix}"), CredentialType::Password),
                (format!("federated-{suffix}"), CredentialType::Oauth),
            ] {
                crate::data::db::create_user(
                    Profile::new(format!("{nickname}@example.com"), nickname.clone()),
                    kind,
                    "irrelevant",
                    None,
                    TrustedAuditContext::admin_control_plane(
                        db::ent::TrustedAdminActor::admin("01JZ000000000000000000000A"),
                        db::ent::TrustedAuditRequest::from_http(
                            ulid::Ulid::generate().to_string(),
                            None,
                            None,
                            None,
                        ),
                    ),
                )
                .await
                .unwrap();
                usernames.push(nickname);
            }
            let ip = "203.0.113.248";
            for username in usernames {
                let error = with_exhausted_budget(verify_login_password(ip, &username, "password"))
                    .await
                    .err()
                    .unwrap();
                assert_eq!(error.code, ErrorCode::IamOverloaded);
                let response = error.into_response();
                assert_eq!(response.status(), 503);
                assert_eq!(response.headers().get_all("retry-after").iter().count(), 1);
                assert_eq!(response.headers()["retry-after"], "10");
            }
            // The overloads left enough room for the entire source allowance.
            for n in 0..20 {
                let username = format!("probe-{n}-{suffix}");
                let throttle = login_guard::LoginThrottle::for_identifier(ip, &username);
                assert!(matches!(
                    throttle.reserve().await.unwrap(),
                    login_guard::Admission::Allowed(_)
                ));
            }
        })
        .await;
    }
}
