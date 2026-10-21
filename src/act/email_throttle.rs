//! Per-recipient throttle for unauthenticated flows that send email.
//!
//! The per-IP ingress allowance cannot stop one address from being flooded
//! from many sources. Each flow that emails an address named in an
//! unauthenticated request charges that address's bucket before any account
//! lookup, whether or not the address belongs to an account, so a denial
//! reveals nothing about account existence. Requests over the limit are
//! rejected without being counted: the window closes `WINDOW_SECS` after the
//! last counted request even while a flood continues.

use crate::err::{ErrorCode, ErrorResponse};
use sha2::{Digest, Sha256};
use store::AtomicStore;

const KEY_PREFIX: &str = "account:email:recipient";
const WINDOW_SECS: u64 = 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmailFlow {
    PasswordReset,
    Signup,
    PasswordlessOtp,
}

impl EmailFlow {
    fn name(self) -> &'static str {
        match self {
            Self::PasswordReset => "password_reset",
            Self::Signup => "signup",
            Self::PasswordlessOtp => "passwordless_otp",
        }
    }

    /// Requests one address may make through this flow per window. Flows keep
    /// separate buckets so flooding one cannot lock the recipient out of another.
    fn max_requests(self) -> i64 {
        match self {
            Self::PasswordReset | Self::Signup => 5,
            // Codes expire within minutes; leave room to re-request a few.
            Self::PasswordlessOtp => 10,
        }
    }
}

pub async fn check_and_record_email_request(
    flow: EmailFlow,
    email: &str,
) -> Result<(), ErrorResponse> {
    let store = crate::etc::store::use_store();
    let key = recipient_key(flow, email);
    let max = flow.max_requests();

    // Already exhausted: reject without extending the window.
    let current = store
        .get_i64(&key)
        .await
        .map_err(ErrorResponse::internal)?
        .unwrap_or(0);
    if current >= max {
        return Err(rate_limited());
    }

    // Concurrent requests that all passed the read are ordered here.
    let count = store
        .incr_i64(&key, 1, Some(WINDOW_SECS))
        .await
        .map_err(ErrorResponse::internal)?
        .unwrap_or(i64::MAX);
    if count > max {
        return Err(rate_limited());
    }
    Ok(())
}

/// Case and surrounding whitespace never select another bucket. The address is
/// hashed: keys stay bounded and the store holds no recipient addresses.
fn recipient_key(flow: EmailFlow, email: &str) -> String {
    let canonical = email.trim().to_lowercase();
    let digest = Sha256::digest(canonical.as_bytes());
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{KEY_PREFIX}:{}:{hex}", flow.name())
}

fn rate_limited() -> ErrorResponse {
    let mut error = ErrorResponse::new(ErrorCode::EmailRecipientRateLimited);
    error.insert_header("Retry-After", &WINDOW_SECS.to_string());
    error
}

#[cfg(test)]
mod tests {
    use super::{EmailFlow, recipient_key};

    #[test]
    fn recipient_keys_ignore_case_and_whitespace_and_hold_no_address() {
        let key = recipient_key(EmailFlow::PasswordReset, "Victim@Example.com");
        assert_eq!(
            key,
            recipient_key(EmailFlow::PasswordReset, "  victim@EXAMPLE.COM ")
        );
        assert!(key.starts_with("account:email:recipient:password_reset:"));
        assert!(!key.contains("example"));
        assert_ne!(key, recipient_key(EmailFlow::Signup, "victim@example.com"));
        assert_ne!(
            key,
            recipient_key(EmailFlow::PasswordReset, "other@example.com")
        );
    }
}

#[cfg(all(test, feature = "memory"))]
mod store_tests {
    use super::{EmailFlow, check_and_record_email_request, recipient_key};
    use crate::err::ErrorCode;
    use axum::response::IntoResponse;
    use store::AtomicStore;

    fn unique_email() -> String {
        format!("{}@example.com", ulid::Ulid::generate())
    }

    #[tokio::test]
    async fn exhausted_addresses_are_denied_without_extending_their_window() {
        let email = unique_email();
        for _ in 0..5 {
            check_and_record_email_request(EmailFlow::PasswordReset, &email)
                .await
                .unwrap();
        }
        let error = check_and_record_email_request(EmailFlow::PasswordReset, &email.to_uppercase())
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::EmailRecipientRateLimited);
        let response = error.into_response();
        assert_eq!(response.status(), http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["retry-after"], "3600");
        let counted = crate::etc::store::use_store()
            .get_i64(&recipient_key(EmailFlow::PasswordReset, &email))
            .await
            .unwrap();
        assert_eq!(counted, Some(5));

        // Other flows and other addresses keep their own budgets.
        check_and_record_email_request(EmailFlow::Signup, &email)
            .await
            .unwrap();
        check_and_record_email_request(EmailFlow::PasswordReset, &unique_email())
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_requests_cannot_exceed_the_budget() {
        let email = unique_email();
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..40 {
            let email = email.clone();
            tasks.spawn(async move {
                check_and_record_email_request(EmailFlow::PasswordlessOtp, &email)
                    .await
                    .is_ok()
            });
        }
        let allowed = tasks.join_all().await.into_iter().filter(|ok| *ok).count();
        assert_eq!(allowed, 10);
    }
}
