use crate::error::{OtpError, Result};

pub(crate) fn validate_issuer(issuer: &str) -> Result<()> {
    validate_component(issuer, OtpError::InvalidIssuer)
}

pub(crate) fn validate_account_name(account_name: &str) -> Result<()> {
    validate_component(account_name, OtpError::InvalidAccountName)
}

pub(crate) fn percent_encode(input: &str) -> String {
    let mut output = String::with_capacity(input.len());

    for byte in input.bytes() {
        if is_unreserved(byte) {
            output.push(byte as char);
        } else {
            output.push('%');
            output.push(nibble_to_hex(byte >> 4));
            output.push(nibble_to_hex(byte & 0x0f));
        }
    }

    output
}

fn validate_component(value: &str, error: OtpError) -> Result<()> {
    if value.trim().is_empty() {
        return Err(error);
    }

    Ok(())
}

fn is_unreserved(byte: u8) -> bool {
    matches!(
        byte,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~'
    )
}

fn nibble_to_hex(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'A' + value - 10) as char,
        _ => unreachable!("nibble is always <= 15"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encodes_uri_components() {
        assert_eq!(percent_encode("Stargate IAM"), "Stargate%20IAM");
        assert_eq!(percent_encode("alice@example.com"), "alice%40example.com");
        assert_eq!(percent_encode("a/b:c"), "a%2Fb%3Ac");
    }
}
