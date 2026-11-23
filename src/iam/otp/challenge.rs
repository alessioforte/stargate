use crate::err::{ErrorCode, ErrorResponse};
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
    let store = crate::data::store::use_store();

    for _ in 0..3 {
        let Some(current) = store.get::<super::types::PendingMfaLogin>(key).await? else {
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
            .await?;

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

    Err(ErrorResponse::new(ErrorCode::OtpChallengeChanged))
}

pub(crate) async fn verify_stored_challenge(
    key: &str,
    code: &str,
    pepper: &[u8],
    now: u64,
) -> Result<VerifiedChallenge, ErrorResponse> {
    let store = crate::data::store::use_store();

    for _ in 0..3 {
        let Some(current) = store.get::<StoredOtpChallenge>(key).await? else {
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
            .await?;

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

    Err(ErrorResponse::new(ErrorCode::OtpChallengeChanged))
}

pub(crate) async fn store_mfa_verified_marker(
    auth: &AuthenticatedUser,
    method: super::types::OtpMethod,
    purpose: &str,
    now: u64,
) -> Result<MfaVerificationMarker, ErrorResponse> {
    let ttl = super::policy::mfa_step_up_ttl_secs();
    let marker = MfaVerificationMarker {
        user_id: auth.user.id.clone(),
        method,
        purpose: purpose.to_string(),
        verified_at_unix: now,
        expires_at_unix: now.saturating_add(ttl),
    };
    let key = mfa_verified_key(&auth.sid, purpose);
    crate::data::store::use_store()
        .set(&key, &marker, Some(ttl))
        .await?;
    Ok(marker)
}

pub(crate) async fn has_valid_mfa_verification(
    sid: &str,
    user_id: &str,
    purpose: &str,
) -> Result<bool, ErrorResponse> {
    let now = current_unix_time()?;
    let key = mfa_verified_key(sid, purpose);
    let store = crate::data::store::use_store();
    let Some(marker) = store.get::<MfaVerificationMarker>(&key).await? else {
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

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::verify_stored_challenge;
    use crate::err::ErrorCode;
    use crate::iam::otp::types::{OtpMethod, StoredOtpChallenge, VerifiedChallenge};
    use crate::iam::otp::util::{mfa_challenge_key, verification_error};
    use ::otp::{MessageOtpConfig, MessageOtpVerification};
    use store::Store;

    const PEPPER: &[u8] = b"challenge-test-pepper";
    const NOW: u64 = 1_700_000_000;

    async fn stored_challenge(max_attempts: u8, ttl_seconds: u64) -> (String, String) {
        let issued = ::otp::issue_email_otp(
            "user@example.com".to_string(),
            "login",
            PEPPER,
            NOW,
            MessageOtpConfig::numeric(6, ttl_seconds, max_attempts).unwrap(),
        )
        .unwrap();
        let user_id = ulid::Ulid::generate().to_string();
        let key = mfa_challenge_key(&user_id, &issued.record.id_hex());
        crate::data::store::use_store()
            .set(
                &key,
                &StoredOtpChallenge {
                    user_id,
                    method: OtpMethod::Email,
                    purpose: "login".to_string(),
                    record: issued.record.clone(),
                },
                Some(ttl_seconds),
            )
            .await
            .unwrap();
        (key, issued.code)
    }

    fn rejected(result: VerifiedChallenge) -> MessageOtpVerification {
        match result {
            VerifiedChallenge::Valid(_) => panic!("a wrong code verified"),
            VerifiedChallenge::Rejected(result) => result,
        }
    }

    #[tokio::test]
    async fn wrong_expired_and_exhausted_codes_keep_their_answers() {
        // A wrong code reports the remaining attempts and keeps the challenge.
        let (key, _) = stored_challenge(5, 300).await;
        let result = rejected(
            verify_stored_challenge(&key, "000000x", PEPPER, NOW + 1)
                .await
                .unwrap(),
        );
        assert!(matches!(
            result,
            MessageOtpVerification::Invalid {
                attempts_remaining: 4
            }
        ));
        let error = verification_error(result);
        assert_eq!(error.code, ErrorCode::OtpCodeInvalid);
        assert_eq!(error.status, http::StatusCode::UNAUTHORIZED);

        // An expired challenge is gone and answers the generic expired code.
        let (expired, _) = stored_challenge(5, 300).await;
        let result = rejected(
            verify_stored_challenge(&expired, "000000", PEPPER, NOW + 300)
                .await
                .unwrap(),
        );
        assert_eq!(
            verification_error(result).code,
            ErrorCode::OtpCodeInvalidOrExpired
        );

        // Exhausting the attempts answers the dedicated rate limit.
        let (exhausted, _) = stored_challenge(2, 300).await;
        let first = rejected(
            verify_stored_challenge(&exhausted, "000000", PEPPER, NOW + 1)
                .await
                .unwrap(),
        );
        assert!(matches!(
            first,
            MessageOtpVerification::Invalid {
                attempts_remaining: 1
            }
        ));
        let second = rejected(
            verify_stored_challenge(&exhausted, "000000", PEPPER, NOW + 2)
                .await
                .unwrap(),
        );
        assert_eq!(second, MessageOtpVerification::AttemptsExceeded);
        let error = verification_error(second);
        assert_eq!(error.code, ErrorCode::OtpAttemptsExceeded);
        assert_eq!(error.status, http::StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn the_right_code_consumes_the_challenge_once() {
        let (key, code) = stored_challenge(5, 300).await;
        match verify_stored_challenge(&key, &code, PEPPER, NOW + 1)
            .await
            .unwrap()
        {
            VerifiedChallenge::Valid(challenge) => assert!(challenge.record.consumed),
            VerifiedChallenge::Rejected(result) => {
                panic!("the issued code was rejected: {result:?}")
            }
        }
        assert!(!crate::data::store::use_store().exists(&key).await.unwrap());
    }
}
