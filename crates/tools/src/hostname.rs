//! ASCII DNS hostname validation shared by process settings.

pub fn valid_hostname(name: &str) -> bool {
    let name = name.strip_suffix('.').unwrap_or(name);
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_ascii_labels_and_hostname_length_boundaries() {
        for name in ["localhost", "EXAMPLE.com", "a-b.example.", "127.0.0.1"] {
            assert!(valid_hostname(name), "{name}");
        }
        for name in [
            "",
            ".",
            "a..b",
            "-a.com",
            "a-.com",
            "a_b.com",
            "a/b",
            "https://a",
            "a:80",
            "*.com",
            "é.com",
            "a..",
        ] {
            assert!(!valid_hostname(name), "{name}");
        }
        assert!(valid_hostname(&format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "a".repeat(63),
            "a".repeat(63),
            "a".repeat(61)
        )));
        assert!(!valid_hostname(&format!("{}.com", "a".repeat(64))));
        assert!(!valid_hostname(&format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "a".repeat(63),
            "a".repeat(63),
            "a".repeat(62)
        )));
    }
}
