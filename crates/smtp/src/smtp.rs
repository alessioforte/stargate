use handlebars::Handlebars;
use lettre::transport::smtp::{Error, response::Response};
use lettre::{Message, SmtpTransport, Transport, message::header::ContentType};
use serde_json::json;

pub enum Template {
    SignupRequest,
    ChangePasswordRequest,
    PasswordChangedNotification,
    // SignupCompleted,
    // LoginWithOauthProvider,
}

impl Template {
    pub fn subject(&self) -> &str {
        match self {
            Template::SignupRequest => "Signup Request",
            Template::ChangePasswordRequest => "Change Password Request",
            Template::PasswordChangedNotification => "Password Changed",
        }
    }

    pub fn filename(&self) -> &str {
        match self {
            Template::SignupRequest => "signup_request",
            Template::ChangePasswordRequest => "change_password_request",
            Template::PasswordChangedNotification => "password_changed_notification",
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

    pub fn build(&mut self) -> &mut Self {
        let name = self.name.clone().unwrap_or("".to_string());
        let to = format!("{} <{}>", name, self.email).parse().unwrap();
        let from =
            std::env::var("SMTP_FROM").unwrap_or_else(|_| "NoBody <nobody@domain.tld>".to_string());
        let subject = self.template.subject();
        let header = ContentType::TEXT_HTML;
        let body = {
            let mut handlebars = Handlebars::new();
            let name = self.template.filename();
            handlebars
                .register_template_file(name, format!(".stargate/transactional/{}.hbs", name))
                .unwrap();
            let data = json!({
                "name": self.name,
                "token": self.token,
            });

            match handlebars.render(self.template.filename(), &data) {
                Ok(body) => body,
                Err(e) => {
                    tracing::error!("build_email: {:?}", e);
                    "".to_string()
                }
            }
        };

        let message = Message::builder()
            .from(from.parse().unwrap())
            .to(to)
            .subject(subject)
            .header(header)
            .body(body)
            .unwrap();

        self.message = Some(message);

        self
    }

    pub fn send(&self) -> Result<Response, Error> {
        let username = std::env::var("SMTP_USERNAME").unwrap_or_else(|_| "".to_string());
        let password = std::env::var("SMTP_PASSWORD").unwrap_or_else(|_| "".to_string());
        let host = std::env::var("SMTP_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = std::env::var("SMTP_PORT").unwrap_or_else(|_| "1025".to_string());
        let connection_url = format!("smtp://{}:{}@{}:{}", username, password, host, port);

        let mailer = SmtpTransport::from_url(&connection_url).unwrap().build();

        let message = self.message.as_ref().unwrap();

        let response = mailer.send(message);
        if response.is_err() {
            tracing::error!("send_email: {:?}", response);
        }

        response
    }
}
