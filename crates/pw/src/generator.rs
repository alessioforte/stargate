use rand::Rng;

/// Generate a random password.
/// # Arguments
/// * `length` - The length of the password.
/// * `use_upper` - Whether to use uppercase letters.
/// * `use_lower` - Whether to use lowercase letters.
/// * `use_digits` - Whether to use digits.
/// * `use_special` - Whether to use special characters.
/// # Returns
/// * A random password as a `String`.
/// # Example
/// ```
/// use pw::generator;
///
/// let password = generator(16, true, true, true, true);
/// assert_eq!(password.len(), 16);
/// ```
/// # Note
/// This function uses the `rand` crate to generate random numbers.
pub fn generator(
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
        let password = generator(16, true, true, true, true);
        assert_eq!(password.len(), 16);
    }

    #[test]
    fn test_generate_password_no_upper() {
        let upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let password = generator(16, false, true, true, true);
        for c in password.chars() {
            assert!(!upper.contains(c));
        }
    }
}
