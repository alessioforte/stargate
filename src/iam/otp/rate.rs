use crate::err::{ErrorCode, ErrorResponse};
use store::AtomicStore;

use super::util::otp_unavailable;
use super::{MAX_OTP_SOURCE_REQUESTS, MAX_OTP_SUBJECT_REQUESTS, OTP_REQUEST_LOCKOUT_SECS};

pub(crate) async fn check_and_record_otp_request(
    client_ip: &str,
    subject: &str,
) -> Result<(), ErrorResponse> {
    let store = crate::data::store::use_store();
    let source_key = store::keys::otp::request_ip(client_ip);
    let subject_key = store::keys::otp::request_subject(subject, client_ip);
    let ttl = Some(OTP_REQUEST_LOCKOUT_SECS);

    let source_attempts = store
        .incr_i64(&source_key, 1, ttl)
        .await
        .map_err(otp_unavailable)?
        .unwrap_or(i64::MAX);

    let subject_attempts = store
        .incr_i64(&subject_key, 1, ttl)
        .await
        .map_err(otp_unavailable)?
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
