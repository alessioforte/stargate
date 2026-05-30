use super::session::issue_user_session;
use crate::err::{ErrorResponse, HttpError};
use crate::etc::ext::RequestExt;
use crate::etc::jwt::jwt_config;
use crate::etc::sub::{Subject, SubjectType};
use crate::fun::format_name;
use axum::Json;
use axum::extract::{FromRequest, Path, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use otp::{MessageOtpConfig, MessageOtpRecord, MessageOtpVerification, issue_email_otp};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use smtp::Smtp;
use std::time::{SystemTime, UNIX_EPOCH};
use store::{AtomicStore, Store};
use utoipa::ToSchema;

const PASSWORDLESS_EMAIL_KEY_PREFIX: &str = "account:login:otp:email";
const MFA_PENDING_LOGIN_KEY_PREFIX: &str = "account:mfa:pending-login";
const MFA_CHALLENGE_KEY_PREFIX: &str = "account:mfa:challenge";
const MFA_VERIFIED_KEY_PREFIX: &str = "account:mfa:verified";
const OTP_REQUEST_KEY_PREFIX: &str = "account:otp:request";
const DEFAULT_EMAIL_OTP_LENGTH: usize = 6;
const DEFAULT_EMAIL_OTP_TTL_SECS: u64 = 300;
const DEFAULT_EMAIL_OTP_MAX_ATTEMPTS: u8 = 5;
const DEFAULT_PASSWORDLESS_PURPOSE: &str = "passwordless_login";
const DEFAULT_MFA_PURPOSE: &str = "login";
const DEFAULT_MFA_STEP_UP_TTL_SECS: u64 = 300;
const OTP_REQUEST_LOCKOUT_SECS: u64 = 300;
const MAX_OTP_SOURCE_REQUESTS: u32 = 30;
const MAX_OTP_SUBJECT_REQUESTS: u32 = 5;
const ISSUER: &str = "Stargate";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OtpMethod {
    Email,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MfaMode {
    Off,
    Optional,
    Required,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordlessEmailOtpRequestBody {
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordlessEmailOtpVerifyRequestBody {
    pub challenge_id: String,
    pub code: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OtpChallengeResponse {
    pub challenge_id: String,
    pub method: OtpMethod,
    pub expires_at_unix: u64,
    pub ttl_seconds: u64,
    pub max_attempts: u8,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginMfaRequiredResponse {
    pub mfa_required: bool,
    pub challenge_id: String,
    pub method: OtpMethod,
    pub expires_at_unix: u64,
    pub ttl_seconds: u64,
    pub max_attempts: u8,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaMethodsResponse {
    pub mode: MfaMode,
    pub required: bool,
    pub methods: Vec<OtpMethod>,
    pub preferred_method: Option<OtpMethod>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaChallengeRequestBody {
    pub method: OtpMethod,
    pub purpose: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MfaChallengeVerifyRequestBody {
    pub code: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MfaVerificationResponse {
    pub method: OtpMethod,
    pub purpose: String,
    pub verified_at_unix: u64,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredOtpChallenge {
    user_id: String,
    method: OtpMethod,
    purpose: String,
    record: MessageOtpRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PendingMfaLogin {
    user_id: String,
    method: OtpMethod,
    challenge: StoredOtpChallenge,
    auth_time: usize,
    created_at_unix: u64,
    expires_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MfaVerificationMarker {
    user_id: String,
    method: OtpMethod,
    purpose: String,
    verified_at_unix: u64,
    expires_at_unix: u64,
}

#[derive(Debug, Clone)]
struct AuthenticatedUser {
    user: db::ent::User,
    sid: String,
}

#[derive(Debug, Clone)]
struct MfaPolicy {
    mode: MfaMode,
    required: bool,
    methods: Vec<OtpMethod>,
    preferred_method: Option<OtpMethod>,
}

#[utoipa::path(
    post,
    path = "/account/login/otp/email",
    tags = ["Account"],
    summary = "Request Passwordless Email OTP",
    description = "Start a passwordless login by sending an email one-time password. The response is generic even when the account does not exist.",
    request_body = PasswordlessEmailOtpRequestBody,
    responses(
        (status = 200, description = "OK", body = OtpChallengeResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 503, description = "Service Unavailable", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn post_login_email_otp(
    req: Request,
) -> Result<Json<OtpChallengeResponse>, ErrorResponse> {
    let client_ip = req.get_client_ip();
    let Json(body) = Json::<PasswordlessEmailOtpRequestBody>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;
    let email = normalize_email(&body.email)?;
    check_and_record_otp_request(
        &client_ip,
        &format!("passwordless:{}", email.to_ascii_lowercase()),
    )
    .await?;

    let now = current_unix_time()?;
    let config = email_otp_config()?;
    let pepper = email_otp_pepper()?;
    let fake_response = fake_challenge_response(now, &config)?;
    let Some(user) = crate::db::get_user_by_username(&email)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Ok(Json(fake_response));
    };

    if !user.email.eq_ignore_ascii_case(&email) {
        return Ok(Json(fake_response));
    }

    let issued = issue_email_challenge(
        &user,
        DEFAULT_PASSWORDLESS_PURPOSE,
        now,
        config,
        pepper.as_bytes(),
    )?;
    let challenge = StoredOtpChallenge {
        user_id: user.id.clone(),
        method: OtpMethod::Email,
        purpose: DEFAULT_PASSWORDLESS_PURPOSE.to_string(),
        record: issued.record.clone(),
    };
    let response = challenge_response(&challenge);
    let key = passwordless_email_key(&response.challenge_id);
    let store = crate::etc::store::use_store();
    store
        .set(
            &key,
            &challenge,
            Some(ttl_until(challenge.record.expires_at_unix, now)),
        )
        .await
        .map_err(ErrorResponse::internal)?;

    if let Err(error) = send_email_otp(&user, &issued) {
        let _ = store.delete(&key).await;
        return Err(ErrorResponse::internal(error));
    }

    Ok(Json(response))
}

#[utoipa::path(
    put,
    path = "/account/login/otp/email",
    tags = ["Account"],
    summary = "Verify Passwordless Email OTP",
    description = "Verify a passwordless email OTP challenge and issue normal session tokens.",
    request_body = PasswordlessEmailOtpVerifyRequestBody,
    responses(
        (status = 200, description = "OK", body = super::AuthResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn put_login_email_otp(req: Request) -> Result<Response, ErrorResponse> {
    let Json(body) = Json::<PasswordlessEmailOtpVerifyRequestBody>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;
    validate_challenge_id(&body.challenge_id)?;
    validate_code(&body.code)?;

    let pepper = email_otp_pepper()?;
    let now = current_unix_time()?;
    let key = passwordless_email_key(&body.challenge_id);
    let result = verify_stored_challenge(&key, &body.code, pepper.as_bytes(), now).await?;

    let challenge = match result {
        VerifiedChallenge::Valid(challenge) => challenge,
        VerifiedChallenge::Rejected(result) => return Err(verification_error(result)),
    };

    let user = crate::db::get_user_by_id(&challenge.user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(invalid_or_expired_code)?;
    let auth_time = chrono::Utc::now().timestamp() as usize;
    issue_user_session(user, auth_time).await
}

#[utoipa::path(
    put,
    path = "/account/login/mfa/challenges/{challenge_id}",
    tags = ["Account", "MFA"],
    summary = "Verify Login MFA Challenge",
    description = "Verify a pending MFA challenge created during password login and issue normal session tokens.",
    params(("challenge_id" = String, Path, description = "Pending login MFA challenge ID")),
    request_body = MfaChallengeVerifyRequestBody,
    responses(
        (status = 200, description = "OK", body = super::AuthResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 409, description = "Conflict", body = ErrorResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn put_login_mfa_challenge(
    Path(challenge_id): Path<String>,
    req: Request,
) -> Result<Response, ErrorResponse> {
    validate_challenge_id(&challenge_id)?;
    let Json(body) = Json::<MfaChallengeVerifyRequestBody>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;
    validate_code(&body.code)?;

    let pepper = email_otp_pepper()?;
    let now = current_unix_time()?;
    let key = pending_login_key(&challenge_id);
    let result = verify_pending_login(&key, &body.code, pepper.as_bytes(), now).await?;
    let pending = match result {
        VerifiedPendingLogin::Valid(pending) => pending,
        VerifiedPendingLogin::Rejected(result) => return Err(verification_error(result)),
    };

    let user = crate::db::get_user_by_id(&pending.user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(invalid_or_expired_code)?;

    issue_user_session(user, pending.auth_time).await
}

pub(crate) async fn maybe_start_login_mfa(
    user: &db::ent::User,
    client_ip: &str,
    auth_time: usize,
) -> Result<Option<Response>, ErrorResponse> {
    let policy = mfa_policy_for_user(user).await?;
    if !policy.required {
        return Ok(None);
    }

    let method = policy.preferred_method.ok_or_else(|| {
        tracing::error!(user_id = %user.id, "MFA is required but no method is available");
        ErrorResponse::from(HttpError::ServiceUnavailable(
            "MFA temporarily unavailable".to_string(),
        ))
    })?;
    ensure_method_allowed(&policy, method)?;
    check_and_record_otp_request(client_ip, &format!("mfa-login:{}", user.id)).await?;

    match method {
        OtpMethod::Email => {
            let now = current_unix_time()?;
            let pepper = email_otp_pepper()?;
            let issued = issue_email_challenge(
                user,
                DEFAULT_MFA_PURPOSE,
                now,
                email_otp_config()?,
                pepper.as_bytes(),
            )?;
            let challenge = StoredOtpChallenge {
                user_id: user.id.clone(),
                method,
                purpose: DEFAULT_MFA_PURPOSE.to_string(),
                record: issued.record.clone(),
            };
            let response = challenge_response(&challenge);
            let pending = PendingMfaLogin {
                user_id: user.id.clone(),
                method,
                challenge,
                auth_time,
                created_at_unix: now,
                expires_at_unix: response.expires_at_unix,
            };
            let key = pending_login_key(&response.challenge_id);
            let store = crate::etc::store::use_store();
            store
                .set(
                    &key,
                    &pending,
                    Some(ttl_until(pending.expires_at_unix, now)),
                )
                .await
                .map_err(ErrorResponse::internal)?;

            if let Err(error) = send_email_otp(user, &issued) {
                let _ = store.delete(&key).await;
                return Err(ErrorResponse::internal(error));
            }

            let mut response = Json(LoginMfaRequiredResponse {
                mfa_required: true,
                challenge_id: response.challenge_id,
                method: response.method,
                expires_at_unix: response.expires_at_unix,
                ttl_seconds: response.ttl_seconds,
                max_attempts: response.max_attempts,
            })
            .into_response();
            *response.status_mut() = StatusCode::ACCEPTED;
            Ok(Some(response))
        }
    }
}

#[utoipa::path(
    get,
    path = "/account/mfa/methods",
    tags = ["Account", "MFA"],
    summary = "List MFA Methods",
    description = "Return the effective MFA policy and available methods for the authenticated user.",
    responses(
        (status = 200, description = "OK", body = MfaMethodsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn get_mfa_methods(req: Request) -> Result<Json<MfaMethodsResponse>, ErrorResponse> {
    let auth = authenticated_user(req.get_token()).await?;
    let policy = mfa_policy_for_user(&auth.user).await?;

    Ok(Json(MfaMethodsResponse {
        mode: policy.mode,
        required: policy.required,
        methods: policy.methods,
        preferred_method: policy.preferred_method,
    }))
}

#[utoipa::path(
    post,
    path = "/account/mfa/challenges",
    tags = ["Account", "MFA"],
    summary = "Request MFA Challenge",
    description = "Create an MFA challenge using one of the authenticated user's available methods. Email is the first supported method.",
    request_body = MfaChallengeRequestBody,
    responses(
        (status = 200, description = "OK", body = OtpChallengeResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 503, description = "Service Unavailable", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn post_mfa_challenge(req: Request) -> Result<Json<OtpChallengeResponse>, ErrorResponse> {
    let client_ip = req.get_client_ip();
    let token = req.get_token();
    let auth = authenticated_user(token).await?;
    let Json(body) = Json::<MfaChallengeRequestBody>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;
    let purpose = normalize_purpose(body.purpose.as_deref(), DEFAULT_MFA_PURPOSE)?;
    let policy = mfa_policy_for_user(&auth.user).await?;
    ensure_method_allowed(&policy, body.method)?;
    check_and_record_otp_request(&client_ip, &format!("mfa:{}:{}", auth.user.id, purpose)).await?;

    match body.method {
        OtpMethod::Email => {
            let now = current_unix_time()?;
            let pepper = email_otp_pepper()?;
            let issued = issue_email_challenge(
                &auth.user,
                &purpose,
                now,
                email_otp_config()?,
                pepper.as_bytes(),
            )?;
            let challenge = StoredOtpChallenge {
                user_id: auth.user.id.clone(),
                method: OtpMethod::Email,
                purpose,
                record: issued.record.clone(),
            };
            let response = challenge_response(&challenge);
            let key = mfa_challenge_key(&auth.user.id, &response.challenge_id);
            let store = crate::etc::store::use_store();
            store
                .set(
                    &key,
                    &challenge,
                    Some(ttl_until(challenge.record.expires_at_unix, now)),
                )
                .await
                .map_err(ErrorResponse::internal)?;

            if let Err(error) = send_email_otp(&auth.user, &issued) {
                let _ = store.delete(&key).await;
                return Err(ErrorResponse::internal(error));
            }

            Ok(Json(response))
        }
    }
}

#[utoipa::path(
    put,
    path = "/account/mfa/challenges/{challenge_id}",
    tags = ["Account", "MFA"],
    summary = "Verify MFA Challenge",
    description = "Verify an MFA challenge and create a short-lived step-up verification marker for its purpose.",
    params(("challenge_id" = String, Path, description = "MFA challenge ID")),
    request_body = MfaChallengeVerifyRequestBody,
    responses(
        (status = 200, description = "OK", body = MfaVerificationResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Not Found", body = ErrorResponse),
        (status = 409, description = "Conflict", body = ErrorResponse),
        (status = 429, description = "Too Many Requests", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
#[axum::debug_handler]
pub async fn put_mfa_challenge(
    Path(challenge_id): Path<String>,
    req: Request,
) -> Result<Json<MfaVerificationResponse>, ErrorResponse> {
    validate_challenge_id(&challenge_id)?;
    let token = req.get_token();
    let auth = authenticated_user(token).await?;
    let Json(body) = Json::<MfaChallengeVerifyRequestBody>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;
    validate_code(&body.code)?;

    let pepper = email_otp_pepper()?;
    let now = current_unix_time()?;
    let key = mfa_challenge_key(&auth.user.id, &challenge_id);
    let result = verify_stored_challenge(&key, &body.code, pepper.as_bytes(), now).await?;
    let challenge = match result {
        VerifiedChallenge::Valid(challenge) => challenge,
        VerifiedChallenge::Rejected(result) => return Err(verification_error(result)),
    };

    if challenge.user_id != auth.user.id {
        return Err(invalid_or_expired_code());
    }

    let marker =
        store_mfa_verified_marker(&auth, challenge.method, &challenge.purpose, now).await?;
    Ok(Json(MfaVerificationResponse {
        method: marker.method,
        purpose: marker.purpose,
        verified_at_unix: marker.verified_at_unix,
        expires_at_unix: marker.expires_at_unix,
    }))
}

enum VerifiedChallenge {
    Valid(StoredOtpChallenge),
    Rejected(MessageOtpVerification),
}

enum VerifiedPendingLogin {
    Valid(Box<PendingMfaLogin>),
    Rejected(MessageOtpVerification),
}

async fn authenticated_user(token: Option<String>) -> Result<AuthenticatedUser, ErrorResponse> {
    let token = token.ok_or_else(|| {
        ErrorResponse::from(HttpError::Unauthorized("Token not found".to_string()))
    })?;

    let claims = jwt_config()
        .validate_session_access_token(&token)
        .map_err(|_| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    if crate::act::token_revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid Token".to_string(),
        )));
    }

    let sid = claims
        .sid
        .clone()
        .ok_or_else(|| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;
    let subject = crate::etc::store::use_store()
        .get::<Subject>(&sid)
        .await
        .map_err(|_| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?
        .ok_or_else(|| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    if subject.sub_type != SubjectType::User {
        return Err(ErrorResponse::from(HttpError::Forbidden(
            "MFA is only available for user sessions".to_string(),
        )));
    }

    if claims.sub_id.as_deref() != Some(&subject.id) {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid Token".to_string(),
        )));
    }

    let user = crate::db::get_user_by_id(&subject.id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::from(HttpError::Unauthorized("Invalid Token".to_string())))?;

    Ok(AuthenticatedUser { user, sid })
}

fn issue_email_challenge(
    user: &db::ent::User,
    purpose: &str,
    now: u64,
    config: MessageOtpConfig,
    pepper: &[u8],
) -> Result<otp::IssuedMessageOtp, ErrorResponse> {
    issue_email_otp(user.email.clone(), purpose, pepper, now, config).map_err(email_otp_internal)
}

async fn verify_pending_login(
    key: &str,
    code: &str,
    pepper: &[u8],
    now: u64,
) -> Result<VerifiedPendingLogin, ErrorResponse> {
    let store = crate::etc::store::use_store();

    for _ in 0..3 {
        let Some(current) = store
            .get::<PendingMfaLogin>(key)
            .await
            .map_err(ErrorResponse::internal)?
        else {
            return Ok(VerifiedPendingLogin::Rejected(
                MessageOtpVerification::Expired,
            ));
        };

        if current.expires_at_unix <= now || current.challenge.record.is_expired(now) {
            let _ = store.delete(key).await;
            return Ok(VerifiedPendingLogin::Rejected(
                MessageOtpVerification::Expired,
            ));
        }

        if current.challenge.record.consumed {
            let _ = store.delete(key).await;
            return Ok(VerifiedPendingLogin::Rejected(
                MessageOtpVerification::AlreadyUsed,
            ));
        }

        let mut next = current.clone();
        let result = next.challenge.record.verify(code, pepper, now);
        let ttl = ttl_until(next.expires_at_unix, now);
        let swapped = store
            .compare_and_swap(key, &current, &next, Some(ttl))
            .await
            .map_err(ErrorResponse::internal)?;

        if !swapped {
            continue;
        }

        if matches!(
            result,
            MessageOtpVerification::Valid
                | MessageOtpVerification::Expired
                | MessageOtpVerification::AttemptsExceeded
                | MessageOtpVerification::AlreadyUsed
        ) && let Err(error) = store.delete(key).await
        {
            tracing::warn!(key, %error, "failed to delete completed pending MFA login");
        }

        return if result == MessageOtpVerification::Valid {
            Ok(VerifiedPendingLogin::Valid(Box::new(next)))
        } else {
            Ok(VerifiedPendingLogin::Rejected(result))
        };
    }

    Err(ErrorResponse::from(HttpError::Conflict(
        "OTP challenge changed, retry verification".to_string(),
    )))
}

async fn verify_stored_challenge(
    key: &str,
    code: &str,
    pepper: &[u8],
    now: u64,
) -> Result<VerifiedChallenge, ErrorResponse> {
    let store = crate::etc::store::use_store();

    for _ in 0..3 {
        let Some(current) = store
            .get::<StoredOtpChallenge>(key)
            .await
            .map_err(ErrorResponse::internal)?
        else {
            return Ok(VerifiedChallenge::Rejected(MessageOtpVerification::Expired));
        };

        if current.record.is_expired(now) {
            let _ = store.delete(key).await;
            return Ok(VerifiedChallenge::Rejected(MessageOtpVerification::Expired));
        }

        if current.record.consumed {
            let _ = store.delete(key).await;
            return Ok(VerifiedChallenge::Rejected(
                MessageOtpVerification::AlreadyUsed,
            ));
        }

        let mut next = current.clone();
        let result = next.record.verify(code, pepper, now);
        let ttl = ttl_until(next.record.expires_at_unix, now);
        let swapped = store
            .compare_and_swap(key, &current, &next, Some(ttl))
            .await
            .map_err(ErrorResponse::internal)?;

        if !swapped {
            continue;
        }

        if matches!(
            result,
            MessageOtpVerification::Valid
                | MessageOtpVerification::Expired
                | MessageOtpVerification::AttemptsExceeded
                | MessageOtpVerification::AlreadyUsed
        ) && let Err(error) = store.delete(key).await
        {
            tracing::warn!(key, %error, "failed to delete completed OTP challenge");
        }

        return if result == MessageOtpVerification::Valid {
            Ok(VerifiedChallenge::Valid(next))
        } else {
            Ok(VerifiedChallenge::Rejected(result))
        };
    }

    Err(ErrorResponse::from(HttpError::Conflict(
        "OTP challenge changed, retry verification".to_string(),
    )))
}

async fn store_mfa_verified_marker(
    auth: &AuthenticatedUser,
    method: OtpMethod,
    purpose: &str,
    now: u64,
) -> Result<MfaVerificationMarker, ErrorResponse> {
    let ttl = mfa_step_up_ttl_secs()?;
    let marker = MfaVerificationMarker {
        user_id: auth.user.id.clone(),
        method,
        purpose: purpose.to_string(),
        verified_at_unix: now,
        expires_at_unix: now.saturating_add(ttl),
    };
    let key = mfa_verified_key(&auth.sid, purpose);
    crate::etc::store::use_store()
        .set(&key, &marker, Some(ttl))
        .await
        .map_err(ErrorResponse::internal)?;
    Ok(marker)
}

fn challenge_response(challenge: &StoredOtpChallenge) -> OtpChallengeResponse {
    OtpChallengeResponse {
        challenge_id: challenge.record.id_hex(),
        method: challenge.method,
        expires_at_unix: challenge.record.expires_at_unix,
        ttl_seconds: challenge.record.ttl_seconds(),
        max_attempts: challenge.record.max_attempts,
    }
}

fn fake_challenge_response(
    now: u64,
    config: &MessageOtpConfig,
) -> Result<OtpChallengeResponse, ErrorResponse> {
    Ok(OtpChallengeResponse {
        challenge_id: random_challenge_id()?,
        method: OtpMethod::Email,
        expires_at_unix: now.saturating_add(config.ttl_seconds),
        ttl_seconds: config.ttl_seconds,
        max_attempts: config.max_attempts,
    })
}

fn send_email_otp(
    user: &db::ent::User,
    issued: &otp::IssuedMessageOtp,
) -> Result<(), smtp::SmtpError> {
    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);
    let content = issued.delivery_content(ISSUER);
    let subject = content
        .subject
        .unwrap_or_else(|| format!("{ISSUER} verification code"));
    let body = render_email_body(&content.body);

    Smtp::new()
        .to(content.recipient)
        .name(Some(name))
        .subject(subject)
        .html_body(body)
        .build()
        .and_then(|smtp| smtp.send())?;

    Ok(())
}

async fn mfa_policy_for_user(user: &db::ent::User) -> Result<MfaPolicy, ErrorResponse> {
    let mode = mfa_mode()?;
    let mut methods = configured_mfa_methods();
    let user_methods = user_mfa_methods(&user.attrs);
    if !user_methods.is_empty() {
        methods.retain(|method| user_methods.contains(method));
    }

    let user_enabled = user_mfa_enabled(&user.attrs);
    if mode == MfaMode::Off {
        methods.clear();
    }

    let super_admin_required = mfa_required_for_super_admin()
        && crate::fun::is_super_admin_user_id(&user.id)
            .await
            .map_err(ErrorResponse::internal)?;

    let required = match mode {
        MfaMode::Off => false,
        MfaMode::Optional => user_enabled || super_admin_required,
        MfaMode::Required => true,
    };

    let preferred_method = user_preferred_mfa_method(&user.attrs)
        .filter(|method| methods.contains(method))
        .or_else(|| default_mfa_method().filter(|method| methods.contains(method)))
        .or_else(|| methods.first().copied());

    Ok(MfaPolicy {
        mode,
        required,
        methods,
        preferred_method,
    })
}

fn ensure_method_allowed(policy: &MfaPolicy, method: OtpMethod) -> Result<(), ErrorResponse> {
    if policy.mode == MfaMode::Off && policy.methods.is_empty() {
        return Err(ErrorResponse::from(HttpError::Forbidden(
            "MFA is disabled".to_string(),
        )));
    }

    if !policy.methods.contains(&method) {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "MFA method is not available".to_string(),
        )));
    }

    Ok(())
}

async fn check_and_record_otp_request(client_ip: &str, subject: &str) -> Result<(), ErrorResponse> {
    let store = crate::etc::store::use_store();
    let source_key = format!("{OTP_REQUEST_KEY_PREFIX}:ip:{client_ip}");
    let subject_key = format!("{OTP_REQUEST_KEY_PREFIX}:subject:{subject}:ip:{client_ip}");
    let source_attempts = read_otp_request_count(&source_key).await?;
    let subject_attempts = read_otp_request_count(&subject_key).await?;

    if source_attempts >= MAX_OTP_SOURCE_REQUESTS || subject_attempts >= MAX_OTP_SUBJECT_REQUESTS {
        let mut err = ErrorResponse::from(HttpError::TooManyRequests(
            "too many OTP requests, try again later".to_string(),
        ));
        err.insert_header("Retry-After", &OTP_REQUEST_LOCKOUT_SECS.to_string());
        return Err(err);
    }

    let ttl = Some(OTP_REQUEST_LOCKOUT_SECS);

    let _ = store
        .incr_i64(&source_key, 1, ttl)
        .await
        .map_err(otp_unavailable)?;

    let _ = store
        .incr_i64(&subject_key, 1, ttl)
        .await
        .map_err(otp_unavailable)?;

    Ok(())
}

async fn read_otp_request_count(key: &str) -> Result<u32, ErrorResponse> {
    let value = crate::etc::store::use_store()
        .get_i64(key)
        .await
        .map_err(otp_unavailable)?
        .unwrap_or(0);

    Ok(if value <= 0 {
        0
    } else {
        u32::try_from(value).unwrap_or(u32::MAX)
    })
}

fn verification_error(result: MessageOtpVerification) -> ErrorResponse {
    match result {
        MessageOtpVerification::Valid => ErrorResponse::internal("unexpected valid OTP state"),
        MessageOtpVerification::Invalid { attempts_remaining } => {
            ErrorResponse::from(HttpError::Unauthorized(format!(
                "Invalid code. {attempts_remaining} attempt(s) remaining"
            )))
        }
        MessageOtpVerification::Expired | MessageOtpVerification::AlreadyUsed => {
            invalid_or_expired_code()
        }
        MessageOtpVerification::AttemptsExceeded => ErrorResponse::from(
            HttpError::TooManyRequests("Too many invalid OTP attempts".to_string()),
        ),
    }
}

fn normalize_email(value: &str) -> Result<String, ErrorResponse> {
    let email = value.trim();
    if email.is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "email is required".to_string(),
        )));
    }

    Ok(email.to_string())
}

fn validate_code(code: &str) -> Result<(), ErrorResponse> {
    if code.trim().is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "code is required".to_string(),
        )));
    }

    Ok(())
}

fn normalize_purpose(value: Option<&str>, default: &str) -> Result<String, ErrorResponse> {
    let purpose = value.unwrap_or(default).trim();
    if purpose.is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "purpose must not be empty".to_string(),
        )));
    }

    if purpose.len() > 128 {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "purpose is too long".to_string(),
        )));
    }

    Ok(purpose.to_string())
}

fn validate_challenge_id(challenge_id: &str) -> Result<(), ErrorResponse> {
    if challenge_id.len() != 32 || !challenge_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "challengeId is invalid".to_string(),
        )));
    }

    Ok(())
}

fn email_otp_config() -> Result<MessageOtpConfig, ErrorResponse> {
    MessageOtpConfig::numeric(
        env_usize("EMAIL_OTP_LENGTH", DEFAULT_EMAIL_OTP_LENGTH)?,
        env_u64("EMAIL_OTP_TTL_SECS", DEFAULT_EMAIL_OTP_TTL_SECS)?,
        env_u8("EMAIL_OTP_MAX_ATTEMPTS", DEFAULT_EMAIL_OTP_MAX_ATTEMPTS)?,
    )
    .map_err(email_otp_internal)
}

fn email_otp_pepper() -> Result<String, ErrorResponse> {
    let value = std::env::var("EMAIL_OTP_PEPPER").map_err(|_| {
        tracing::error!("EMAIL_OTP_PEPPER is not configured");
        ErrorResponse::from(HttpError::ServiceUnavailable(
            "email OTP temporarily unavailable".to_string(),
        ))
    })?;

    if value.trim().is_empty() {
        tracing::error!("EMAIL_OTP_PEPPER is empty");
        return Err(ErrorResponse::from(HttpError::ServiceUnavailable(
            "email OTP temporarily unavailable".to_string(),
        )));
    }

    Ok(value)
}

fn mfa_mode() -> Result<MfaMode, ErrorResponse> {
    match std::env::var("MFA_MODE")
        .unwrap_or_else(|_| "optional".to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => Ok(MfaMode::Off),
        "optional" => Ok(MfaMode::Optional),
        "required" => Ok(MfaMode::Required),
        other => {
            tracing::error!(value = other, "invalid MFA_MODE");
            Err(ErrorResponse::from(HttpError::ServiceUnavailable(
                "MFA temporarily unavailable".to_string(),
            )))
        }
    }
}

fn configured_mfa_methods() -> Vec<OtpMethod> {
    if !env_bool("MFA_EMAIL_ENABLED", true) {
        return Vec::new();
    }

    let configured = std::env::var("MFA_METHODS").unwrap_or_else(|_| "email".to_string());
    let mut methods = Vec::new();
    for method in configured.split(',').filter_map(parse_method) {
        if !methods.contains(&method) {
            methods.push(method);
        }
    }
    methods
}

fn default_mfa_method() -> Option<OtpMethod> {
    std::env::var("MFA_DEFAULT_METHOD")
        .ok()
        .as_deref()
        .and_then(parse_method)
        .or(Some(OtpMethod::Email))
}

fn mfa_required_for_super_admin() -> bool {
    env_bool("MFA_REQUIRED_FOR_SUPER_ADMIN", true)
}

fn mfa_step_up_ttl_secs() -> Result<u64, ErrorResponse> {
    let ttl = env_u64("MFA_STEP_UP_TTL_SECS", DEFAULT_MFA_STEP_UP_TTL_SECS)?;
    if ttl == 0 {
        tracing::error!("MFA_STEP_UP_TTL_SECS must be greater than zero");
        return Err(ErrorResponse::from(HttpError::ServiceUnavailable(
            "MFA temporarily unavailable".to_string(),
        )));
    }
    Ok(ttl)
}

fn parse_method(value: &str) -> Option<OtpMethod> {
    match value.trim().to_ascii_lowercase().as_str() {
        "email" => Some(OtpMethod::Email),
        _ => None,
    }
}

fn user_mfa_enabled(attrs: &Value) -> bool {
    attrs
        .get("mfa")
        .and_then(|mfa| mfa.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn user_mfa_methods(attrs: &Value) -> Vec<OtpMethod> {
    attrs
        .get("mfa")
        .and_then(|mfa| mfa.get("methods"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter_map(parse_method)
                .collect()
        })
        .unwrap_or_default()
}

fn user_preferred_mfa_method(attrs: &Value) -> Option<OtpMethod> {
    attrs
        .get("mfa")
        .and_then(|mfa| mfa.get("preferredMethod"))
        .and_then(Value::as_str)
        .and_then(parse_method)
}

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn env_usize(name: &str, default: usize) -> Result<usize, ErrorResponse> {
    parse_env(name, default)
}

fn env_u64(name: &str, default: u64) -> Result<u64, ErrorResponse> {
    parse_env(name, default)
}

fn env_u8(name: &str, default: u8) -> Result<u8, ErrorResponse> {
    parse_env(name, default)
}

fn parse_env<T>(name: &str, default: T) -> Result<T, ErrorResponse>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match std::env::var(name) {
        Ok(value) => value.parse::<T>().map_err(|error| {
            tracing::error!(name, %error, "invalid OTP environment variable");
            ErrorResponse::from(HttpError::ServiceUnavailable(
                "OTP temporarily unavailable".to_string(),
            ))
        }),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => {
            tracing::error!(name, %error, "invalid OTP environment variable");
            Err(ErrorResponse::from(HttpError::ServiceUnavailable(
                "OTP temporarily unavailable".to_string(),
            )))
        }
    }
}

fn current_unix_time() -> Result<u64, ErrorResponse> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ErrorResponse::internal("system time is before Unix epoch"))?
        .as_secs())
}

