use lettre::{message::header::ContentType, Message, SmtpTransport, Transport};

pub fn send_email(address: String, token: String) -> bool {
    let email = Message::builder()
        .from("NoBody <nobody@domain.tld>".parse().unwrap())
        .to(address.parse().unwrap())
        .subject("Registration Confirmation")
        .header(ContentType::TEXT_PLAIN)
        .body(String::from("Please confirm your email address by clicking the link below: http://landingpage/registration/confirm?token=") + &token)
        .unwrap();

    let username = std::env::var("SMTP_USERNAME").unwrap();
    let password = std::env::var("SMTP_PASSWORD").unwrap();
    let host = std::env::var("SMTP_HOST").unwrap();
    let port = std::env::var("SMTP_PORT").unwrap();
    let connection_url = format!("smtp://{}:{}@{}:{}", username, password, host, port);
    println!("SMTP URL: {}", connection_url);

    // Open a remote connection to gmail
    let mailer = SmtpTransport::from_url(&connection_url).unwrap().build();

    // Send the email
    match mailer.send(&email) {
        Ok(_) => {
            log::info!("Email sent successfully!");
            true
        }
        Err(e) => {
            log::error!("Could not send email: {:?}", e);
            false
        }
    }
}
