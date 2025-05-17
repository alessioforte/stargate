use rand::Rng;

/// Generate a random password.
pub fn generate_password(
    length: usize,
    use_upper: bool,
    use_lower: bool,
    use_digits: bool,
    use_special: bool,
) -> String {
    let upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let lower = "abcdefghijklmnopqrstuvwxyz";
    let digits = "0123456789";
    let special = "!@#$%^&*()-_=+[]{}|;:,.<>?";

    let mut charset = String::new();
    if use_upper {
        charset.push_str(upper);
    }
    if use_lower {
        charset.push_str(lower);
    }
    if use_digits {
        charset.push_str(digits);
    }
    if use_special {
        charset.push_str(special);
    }

    let mut rng = rand::rng();
    let password: String = (0..length)
        .map(|_| {
            let idx = rng.random_range(0..charset.len());
            charset.chars().nth(idx).unwrap()
        })
        .collect();
    password
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_password() {
        let password = generate_password(16, true, true, true, true);
        assert_eq!(password.len(), 16);
    }

    #[test]
    fn test_generate_password_no_upper() {
        let upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let password = generate_password(16, false, true, true, true);
        for c in password.chars() {
            assert!(!upper.contains(c));
        }
    }
}