fn ttl_until(expires_at: u64, now: u64) -> u64 {
    expires_at.saturating_sub(now).max(1)
}

fn passwordless_email_key(challenge_id: &str) -> String {
    format!("{PASSWORDLESS_EMAIL_KEY_PREFIX}:{challenge_id}")
}

fn pending_login_key(challenge_id: &str) -> String {
    format!("{MFA_PENDING_LOGIN_KEY_PREFIX}:{challenge_id}")
}

fn mfa_challenge_key(user_id: &str, challenge_id: &str) -> String {
    format!("{MFA_CHALLENGE_KEY_PREFIX}:{user_id}:{challenge_id}")
}

fn mfa_verified_key(sid: &str, purpose: &str) -> String {
    format!("{MFA_VERIFIED_KEY_PREFIX}:{sid}:{purpose}")
}

fn random_challenge_id() -> Result<String, ErrorResponse> {
    let bytes = otp::generate_secret(16).map_err(email_otp_internal)?;
    Ok(hex_encode(&bytes))
}

fn render_email_body(body: &str) -> String {
    format!(
        "<!doctype html><html><body><p>{}</p></body></html>",
        escape_html(body)
    )
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }

    escaped
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn invalid_or_expired_code() -> ErrorResponse {
    ErrorResponse::from(HttpError::Unauthorized(
        "Invalid or expired code".to_string(),
    ))
}

