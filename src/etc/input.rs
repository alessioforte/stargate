#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("{field} is required")]
    Required { field: &'static str },
    #[error("{field} is too long")]
    TooLong { field: &'static str },
    #[error("{field} contains invalid characters")]
    InvalidCharacters { field: &'static str },
}

pub fn trim_required<'a>(value: &'a str, field: &'static str) -> Result<&'a str, InputError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(InputError::Required { field });
    }
    Ok(value)
}

pub fn reject_control_chars(value: &str, field: &'static str) -> Result<(), InputError> {
    if value.chars().any(char::is_control) {
        return Err(InputError::InvalidCharacters { field });
    }
    Ok(())
}

pub fn enforce_max_len(value: &str, max_len: usize, field: &'static str) -> Result<(), InputError> {
    if value.len() > max_len {
        return Err(InputError::TooLong { field });
    }
    Ok(())
}

pub fn is_fixed_len_ascii_hex(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
