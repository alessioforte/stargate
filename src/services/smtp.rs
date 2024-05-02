use lettre::transport::smtp::{response::Response, Error};
use lettre::{message::header::ContentType, Message, SmtpTransport, Transport};

// TODO: create template for email
pub fn send_email(address: String, token: String) -> Result<Response, Error> {
    let email = Message::builder()
        .from("NoBody <nobody@domain.tld>".parse().unwrap())
        .to(address.parse().unwrap())
        .subject("Registration Confirmation")
        .header(ContentType::TEXT_PLAIN)
        .body(String::from("Please confirm your email address by clicking the link below: http://landingpage/registration/confirm?token=") + &token)
        .unwrap();

    let username = std::env::var("SMTP_USERNAME").unwrap_or_else(|_| "".to_string());
    let password = std::env::var("SMTP_PASSWORD").unwrap_or_else(|_| "".to_string());
    let host = std::env::var("SMTP_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("SMTP_PORT").unwrap_or_else(|_| "1025".to_string());
    let connection_url = format!("smtp://{}:{}@{}:{}", username, password, host, port);
    let mailer = SmtpTransport::from_url(&connection_url).unwrap().build();

    let response = mailer.send(&email);
    if response.is_err() {
        log::error!("{:?}", response);
    }

    response
}
