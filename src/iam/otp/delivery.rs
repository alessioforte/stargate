use crate::err::ErrorResponse;
use crate::iam::user::profile::{format_name, locale as user_locale};
use smtp::{Smtp, Template};

use super::types::EmailOtpDelivery;
use super::util::email_otp_internal;

pub(crate) fn issue_email_challenge(
    user: &db::ent::User,
    purpose: &str,
    now: u64,
    config: ::otp::MessageOtpConfig,
    pepper: &[u8],
) -> Result<::otp::IssuedMessageOtp, ErrorResponse> {
    ::otp::issue_email_otp(user.email.clone(), purpose, pepper, now, config)
        .map_err(email_otp_internal)
}

fn email_otp_delivery(user: &db::ent::User, issued: &::otp::IssuedMessageOtp) -> EmailOtpDelivery {
    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);
    let minutes = issued.record.ttl_seconds().div_ceil(60);

    EmailOtpDelivery {
        recipient: issued.record.recipient.clone(),
        name: Some(name),
        code: issued.code.clone(),
        minutes,
        locale: user_locale(user).unwrap_or_else(smtp::default_locale),
    }
}

pub(crate) async fn send_email_otp(
    user: &db::ent::User,
    issued: &::otp::IssuedMessageOtp,
) -> Result<(), ErrorResponse> {
    let delivery = email_otp_delivery(user, issued);
    tokio::task::spawn_blocking(move || deliver_email_otp(delivery))
        .await
        .map_err(|error| {
            tracing::error!(%error, "email OTP delivery task failed");
            ErrorResponse::internal("email OTP delivery task failed")
        })?
        .map_err(email_otp_internal)?;

    Ok(())
}

pub(crate) fn spawn_email_otp_delivery(user: &db::ent::User, issued: &::otp::IssuedMessageOtp) {
    let delivery = email_otp_delivery(user, issued);
    std::mem::drop(tokio::task::spawn_blocking(move || {
        if let Err(error) = deliver_email_otp(delivery) {
            tracing::error!(%error, "failed to send passwordless email OTP");
        }
    }));
}

fn deliver_email_otp(delivery: EmailOtpDelivery) -> Result<(), smtp::SmtpError> {
    Smtp::new()
        .template(Template::EmailOtp)
        .locale(delivery.locale)
        .to(delivery.recipient)
        .name(delivery.name)
        .variable("code", delivery.code)
        .variable("minutes", delivery.minutes.to_string())
        .build()
        .and_then(|smtp| smtp.send())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::email_otp_delivery;
    use serde_json::json;
    use tools::Locale;

    fn issued(ttl_seconds: u64) -> ::otp::IssuedMessageOtp {
        let config = ::otp::MessageOtpConfig::numeric(6, ttl_seconds, 5).unwrap();
        ::otp::issue_email_otp(
            "alice@example.com".to_string(),
            "login",
            b"pepper",
            1_000,
            config,
        )
        .unwrap()
    }

    fn user(locale: Option<&str>) -> db::ent::User {
        let attrs = locale
            .map(|locale| json!({ "locale": locale }))
            .unwrap_or(json!({}));
        db::ent::User::new("alice@example.com".to_string(), "alice".to_string()).attrs(attrs)
    }

    #[test]
    fn delivery_uses_the_code_and_minutes_placeholders() {
        let issued = issued(300);
        let delivery = email_otp_delivery(&user(None), &issued);

        assert_eq!(delivery.code, issued.code);
        assert_eq!(delivery.minutes, 5);
        assert_eq!(delivery.locale, Locale::En);
    }

    #[test]
    fn delivery_uses_the_account_locale() {
        let delivery = email_otp_delivery(&user(Some("it")), &issued(30));

        assert_eq!(delivery.minutes, 1);
        assert_eq!(delivery.locale, Locale::It);
    }
}
