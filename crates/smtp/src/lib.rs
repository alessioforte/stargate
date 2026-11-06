pub mod settings;
mod smtp;

pub use settings::{SmtpSettings, SmtpSettingsError, SmtpTls};
pub use smtp::*;
