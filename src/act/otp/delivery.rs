use crate::err::ErrorResponse;
use crate::fun::format_name;
use smtp::Smtp;

use super::ISSUER;
use super::types::EmailOtpDelivery;
use super::util::{email_otp_internal, render_email_body};

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
    let content = issued.delivery_content(ISSUER);
    let subject = content
        .subject
        .unwrap_or_else(|| format!("{ISSUER} verification code"));
    let body = render_email_body(&content.body);

    EmailOtpDelivery {
        recipient: content.recipient,
        name: Some(name),
        subject,
        body,
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
        .to(delivery.recipient)
        .name(delivery.name)
        .subject(delivery.subject)
        .html_body(delivery.body)
        .build()
        .and_then(|smtp| smtp.send())?;

    Ok(())
}
