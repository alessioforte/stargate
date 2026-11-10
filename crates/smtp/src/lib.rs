pub mod settings;
mod smtp;

pub use settings::{SmtpSettings, SmtpSettingsError, SmtpTls, default_locale};
pub use smtp::*;
