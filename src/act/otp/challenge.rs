use crate::err::{ErrorResponse, HttpError};
use store::Store;

use super::types::{
    AuthenticatedUser, MfaVerificationMarker, OtpChallengeResponse, StoredOtpChallenge,
    VerifiedChallenge, VerifiedPendingLogin,
};
use super::util::{current_unix_time, mfa_verified_key, random_challenge_id, ttl_until};

pub(crate) async fn verify_pending_login(
    key: &str,
    code: &str,
    pepper: &[u8],
    now: u64,
) -> Result<VerifiedPendingLogin, ErrorResponse> {
    let store = crate::etc::store::use_store();

    for _ in 0..3 {
        let Some(current) = store
            .get::<super::types::PendingMfaLogin>(key)
            .await
            .map_err(ErrorResponse::internal)?
        else {
            return Ok(VerifiedPendingLogin::Rejected(
                ::otp::MessageOtpVerification::Expired,
            ));
        };

        if current.expires_at_unix <= now || current.challenge.record.is_expired(now) {
            let _ = store.delete(key).await;
            return Ok(VerifiedPendingLogin::Rejected(
                ::otp::MessageOtpVerification::Expired,
            ));
        }

        if current.challenge.record.consumed {
            let _ = store.delete(key).await;
            return Ok(VerifiedPendingLogin::Rejected(
                ::otp::MessageOtpVerification::AlreadyUsed,
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
            ::otp::MessageOtpVerification::Valid
                | ::otp::MessageOtpVerification::Expired
                | ::otp::MessageOtpVerification::AttemptsExceeded
                | ::otp::MessageOtpVerification::AlreadyUsed
        ) && let Err(error) = store.delete(key).await
        {
            tracing::warn!(key, %error, "failed to delete completed pending MFA login");
        }

        return if result == ::otp::MessageOtpVerification::Valid {
            Ok(VerifiedPendingLogin::Valid(Box::new(next)))
        } else {
            Ok(VerifiedPendingLogin::Rejected(result))
        };
    }

    Err(ErrorResponse::from(HttpError::Conflict(
        "OTP challenge changed, retry verification".to_string(),
    )))
}

pub(crate) async fn verify_stored_challenge(
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
            return Ok(VerifiedChallenge::Rejected(
                ::otp::MessageOtpVerification::Expired,
            ));
        };

        if current.record.is_expired(now) {
            let _ = store.delete(key).await;
            return Ok(VerifiedChallenge::Rejected(
                ::otp::MessageOtpVerification::Expired,
            ));
        }

        if current.record.consumed {
            let _ = store.delete(key).await;
            return Ok(VerifiedChallenge::Rejected(
                ::otp::MessageOtpVerification::AlreadyUsed,
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
            ::otp::MessageOtpVerification::Valid
                | ::otp::MessageOtpVerification::Expired
                | ::otp::MessageOtpVerification::AttemptsExceeded
                | ::otp::MessageOtpVerification::AlreadyUsed
        ) && let Err(error) = store.delete(key).await
        {
            tracing::warn!(key, %error, "failed to delete completed OTP challenge");
        }

        return if result == ::otp::MessageOtpVerification::Valid {
            Ok(VerifiedChallenge::Valid(next))
        } else {
            Ok(VerifiedChallenge::Rejected(result))
        };
    }

    Err(ErrorResponse::from(HttpError::Conflict(
        "OTP challenge changed, retry verification".to_string(),
    )))
}

pub(crate) async fn store_mfa_verified_marker(
    auth: &AuthenticatedUser,
    method: super::types::OtpMethod,
    purpose: &str,
    now: u64,
) -> Result<MfaVerificationMarker, ErrorResponse> {
    let ttl = super::policy::mfa_step_up_ttl_secs()?;
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

pub(crate) async fn has_valid_mfa_verification(
    sid: &str,
    user_id: &str,
    purpose: &str,
) -> Result<bool, ErrorResponse> {
    let now = current_unix_time()?;
    let key = mfa_verified_key(sid, purpose);
    let store = crate::etc::store::use_store();
    let Some(marker) = store
        .get::<MfaVerificationMarker>(&key)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Ok(false);
    };

    if marker.expires_at_unix <= now {
        let _ = store.delete(&key).await;
        return Ok(false);
    }

    if marker.user_id != user_id || marker.purpose != purpose {
        let _ = store.delete(&key).await;
        return Ok(false);
    }

    Ok(true)
}

pub(crate) fn challenge_response(challenge: &StoredOtpChallenge) -> OtpChallengeResponse {
    OtpChallengeResponse {
        challenge_id: challenge.record.id_hex(),
        method: challenge.method,
        expires_at_unix: challenge.record.expires_at_unix,
        ttl_seconds: challenge.record.ttl_seconds(),
        max_attempts: challenge.record.max_attempts,
    }
}

pub(crate) fn fake_challenge_response(
    now: u64,
    config: &::otp::MessageOtpConfig,
) -> Result<OtpChallengeResponse, ErrorResponse> {
    Ok(OtpChallengeResponse {
        challenge_id: random_challenge_id()?,
        method: super::types::OtpMethod::Email,
        expires_at_unix: now.saturating_add(config.ttl_seconds),
        ttl_seconds: config.ttl_seconds,
        max_attempts: config.max_attempts,
    })
}
