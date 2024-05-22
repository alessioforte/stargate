#![allow(dead_code)]

// Expire Password
// Hashing iterations ?
// Not Recently Used
// Password Blacklist
// Regular Expression
// Maximun Authentication Age
// Hashing Algorithm

// force_expired password_change
// hash iterations
// password history
// password_blacklist
// regex_pattern
// max_auth_age
// hash_algorithm

pub enum PasswordPolicy {
    MinLength(usize),
    MaxLength(usize),
    Lowercase,
    Uppercase,
    Digits,
    SpecialChars,
    // NotUsername,
    // NotEmail,

    // ForceExpired,
    // HashIterations,
    // PasswordHistory,
    // PasswordBlacklist,
    // Length,
    // RegexPattern,
    // MaxAuthAge,
    // HashAlgorithm,
}

impl PasswordPolicy {
    pub fn new(policies: Vec<&str>) -> Vec<PasswordPolicy> {
        policies
            .iter()
            .filter_map(|policy| PasswordPolicy::from_str(policy))
            .collect()
    }

    pub fn default() -> Vec<PasswordPolicy> {
        vec![
            PasswordPolicy::MinLength(8),
            PasswordPolicy::MaxLength(64),
            PasswordPolicy::Lowercase,
            PasswordPolicy::Uppercase,
            PasswordPolicy::Digits,
            PasswordPolicy::SpecialChars,
        ]
    }

    pub fn from_str(policy: &str) -> Option<PasswordPolicy> {
        match policy {
            "min_length" => Some(PasswordPolicy::MinLength(8)),
            "max_length" => Some(PasswordPolicy::MaxLength(64)),
            "lowercase" => Some(PasswordPolicy::Lowercase),
            "uppercase" => Some(PasswordPolicy::Uppercase),
            "digits" => Some(PasswordPolicy::Digits),
            "special_chars" => Some(PasswordPolicy::SpecialChars),
            _ => None,
        }
    }

    pub fn to_string(&self) -> String {
        match *self {
            PasswordPolicy::MinLength(_) => "min_length".to_string(),
            PasswordPolicy::MaxLength(_) => "max_length".to_string(),
            PasswordPolicy::Lowercase => "lowercase".to_string(),
            PasswordPolicy::Uppercase => "uppercase".to_string(),
            PasswordPolicy::Digits => "digits".to_string(),
            PasswordPolicy::SpecialChars => "special_chars".to_string(),
        }
    }
}

pub trait PasswordPolicyValidator {
    fn validate(&self, password: &str) -> Result<(), String>;
}

impl PasswordPolicyValidator for Vec<PasswordPolicy> {
    fn validate(&self, password: &str) -> Result<(), String> {
        for policy in self {
            match *policy {
                PasswordPolicy::MinLength(min) => {
                    if password.len() < min {
                        return Err(format!("Password must be at least {} characters long", min));
                    }
                }
                PasswordPolicy::MaxLength(max) => {
                    if password.len() > max {
                        return Err(format!("Password must be at most {} characters long", max));
                    }
                }
                PasswordPolicy::Lowercase => {
                    if !password.chars().any(|c| c.is_lowercase()) {
                        return Err(
                            "Password must contain at least one lowercase letter".to_string()
                        );
                    }
                }
                PasswordPolicy::Uppercase => {
                    if !password.chars().any(|c| c.is_uppercase()) {
                        return Err(
                            "Password must contain at least one uppercase letter".to_string()
                        );
                    }
                }
                PasswordPolicy::Digits => {
                    if !password.chars().any(|c| c.is_numeric()) {
                        return Err("Password must contain at least one digit".to_string());
                    }
                }
                PasswordPolicy::SpecialChars => {
                    if password.chars().all(|c| c.is_alphanumeric()) {
                        return Err(
                            "Password must contain at least one special character".to_string()
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn test_password_validation() {
    //     let password_policies = PasswordPolicy::new(vec![
    //         "min_length",
    //         "max_length",
    //         "lowercase",
    //         "uppercase",
    //         "digits",
    //         "special_chars",
    //     ]);

    //     let password = "pass";
    //     let result = password_policies.validate(password);
    //     assert_eq!(
    //         result,
    //         Err("Password must be at least 8 characters long".to_string())
    //     );

    //     let password = "Password";
    //     assert_eq!(
    //         password_policies.validate(password),
    //         Err("Password must contain at least one digit".to_string())
    //     );

    //     let password = "Password1";
    //     assert_eq!(
    //         password_policies.validate(password),
    //         Err("Password must contain at least one special character".to_string())
    //     );

    //     let password = "Password1!";
    //     assert_eq!(password_policies.validate(password), Ok(()));
    // }

    #[test]
    fn min_length_validation() {
        let password_policies = PasswordPolicy::new(vec!["min_length"]);

        let password = "pass";
        let result = password_policies.validate(password);
        assert_eq!(
            result,
            Err("Password must be at least 8 characters long".to_string())
        );
    }

    #[test]
    fn max_length_validation() {
        let password_policies = PasswordPolicy::new(vec!["max_length"]);

        let password = "Password123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890";
        let result = password_policies.validate(password);
        assert_eq!(
            result,
            Err("Password must be at most 64 characters long".to_string())
        );
    }

    #[test]
    fn lowercase_validation() {
        let password_policies = PasswordPolicy::new(vec!["lowercase"]);

        let password = "PASSWORD";
        let result = password_policies.validate(password);
        assert_eq!(
            result,
            Err("Password must contain at least one lowercase letter".to_string())
        );
    }

    #[test]
    fn uppercase_validation() {
        let password_policies = PasswordPolicy::new(vec!["uppercase"]);

        let password = "password";
        let result = password_policies.validate(password);
        assert_eq!(
            result,
            Err("Password must contain at least one uppercase letter".to_string())
        );
    }

    #[test]
    fn digits_validation() {
        let password_policies = PasswordPolicy::new(vec!["digits"]);

        let password = "Password";
        assert_eq!(
            password_policies.validate(password),
            Err("Password must contain at least one digit".to_string())
        );
    }

    #[test]
    fn special_chars_validation() {
        let password_policies = PasswordPolicy::new(vec!["special_chars"]);

        let password = "Password1!";
        assert_eq!(password_policies.validate(password), Ok(()));
    }

    #[test]
    fn multiple_policies_validation() {
        let password_policies = PasswordPolicy::new(vec!["min_length", "uppercase", "digits"]);

        let password = "pass";
        let result = password_policies.validate(password);
        assert_eq!(
            result,
            Err("Password must be at least 8 characters long".to_string())
        );

        let password = "Password";
        assert_eq!(
            password_policies.validate(password),
            Err("Password must contain at least one digit".to_string())
        );

        let password = "Password1";
        assert_eq!(password_policies.validate(password), Ok(()));
    }
}
