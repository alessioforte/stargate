use crate::err::{ErrorCode, ErrorResponse};
use store::Store;

use super::auth::authenticated_user;
use super::challenge::{
    challenge_response, fake_challenge_response, store_mfa_verified_marker, verify_pending_login,
    verify_stored_challenge,
};
use super::delivery::{issue_email_challenge, send_email_otp, spawn_email_otp_delivery};
use super::policy::{
    ensure_method_allowed, ensure_passwordless_session_allowed, mfa_policy_for_user,
};
use super::rate::check_and_record_otp_request;
use super::types::{
    LoginMfaRequiredResponse, MfaMethodsResponse, MfaVerificationResponse, OtpChallengeResponse,
    OtpMethod, PendingMfaLogin, StoredOtpChallenge, VerifiedChallenge, VerifiedPendingLogin,
};
use super::util::{
    current_unix_time, email_otp_config, email_otp_pepper, invalid_or_expired_code,
    mfa_challenge_key, normalize_email, normalize_purpose, passwordless_email_key,
    pending_login_key, ttl_until, validate_challenge_id, validate_code, verification_error,
};
use super::{DEFAULT_MFA_PURPOSE, DEFAULT_PASSWORDLESS_PURPOSE};

pub(crate) async fn start_passwordless_email_otp(
    raw_email: &str,
    client_ip: &str,
) -> Result<OtpChallengeResponse, ErrorResponse> {
    let email = normalize_email(raw_email)?;
    check_and_record_otp_request(
        client_ip,
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
        return Ok(fake_response);
    };

    if !user.email.eq_ignore_ascii_case(&email) {
        return Ok(fake_response);
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

    spawn_email_otp_delivery(&user, &issued);

    Ok(response)
}

pub(crate) async fn complete_passwordless_email_otp(
    challenge_id: &str,
    code: &str,
) -> Result<(db::ent::User, usize), ErrorResponse> {
    validate_challenge_id(challenge_id)?;
    validate_code(code)?;

    let pepper = email_otp_pepper()?;
    let now = current_unix_time()?;
    let key = passwordless_email_key(challenge_id);
    let result = verify_stored_challenge(&key, code, pepper.as_bytes(), now).await?;

    let challenge = match result {
        VerifiedChallenge::Valid(challenge) => challenge,
        VerifiedChallenge::Rejected(result) => return Err(verification_error(result)),
    };

    let user = crate::db::get_user_by_id(&challenge.user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(invalid_or_expired_code)?;
    ensure_passwordless_session_allowed(&user).await?;

    let auth_time = chrono::Utc::now().timestamp() as usize;
    Ok((user, auth_time))
}

pub(crate) async fn maybe_start_login_mfa(
    user: &db::ent::User,
    client_ip: &str,
    auth_time: usize,
    requested_org_id: Option<&str>,
) -> Result<Option<LoginMfaRequiredResponse>, ErrorResponse> {
    let policy = mfa_policy_for_user(user).await?;
    if !policy.required {
        return Ok(None);
    }

    let method = policy.preferred_method.ok_or_else(|| {
        tracing::error!(user_id = %user.id, "MFA is required but no method is available");
        ErrorResponse::new(ErrorCode::MfaUnavailable)
    })?;
    ensure_method_allowed(&policy, method)?;
    check_and_record_otp_request(client_ip, &format!("mfa-login:{}", user.id)).await?;

    match method {
        OtpMethod::Email => start_email_login_mfa(
            user,
            method,
            auth_time,
            requested_org_id,
            current_unix_time()?,
        )
        .await
        .map(Some),
    }
}

async fn start_email_login_mfa(
    user: &db::ent::User,
    method: OtpMethod,
    auth_time: usize,
    requested_org_id: Option<&str>,
    now: u64,
) -> Result<LoginMfaRequiredResponse, ErrorResponse> {
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
        requested_org_id: requested_org_id.map(str::to_string),
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

    if let Err(error) = send_email_otp(user, &issued).await {
        let _ = store.delete(&key).await;
        return Err(error);
    }

    Ok(LoginMfaRequiredResponse {
        mfa_required: true,
        challenge_id: response.challenge_id,
        method: response.method,
        expires_at_unix: response.expires_at_unix,
        ttl_seconds: response.ttl_seconds,
        max_attempts: response.max_attempts,
    })
}

pub(crate) async fn complete_login_mfa(
    challenge_id: &str,
    code: &str,
) -> Result<(db::ent::User, usize, Option<String>), ErrorResponse> {
    validate_challenge_id(challenge_id)?;
    validate_code(code)?;

    let pepper = email_otp_pepper()?;
    let now = current_unix_time()?;
    let key = pending_login_key(challenge_id);
    let result = verify_pending_login(&key, code, pepper.as_bytes(), now).await?;
    let pending = match result {
        VerifiedPendingLogin::Valid(pending) => pending,
        VerifiedPendingLogin::Rejected(result) => return Err(verification_error(result)),
    };

    let user = crate::db::get_user_by_id(&pending.user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(invalid_or_expired_code)?;

    Ok((user, pending.auth_time, pending.requested_org_id))
}

pub(crate) async fn list_mfa_methods(
    token: Option<String>,
) -> Result<MfaMethodsResponse, ErrorResponse> {
    let auth = authenticated_user(token).await?;
    let policy = mfa_policy_for_user(&auth.user).await?;

    Ok(MfaMethodsResponse {
        mode: policy.mode,
        required: policy.required,
        methods: policy.methods,
        preferred_method: policy.preferred_method,
    })
}

pub(crate) async fn start_mfa_challenge(
    token: Option<String>,
    client_ip: &str,
    method: OtpMethod,
    purpose: Option<&str>,
) -> Result<OtpChallengeResponse, ErrorResponse> {
    let auth = authenticated_user(token).await?;
    let purpose = normalize_purpose(purpose, DEFAULT_MFA_PURPOSE)?;
    let policy = mfa_policy_for_user(&auth.user).await?;
    ensure_method_allowed(&policy, method)?;
    check_and_record_otp_request(client_ip, &format!("mfa:{}:{}", auth.user.id, purpose)).await?;

    match method {
        OtpMethod::Email => {
            start_email_mfa_challenge(&auth.user, &purpose, current_unix_time()?).await
        }
    }
}

async fn start_email_mfa_challenge(
    user: &db::ent::User,
    purpose: &str,
    now: u64,
) -> Result<OtpChallengeResponse, ErrorResponse> {
    let pepper = email_otp_pepper()?;
    let issued = issue_email_challenge(user, purpose, now, email_otp_config()?, pepper.as_bytes())?;
    let challenge = StoredOtpChallenge {
        user_id: user.id.clone(),
        method: OtpMethod::Email,
        purpose: purpose.to_string(),
        record: issued.record.clone(),
    };
    let response = challenge_response(&challenge);
    let key = mfa_challenge_key(&user.id, &response.challenge_id);
    let store = crate::etc::store::use_store();
    store
        .set(
            &key,
            &challenge,
            Some(ttl_until(challenge.record.expires_at_unix, now)),
        )
        .await
        .map_err(ErrorResponse::internal)?;

    if let Err(error) = send_email_otp(user, &issued).await {
        let _ = store.delete(&key).await;
        return Err(error);
    }

    Ok(response)
}

pub(crate) async fn complete_mfa_challenge(
    token: Option<String>,
    challenge_id: &str,
    code: &str,
) -> Result<MfaVerificationResponse, ErrorResponse> {
    validate_challenge_id(challenge_id)?;
    let auth = authenticated_user(token).await?;
    validate_code(code)?;

    let pepper = email_otp_pepper()?;
    let now = current_unix_time()?;
    let key = mfa_challenge_key(&auth.user.id, challenge_id);
    let result = verify_stored_challenge(&key, code, pepper.as_bytes(), now).await?;
    let challenge = match result {
        VerifiedChallenge::Valid(challenge) => challenge,
        VerifiedChallenge::Rejected(result) => return Err(verification_error(result)),
    };

    if challenge.user_id != auth.user.id {
        return Err(invalid_or_expired_code());
    }

    let marker =
        store_mfa_verified_marker(&auth, challenge.method, &challenge.purpose, now).await?;
    Ok(MfaVerificationResponse {
        method: marker.method,
        purpose: marker.purpose,
        verified_at_unix: marker.verified_at_unix,
        expires_at_unix: marker.expires_at_unix,
    })
}
