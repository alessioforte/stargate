use lettre::address::AddressError;
use lettre::message::{Mailbox, header::ContentType};
use lettre::transport::smtp::{
    authentication::Credentials,
    client::{Tls, TlsParameters},
    response::Response,
};
use lettre::{Message, SmtpTransport, Transport};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tools::Locale;

use crate::settings::{SmtpSettings, SmtpTls, smtp_settings};

const TRANSACTIONAL_DIR: &str = ".stargate/transactional";
const SUBJECTS_FILE: &str = "subjects.json";

#[derive(Debug, thiserror::Error)]
pub enum SmtpError {
    #[error("invalid recipient email `{email}`")]
    InvalidRecipient {
        email: String,
        #[source]
        source: AddressError,
    },
    #[error("failed to read email template `{path}`")]
    TemplateRead {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read email subjects `{path}`")]
    SubjectsRead {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse email subjects `{path}`")]
    SubjectsParse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("no email subject for `{template}` in locale `{locale}`")]
    SubjectMissing { template: String, locale: String },
    #[error("email message has not been built")]
    MissingMessage,
    #[error(transparent)]
    MessageBuild(#[from] lettre::error::Error),
    #[error(transparent)]
    Transport(#[from] lettre::transport::smtp::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateValidationError {
    #[error("failed to read email subjects `{path}`")]
    SubjectsRead {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse email subjects `{path}`")]
    SubjectsParse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("email subjects have no `{locale}` translations")]
    LocaleMissing { locale: String },
    #[error("missing email template `{path}` for locale `{locale}`")]
    TemplateMissing { path: String, locale: String },
    #[error("missing email subject for `{template}` in locale `{locale}`")]
    SubjectMissing { template: String, locale: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Template {
    SignupRequest,
    SignupAccountExists,
    SignupCompleted,
    ChangePasswordRequest,
    PasswordChangedNotification,
    EmailOtp,
    // LoginWithOauthProvider,
}

impl Template {
    pub const ALL: &'static [Self] = &[
        Self::SignupRequest,
        Self::SignupAccountExists,
        Self::SignupCompleted,
        Self::ChangePasswordRequest,
        Self::PasswordChangedNotification,
        Self::EmailOtp,
    ];

    pub const fn filename(self) -> &'static str {
        match self {
            Self::SignupRequest => "signup-request",
            Self::SignupAccountExists => "signup-account-exists",
            Self::SignupCompleted => "signup-completed",
            Self::ChangePasswordRequest => "change-password-request",
            Self::PasswordChangedNotification => "password-changed",
            Self::EmailOtp => "email-otp",
        }
    }
}

fn template_path(dir: &Path, template: Template, locale: Locale) -> PathBuf {
    dir.join(format!("{}.{}.html", template.filename(), locale.as_str()))
}

type Subjects = BTreeMap<String, BTreeMap<String, String>>;

fn subjects_path(dir: &Path) -> PathBuf {
    dir.join(SUBJECTS_FILE)
}

fn load_subjects(dir: &Path) -> Result<Subjects, TemplateValidationError> {
    let path = subjects_path(dir);
    let contents =
        std::fs::read_to_string(&path).map_err(|source| TemplateValidationError::SubjectsRead {
            path: path.display().to_string(),
            source,
        })?;
    serde_json::from_str(&contents).map_err(|source| TemplateValidationError::SubjectsParse {
        path: path.display().to_string(),
        source,
    })
}

/// Verifies that every template and every subject exists for every supported
/// locale. Called during startup so a broken build never sends a partial email.
pub fn validate_templates_at(dir: &Path) -> Result<(), TemplateValidationError> {
    let subjects = load_subjects(dir)?;

    for &locale in Locale::ALL {
        let Some(locale_subjects) = subjects.get(locale.as_str()) else {
            return Err(TemplateValidationError::LocaleMissing {
                locale: locale.to_string(),
            });
        };

        for &template in Template::ALL {
            let path = template_path(dir, template, locale);
            if !path.is_file() {
                return Err(TemplateValidationError::TemplateMissing {
                    path: path.display().to_string(),
                    locale: locale.to_string(),
                });
            }

            let subject = locale_subjects
                .get(template.filename())
                .map(String::as_str)
                .unwrap_or_default();
            if subject.trim().is_empty() {
                return Err(TemplateValidationError::SubjectMissing {
                    template: template.filename().to_string(),
                    locale: locale.to_string(),
                });
            }
        }
    }

    Ok(())
}

pub fn validate_templates() -> Result<(), TemplateValidationError> {
    validate_templates_at(Path::new(TRANSACTIONAL_DIR))
}

fn load_subject(template: Template, locale: Locale) -> Result<String, SmtpError> {
    let path = subjects_path(Path::new(TRANSACTIONAL_DIR));
    let contents = std::fs::read_to_string(&path).map_err(|source| SmtpError::SubjectsRead {
        path: path.display().to_string(),
        source,
    })?;
    let subjects: Subjects =
        serde_json::from_str(&contents).map_err(|source| SmtpError::SubjectsParse {
            path: path.display().to_string(),
            source,
        })?;

    subjects
        .get(locale.as_str())
        .and_then(|locale_subjects| locale_subjects.get(template.filename()))
        .filter(|subject| !subject.trim().is_empty())
        .cloned()
        .ok_or_else(|| SmtpError::SubjectMissing {
            template: template.filename().to_string(),
            locale: locale.to_string(),
        })
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }

    escaped
}

/// Whether `value` parses as a deliverable recipient address.
pub fn is_valid_address(value: &str) -> bool {
    value.parse::<lettre::Address>().is_ok()
}

pub struct Smtp {
    message: Option<Message>,
    email: String,
    link: String,
    template: Template,
    locale: Option<Locale>,
    name: Option<String>,
    subject: Option<String>,
    body: Option<String>,
    variables: Vec<(String, String)>,
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
            link: "".to_string(),
            template: Template::SignupRequest,
            locale: None,
            name: None,
            subject: None,
            body: None,
            variables: Vec::new(),
        }
    }

