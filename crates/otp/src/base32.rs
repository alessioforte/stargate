use crate::error::{OtpError, Result};

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn encode_base32_secret(secret: &[u8]) -> String {
    if secret.is_empty() {
        return String::new();
    }

    let mut output = String::with_capacity((secret.len() * 8).div_ceil(5));
    let mut buffer = 0u16;
    let mut bits_left = 0u8;

    for byte in secret {
        buffer = (buffer << 8) | u16::from(*byte);
        bits_left += 8;

        while bits_left >= 5 {
            let index = ((buffer >> (bits_left - 5)) & 0b1_1111) as usize;
            output.push(ALPHABET[index] as char);
            bits_left -= 5;
        }
    }

    if bits_left > 0 {
        let index = ((buffer << (5 - bits_left)) & 0b1_1111) as usize;
        output.push(ALPHABET[index] as char);
    }

    output
}

pub fn decode_base32_secret(encoded: &str) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(encoded.len() * 5 / 8);
    let mut buffer = 0u32;
    let mut bits_left = 0u8;
    let mut saw_padding = false;
    let mut symbols = 0usize;

    for byte in encoded.bytes() {
        if byte == b'=' {
            saw_padding = true;
            continue;
        }

        if byte.is_ascii_whitespace() || byte == b'-' {
            continue;
        }

        if saw_padding {
            return Err(OtpError::InvalidBase32Secret);
        }

        let value = decode_value(byte).ok_or(OtpError::InvalidBase32Secret)?;
        symbols += 1;
        buffer = (buffer << 5) | u32::from(value);
        bits_left += 5;

        while bits_left >= 8 {
            output.push((buffer >> (bits_left - 8)) as u8);
            bits_left -= 8;
            buffer &= (1 << bits_left) - 1;
        }
    }

    if symbols == 0 {
        return Err(OtpError::InvalidBase32Secret);
    }

    if matches!(symbols % 8, 1 | 3 | 6) {
        return Err(OtpError::InvalidBase32Secret);
    }

    if bits_left > 0 && (buffer << (8 - bits_left)) != 0 {
        return Err(OtpError::InvalidBase32Secret);
    }

    Ok(output)
}

fn decode_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a'),
        b'2'..=b'7' => Some(byte - b'2' + 26),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_rfc4648_examples_without_padding() {
        assert_eq!(encode_base32_secret(b"f"), "MY");
        assert_eq!(encode_base32_secret(b"fo"), "MZXQ");
        assert_eq!(encode_base32_secret(b"foo"), "MZXW6");
        assert_eq!(encode_base32_secret(b"foob"), "MZXW6YQ");
        assert_eq!(encode_base32_secret(b"fooba"), "MZXW6YTB");
        assert_eq!(encode_base32_secret(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn decodes_rfc4648_examples_with_padding_and_spaces() {
        assert_eq!(decode_base32_secret("MY======").unwrap(), b"f");
        assert_eq!(decode_base32_secret("mzxw 6ytb oi").unwrap(), b"foobar");
        assert_eq!(decode_base32_secret("MZXW-6YTB-OI").unwrap(), b"foobar");
    }

    #[test]
    fn rejects_invalid_base32() {
        assert_eq!(
            decode_base32_secret("MZXW6YTB0I").unwrap_err(),
            OtpError::InvalidBase32Secret
        );
        assert_eq!(
            decode_base32_secret("A").unwrap_err(),
            OtpError::InvalidBase32Secret
        );
    }
}