fn email_otp_internal(error: impl std::fmt::Display) -> ErrorResponse {
    tracing::error!("email OTP error: {}", error);
    ErrorResponse::from(HttpError::ServiceUnavailable(
        "email OTP temporarily unavailable".to_string(),
    ))
}

fn otp_unavailable(error: store::StoreError) -> ErrorResponse {
    tracing::error!("OTP request guard unavailable: {}", error);
    ErrorResponse::from(HttpError::ServiceUnavailable(
        "OTP temporarily unavailable".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn challenge_id_must_be_32_hex_chars() {
        assert!(validate_challenge_id("0123456789abcdef0123456789abcdef").is_ok());
        assert!(validate_challenge_id("0123456789abcdef").is_err());
        assert!(validate_challenge_id("0123456789abcdef0123456789abcdeg").is_err());
    }

    #[test]
    fn html_body_escapes_untrusted_text() {
        let rendered = render_email_body("Code <123> & \"quote\"");

        assert!(rendered.contains("&lt;123&gt;"));
        assert!(rendered.contains("&amp;"));
        assert!(rendered.contains("&quot;quote&quot;"));
    }

    #[test]
    fn ttl_until_never_returns_zero() {
        assert_eq!(ttl_until(10, 20), 1);
        assert_eq!(ttl_until(20, 20), 1);
        assert_eq!(ttl_until(30, 20), 10);
    }

    #[test]
    fn user_mfa_attrs_parse_email_method() {
        let attrs = json!({
            "mfa": {
                "enabled": true,
                "methods": ["email", "totp"],
                "preferredMethod": "email"
            }
        });

        assert!(user_mfa_enabled(&attrs));
        assert_eq!(user_mfa_methods(&attrs), vec![OtpMethod::Email]);
        assert_eq!(user_preferred_mfa_method(&attrs), Some(OtpMethod::Email));
    }
}
