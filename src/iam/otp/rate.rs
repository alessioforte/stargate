use crate::err::{ErrorCode, ErrorResponse};
use store::AtomicStore;

use super::{MAX_OTP_SOURCE_REQUESTS, MAX_OTP_SUBJECT_REQUESTS, OTP_REQUEST_LOCKOUT_SECS};

pub(crate) async fn check_and_record_otp_request(
    client: &str,
    subject: &str,
) -> Result<(), ErrorResponse> {
    let store = crate::data::store::use_store();
    let source_key = store::keys::otp::request_ip(client);
    let subject_key = store::keys::otp::request_subject(subject, client);
    let ttl = Some(OTP_REQUEST_LOCKOUT_SECS);

    let source_attempts = store
        .incr_i64(&source_key, 1, ttl)
        .await?
        .unwrap_or(i64::MAX);

    let subject_attempts = store
        .incr_i64(&subject_key, 1, ttl)
        .await?
        .unwrap_or(i64::MAX);

    if source_attempts > i64::from(MAX_OTP_SOURCE_REQUESTS)
        || subject_attempts > i64::from(MAX_OTP_SUBJECT_REQUESTS)
    {
        let mut err = ErrorResponse::new(ErrorCode::OtpRequestRateLimited);
        err.insert_header("Retry-After", &OTP_REQUEST_LOCKOUT_SECS.to_string());
        return Err(err);
    }

    Ok(())
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::{MAX_OTP_SUBJECT_REQUESTS, OTP_REQUEST_LOCKOUT_SECS, check_and_record_otp_request};
    use crate::err::ErrorCode;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn an_exhausted_subject_budget_keeps_its_rate_limit_answer() {
        let ip = format!("2001:db8:{}::1", ulid::Ulid::generate());
        let subject = ulid::Ulid::generate().to_string();
        for _ in 0..MAX_OTP_SUBJECT_REQUESTS {
            check_and_record_otp_request(&ip, &subject).await.unwrap();
        }

        let error = check_and_record_otp_request(&ip, &subject)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::OtpRequestRateLimited);
        let response = error.into_response();
        assert_eq!(response.status(), http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers()["retry-after"],
            OTP_REQUEST_LOCKOUT_SECS.to_string()
        );

        // Another subject behind the same address has its own budget.
        check_and_record_otp_request(&ip, &ulid::Ulid::generate().to_string())
            .await
            .unwrap();
    }
}
