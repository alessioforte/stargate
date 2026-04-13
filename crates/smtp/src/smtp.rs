use lettre::address::AddressError;
use lettre::message::{Mailbox, header::ContentType};
use lettre::transport::smtp::{authentication::Credentials, response::Response};
use lettre::{Message, SmtpTransport, Transport};

#[derive(Debug, thiserror::Error)]
pub enum SmtpError {
    #[error("invalid recipient email `{email}`")]
    InvalidRecipient {
        email: String,
        #[source]
        source: AddressError,
    },
    #[error("invalid SMTP sender mailbox `{value}`")]
    InvalidSender {
        value: String,
        #[source]
        source: AddressError,
    },
    #[error("failed to read email template `{path}`")]
    TemplateRead {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid SMTP port `{value}`")]
    InvalidPort {
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("email message has not been built")]
    MissingMessage,
    #[error(transparent)]
    MessageBuild(#[from] lettre::error::Error),
    #[error(transparent)]
    Transport(#[from] lettre::transport::smtp::Error),
}

pub enum Template {
    SignupRequest,
    SignupCompleted,
    ChangePasswordRequest,
    PasswordChangedNotification,
    // LoginWithOauthProvider,
}

impl Template {
    pub fn subject(&self) -> &str {
        match self {
            Template::SignupRequest => "Signup Request",
            Template::ChangePasswordRequest => "Change Password Request",
            Template::PasswordChangedNotification => "Password Changed",
            Template::SignupCompleted => "Welcome to Stargate!",
        }
    }

    pub fn filename(&self) -> &str {
        match self {
            Template::SignupRequest => "signup-request",
            Template::ChangePasswordRequest => "change-password-request",
            Template::PasswordChangedNotification => "password-changed",
            Template::SignupCompleted => "signup-completed",
        }
    }
}

pub struct Smtp {
    message: Option<Message>,
    email: String,
    token: String,
    template: Template,
    name: Option<String>,
}

impl Default for Smtp {
    fn default() -> Self {
        Smtp::new()
    }
}

impl Smtp {
    pub fn new() -> Smtp {
        Smtp {
            message: None,
            email: "".to_string(),
            token: "".to_string(),
            template: Template::SignupRequest,
            name: None,
        }
    }

    pub fn template(&mut self, template: Template) -> &mut Self {
        self.template = template;
        self
    }

    pub fn to(&mut self, email: String) -> &mut Self {
        self.email = email;
        self
    }

    pub fn token(&mut self, token: String) -> &mut Self {
        self.token = token;
        self
    }

    pub fn name(&mut self, name: Option<String>) -> &mut Self {
        self.name = name;
        self
    }

    fn recipient_mailbox(&self) -> Result<Mailbox, SmtpError> {
        let address = self
            .email
            .parse()
            .map_err(|source| SmtpError::InvalidRecipient {
                email: self.email.clone(),
                source,
            })?;
        let name = self.name.clone().filter(|value| !value.trim().is_empty());
        Ok(Mailbox::new(name, address))
    }

    fn sender_mailbox(value: &str) -> Result<Mailbox, SmtpError> {
        value.parse().map_err(|source| SmtpError::InvalidSender {
            value: value.to_string(),
            source,
        })
    }

    fn render_body(&self) -> Result<String, SmtpError> {
        let file_path = format!(".stargate/transactional/{}.html", self.template.filename());
        let content =
            std::fs::read_to_string(&file_path).map_err(|source| SmtpError::TemplateRead {
                path: file_path,
                source,
            })?;
        Ok(content
            .replace("{{name}}", self.name.as_deref().unwrap_or(""))
            .replace("{{token}}", &self.token))
    }

    fn build_mailer(
        host: String,
        port: &str,
        username: String,
        password: String,
    ) -> Result<SmtpTransport, SmtpError> {
        let port = port.parse().map_err(|source| SmtpError::InvalidPort {
            value: port.to_string(),
            source,
        })?;
        let builder = SmtpTransport::builder_dangerous(host).port(port);
        let builder = if username.is_empty() && password.is_empty() {
            builder
        } else {
            builder.credentials(Credentials::new(username, password))
        };
        Ok(builder.build())
    }

    pub fn build(&mut self) -> Result<&mut Self, SmtpError> {
        let to = self.recipient_mailbox()?;
        let from = Self::sender_mailbox(
            &std::env::var("SMTP_FROM")
                .unwrap_or_else(|_| "NoBody <nobody@domain.tld>".to_string()),
        )?;
        let subject = self.template.subject();
        let header = ContentType::TEXT_HTML;
        let body = self.render_body()?;

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .header(header)
            .body(body)
            .map_err(SmtpError::MessageBuild)?;

        self.message = Some(message);

        Ok(self)
    }

    pub fn send(&self) -> Result<Response, SmtpError> {
        let username = std::env::var("SMTP_USERNAME").unwrap_or_else(|_| "".to_string());
        let password = std::env::var("SMTP_PASSWORD").unwrap_or_else(|_| "".to_string());
        let host = std::env::var("SMTP_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = std::env::var("SMTP_PORT").unwrap_or_else(|_| "1025".to_string());
        let mailer = Self::build_mailer(host, &port, username, password)?;
        let message = self.message.as_ref().ok_or(SmtpError::MissingMessage)?;

        let response = mailer.send(message).map_err(SmtpError::Transport);
        if let Err(error) = &response {
            tracing::error!("send_email: {:?}", error);
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::{Smtp, SmtpError, Template};

    #[test]
    fn build_rejects_invalid_recipient_email() {
        let mut smtp = Smtp::new();
        let result = smtp
            .template(Template::SignupRequest)
            .to("not-an-email".to_string())
            .build();

        assert!(matches!(result, Err(SmtpError::InvalidRecipient { .. })));
    }

    #[test]
    fn send_rejects_missing_message() {
        let result = Smtp::new().send();

        assert!(matches!(result, Err(SmtpError::MissingMessage)));
    }

    #[test]
    fn build_mailer_rejects_invalid_port() {
        let result = Smtp::build_mailer(
            "localhost".to_string(),
            "not-a-port",
            String::new(),
            String::new(),
        );

        assert!(matches!(result, Err(SmtpError::InvalidPort { .. })));
    }
}