    pub fn template(&mut self, template: Template) -> &mut Self {
        self.template = template;
        self
    }

    /// The language of the template and subject. Defaults to
    /// `DEFAULT_LOCALE`, then English.
    pub fn locale(&mut self, locale: Locale) -> &mut Self {
        self.locale = Some(locale);
        self
    }

    pub fn to(&mut self, email: String) -> &mut Self {
        self.email = email;
        self
    }

    /// The action URL a template renders as `{{link}}`, always built by the
    /// server so recipients only ever see links to this deployment.
    pub fn link(&mut self, link: String) -> &mut Self {
        self.link = link;
        self
    }

    pub fn name(&mut self, name: Option<String>) -> &mut Self {
        self.name = name;
        self
    }

    pub fn subject(&mut self, subject: impl Into<String>) -> &mut Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn html_body(&mut self, body: impl Into<String>) -> &mut Self {
        self.body = Some(body.into());
        self
    }

    /// A `{{key}}` value rendered in the template body, e.g. an OTP code.
    /// Values are HTML-escaped before insertion.
    pub fn variable(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.variables.push((key.into(), value.into()));
        self
    }

    fn resolved_locale(&self) -> Locale {
        self.locale.unwrap_or_else(crate::settings::default_locale)
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

    fn render_body(&self) -> Result<String, SmtpError> {
        if let Some(body) = &self.body {
            return Ok(body.clone());
        }

        let file_path = template_path(
            Path::new(TRANSACTIONAL_DIR),
            self.template,
            self.resolved_locale(),
        )
        .display()
        .to_string();
        let content =
            std::fs::read_to_string(&file_path).map_err(|source| SmtpError::TemplateRead {
                path: file_path,
                source,
            })?;
        Ok(self.render_template(content))
    }

    fn render_template(&self, content: String) -> String {
        let name = escape_html(self.name.as_deref().unwrap_or(""));
        let link = escape_html(&self.link);
        let mut rendered = content
            .replace("{{name}}", &name)
            .replace("{{link}}", &link);

        for (key, value) in &self.variables {
            rendered = rendered.replace(&format!("{{{{{key}}}}}"), &escape_html(value));
        }

        rendered
    }

    fn build_mailer(settings: &SmtpSettings) -> Result<SmtpTransport, SmtpError> {
        let builder = SmtpTransport::builder_dangerous(settings.host.clone()).port(settings.port);
        let builder = match settings.tls {
            SmtpTls::Starttls => {
                let tls = TlsParameters::new(settings.host.clone())?;
                builder.tls(Tls::Required(tls))
            }
            SmtpTls::Implicit => {
                let tls = TlsParameters::new(settings.host.clone())?;
                builder.tls(Tls::Wrapper(tls))
            }
            SmtpTls::None => builder,
        };
        let builder = if settings.username.is_empty() && settings.password.is_empty() {
            builder
        } else {
            builder.credentials(Credentials::new(
                settings.username.clone(),
                settings.password.clone(),
            ))
        };
        Ok(builder.build())
    }

    pub fn build(&mut self) -> Result<&mut Self, SmtpError> {
        let to = self.recipient_mailbox()?;
        let from = smtp_settings().from.clone();
        let subject = match self.subject.as_deref() {
            Some(subject) => subject.to_string(),
            None => load_subject(self.template, self.resolved_locale())?,
        };
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
        let message = self.message.as_ref().ok_or(SmtpError::MissingMessage)?;
        let mailer = Self::build_mailer(smtp_settings())?;

        let response = mailer.send(message).map_err(SmtpError::Transport);
        if let Err(error) = &response {
            tracing::error!("send_email: {:?}", error);
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Locale, Smtp, SmtpError, SmtpSettings, SmtpTls, Template, TemplateValidationError,
        subjects_path, template_path, validate_templates_at,
    };
    use std::path::{Path, PathBuf};

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
    fn build_accepts_custom_body_without_template_file() {
        let mut smtp = Smtp::new();
        let result = smtp
            .to("alice@example.com".to_string())
            .subject("Verification code")
            .html_body("<p>123456</p>")
            .build();

        assert!(result.is_ok());
    }

    #[test]
    fn render_template_replaces_the_action_link() {
        let mut smtp = Smtp::new();
        smtp.link("https://auth.example.com/signup?token=abc".to_string());

        assert_eq!(
            smtp.render_template("<a href=\"{{link}}\">Sign up</a>".to_string()),
            "<a href=\"https://auth.example.com/signup?token=abc\">Sign up</a>"
        );
    }

    #[test]
    fn render_template_replaces_custom_variables() {
        let mut smtp = Smtp::new();
        smtp.variable("code", "123456")
            .variable("expires", "5 minutes");

        assert_eq!(
            smtp.render_template("{{code}} expires in {{expires}}".to_string()),
            "123456 expires in 5 minutes"
        );
    }

    #[test]
    fn render_template_escapes_variables() {
        let mut smtp = Smtp::new();
        smtp.name(Some("Ada <admin> & Co".to_string()));
        smtp.link("https://example.com/?a=1&b=2".to_string());
        smtp.variable("code", "<123>");

        assert_eq!(
            smtp.render_template("{{name}}|{{link}}|{{code}}".to_string()),
            "Ada &lt;admin&gt; &amp; Co|https://example.com/?a=1&amp;b=2|&lt;123&gt;"
        );
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stargate-smtp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_template_tree(dir: &Path) {
        let mut locales = serde_json::Map::new();

        for &locale in Locale::ALL {
            let mut subjects = serde_json::Map::new();
            for &template in Template::ALL {
                subjects.insert(
                    template.filename().to_string(),
                    serde_json::Value::String(format!(
                        "{} {}",
                        template.filename(),
                        locale.as_str()
                    )),
                );
                std::fs::write(template_path(dir, template, locale), "<html></html>").unwrap();
            }
            locales.insert(
                locale.as_str().to_string(),
                serde_json::Value::Object(subjects),
            );
        }

        std::fs::write(subjects_path(dir), serde_json::to_string(&locales).unwrap()).unwrap();
    }

    #[test]
    fn validates_a_complete_template_tree() {
        let dir = temp_dir("complete");
        write_template_tree(&dir);

        assert!(validate_templates_at(&dir).is_ok());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn validation_fails_when_a_template_is_missing() {
        let dir = temp_dir("missing-template");
        write_template_tree(&dir);
        std::fs::remove_file(template_path(&dir, Template::EmailOtp, Locale::It)).unwrap();

        let error = validate_templates_at(&dir).unwrap_err();
        assert!(matches!(
            error,
            TemplateValidationError::TemplateMissing { .. }
        ));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn validation_fails_when_a_subject_is_missing() {
        let dir = temp_dir("missing-subject");
        write_template_tree(&dir);

        let path = subjects_path(&dir);
        let mut subjects: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        subjects["it"].as_object_mut().unwrap().remove("email-otp");
        std::fs::write(&path, serde_json::to_string(&subjects).unwrap()).unwrap();

        let error = validate_templates_at(&dir).unwrap_err();
        assert!(matches!(
            error,
            TemplateValidationError::SubjectMissing { .. }
        ));

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn build_mailer_builds_every_tls_mode() {
        for tls in [SmtpTls::Starttls, SmtpTls::Implicit, SmtpTls::None] {
            let settings = SmtpSettings {
                host: "localhost".to_string(),
                port: 1025,
                from: "NoBody <nobody@domain.tld>".parse().unwrap(),
                username: String::new(),
                password: String::new(),
                tls,
                default_locale: Locale::En,
            };
            assert!(Smtp::build_mailer(&settings).is_ok(), "{tls}");
        }
    }

    /// A server that advertises AUTH but not STARTTLS must never receive the
    /// credentials: the transport fails while negotiating TLS, before AUTH.
    #[test]
    fn required_starttls_sends_no_credentials_to_a_server_without_it() {
        use lettre::{Message, Transport};
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (commands, received) = mpsc::channel::<Vec<String>>();

        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            writer.write_all(b"220 test ESMTP\r\n").unwrap();
            let mut seen = Vec::new();
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let command = line.trim_end().to_string();
                seen.push(command.clone());
                let reply: &[u8] = if command.starts_with("EHLO") || command.starts_with("HELO") {
                    // Advertises an authentication mechanism and no STARTTLS.
                    b"250-test\r\n250-AUTH PLAIN LOGIN\r\n250 SIZE 10240000\r\n"
                } else if command.starts_with("QUIT") {
                    b"221 Bye\r\n"
                } else {
                    b"250 OK\r\n"
                };
                if writer.write_all(reply).is_err() {
                    break;
                }
                if command.starts_with("QUIT") {
                    break;
                }
            }
            let _ = commands.send(seen);
        });

        let settings = SmtpSettings {
            host: "127.0.0.1".to_string(),
            port,
            from: "Smoke <smoke@example.com>".parse().unwrap(),
            username: "project-id".to_string(),
            password: "secret".to_string(),
            tls: SmtpTls::Starttls,
            default_locale: Locale::En,
        };
        let message = Message::builder()
            .from(settings.from.clone())
            .to("recipient@example.com".parse().unwrap())
            .subject("STARTTLS regression")
            .body(String::from("body"))
            .unwrap();
        let mailer = Smtp::build_mailer(&settings).unwrap();

        assert!(mailer.send(&message).is_err());

        let seen = received.recv_timeout(Duration::from_secs(10)).unwrap();
        server.join().unwrap();
        assert!(
            seen.iter().any(|command| command.starts_with("EHLO")),
            "no EHLO: {seen:?}"
        );
        assert!(
            !seen.iter().any(|command| command.starts_with("AUTH")),
            "credentials sent without STARTTLS: {seen:?}"
        );
    }
}
