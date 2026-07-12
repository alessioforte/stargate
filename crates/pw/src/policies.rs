//! Password policies: a serde-configurable rule set with typed violations.
//!
//! Roadmap for stateful policies (history, expiry) and hashing hardening:
//! `docs/pw-improvement-plan.md`.

use serde::{Deserialize, Serialize};

use crate::banned::BannedPasswords;

/// Hard upper bound on password size in bytes, checked before any other rule
/// regardless of configured policy. Bounds the input handed to the Argon2
/// blocking pool.
pub const MAX_PASSWORD_BYTES: usize = 512;

/// Account context for context-aware rules (`not_username`, `not_email`) and
/// the banned-password list. `banned: None` falls back to the built-in list.
#[derive(Debug, Default, Clone, Copy)]
pub struct PolicyContext<'a> {
    pub username: Option<&'a str>,
    pub email: Option<&'a str>,
    pub banned: Option<&'a BannedPasswords>,
}

/// A parameterized password policy.
///
/// Deserializable from JSON/YAML (snake_case canonical, camelCase accepted),
/// so it can come from env config or `organizations.attrs.password_policy`.
/// Missing fields take the [`PasswordPolicySet::standard`] value, so partial
/// configs strengthen rather than silently weaken the policy; rules are
/// disabled only by explicit `false`/`null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PasswordPolicySet {
    /// Minimum length in characters. `null` disables.
    #[serde(alias = "minLength")]
    pub min_length: Option<usize>,
    /// Maximum length in characters. `null` disables (the
    /// [`MAX_PASSWORD_BYTES`] cap still applies).
    #[serde(alias = "maxLength")]
    pub max_length: Option<usize>,
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    #[serde(alias = "specialChars")]
    pub special_chars: bool,
    /// Reject passwords containing the account username (case-insensitive).
    #[serde(alias = "notUsername")]
    pub not_username: bool,
    /// Reject passwords containing the account email or its local part
    /// (case-insensitive).
    #[serde(alias = "notEmail")]
    pub not_email: bool,
    /// Reject passwords on the banned/common-password list.
    #[serde(alias = "bannedPasswords")]
    pub banned_passwords: bool,
    /// Regex the password must match (`Regex::is_match`). Validate with
    /// [`PasswordPolicySet::check`] at config-load time; an invalid pattern
    /// is skipped during validation.
    #[serde(alias = "regexPattern")]
    pub regex_pattern: Option<String>,
    /// Maximum allowed run of the same character (e.g. `2` rejects `"aaa"`).
    /// `null` disables.
    #[serde(alias = "maxRepeats")]
    pub max_repeats: Option<usize>,
    /// Maximum password age in days before login forces a change. **Stateful**:
    /// not evaluated by `validate_all` — enforced at login by the application
    /// against the credential's last-changed timestamp. `null` disables.
    #[serde(alias = "maxAgeDays")]
    pub max_age_days: Option<u32>,
    /// Reject reuse of the last N passwords (including the current one).
    /// **Stateful**: not evaluated by `validate_all` — enforced on password
    /// change by the application against stored hashes (see
    /// [`crate::Hash::matches_any`]). At most [`MAX_HISTORY_DEPTH`]. `null`
    /// disables.
    pub history: Option<usize>,
}

/// Maximum supported `history` depth; storage prunes older entries.
pub const MAX_HISTORY_DEPTH: usize = 24;

impl Default for PasswordPolicySet {
    fn default() -> Self {
        Self::standard()
    }
}

impl PasswordPolicySet {
    /// The default policy: 8-64 chars, all four character classes, no
    /// username/email substring, not a known-common password.
    pub fn standard() -> Self {
        Self {
            min_length: Some(8),
            max_length: Some(64),
            lowercase: true,
            uppercase: true,
            digits: true,
            special_chars: true,
            not_username: true,
            not_email: true,
            banned_passwords: true,
            regex_pattern: None,
            max_repeats: None,
            max_age_days: None,
            history: None,
        }
    }

    /// Check the policy itself for misconfiguration (call at config-load
    /// time): min/max consistency, compilable regex, sane `max_repeats`.
    pub fn check(&self) -> Result<(), String> {
        if let (Some(min), Some(max)) = (self.min_length, self.max_length)
            && min > max
        {
            return Err(format!(
                "min_length ({min}) must not exceed max_length ({max})"
            ));
        }
        if let Some(pattern) = &self.regex_pattern
            && let Err(e) = regex::Regex::new(pattern)
        {
            return Err(format!("invalid regex_pattern: {e}"));
        }
        if self.max_repeats == Some(0) {
            return Err("max_repeats must be at least 1".to_string());
        }
        if self.max_age_days == Some(0) {
            return Err("max_age_days must be at least 1".to_string());
        }
        if let Some(history) = self.history
            && !(1..=MAX_HISTORY_DEPTH).contains(&history)
        {
            return Err(format!("history must be between 1 and {MAX_HISTORY_DEPTH}"));
        }
        Ok(())
    }

    /// Validate, returning the first violation. See [`Self::validate_all`].
    pub fn validate(&self, password: &str, ctx: &PolicyContext) -> Result<(), PolicyViolation> {
        match self.validate_all(password, ctx).into_iter().next() {
            Some(violation) => Err(violation),
            None => Ok(()),
        }
    }

    /// Validate and return every violation (empty = accepted), so callers can
    /// surface a full requirements checklist in one round trip.
    pub fn validate_all(&self, password: &str, ctx: &PolicyContext) -> Vec<PolicyViolation> {
        if password.len() > MAX_PASSWORD_BYTES {
            return vec![PolicyViolation::TooManyBytes {
                max: MAX_PASSWORD_BYTES,
            }];
        }

        let mut violations = Vec::new();
        // Length rules count characters, not bytes, so multibyte passwords
        // are measured the way users perceive them.
        let char_count = password.chars().count();
        let lowered = password.to_lowercase();

        if let Some(min) = self.min_length
            && char_count < min
        {
            violations.push(PolicyViolation::TooShort { min });
        }
        if let Some(max) = self.max_length
            && char_count > max
        {
            violations.push(PolicyViolation::TooLong { max });
        }
        if self.lowercase && !password.chars().any(|c| c.is_lowercase()) {
            violations.push(PolicyViolation::MissingLowercase);
        }
        if self.uppercase && !password.chars().any(|c| c.is_uppercase()) {
            violations.push(PolicyViolation::MissingUppercase);
        }
        if self.digits && !password.chars().any(|c| c.is_numeric()) {
            violations.push(PolicyViolation::MissingDigit);
        }
        if self.special_chars && password.chars().all(|c| c.is_alphanumeric()) {
            violations.push(PolicyViolation::MissingSpecialChar);
        }
        if self.not_username
            && let Some(username) = ctx.username
            && contains_ci(&lowered, username)
        {
            violations.push(PolicyViolation::ContainsUsername);
        }
        if self.not_email
            && let Some(email) = ctx.email
            && (contains_ci(&lowered, email)
                || email
                    .split('@')
                    .next()
                    .is_some_and(|local| contains_ci(&lowered, local)))
        {
            violations.push(PolicyViolation::ContainsEmail);
        }
        if self.banned_passwords {
            let banned = ctx
                .banned
                .unwrap_or_else(|| BannedPasswords::default_list());
            if banned.contains(password) {
                violations.push(PolicyViolation::BannedPassword);
            }
        }
        if let Some(pattern) = &self.regex_pattern {
            // Invalid patterns are a config error caught by `check()` at load
            // time; at validation time the rule is skipped rather than
            // locking every password out.
            if let Ok(re) = regex::Regex::new(pattern)
                && !re.is_match(password)
            {
                violations.push(PolicyViolation::PatternMismatch);
            }
        }
        if let Some(max) = self.max_repeats
            && max > 0
            && has_run_longer_than(password, max)
        {
            violations.push(PolicyViolation::RepeatedCharacters { max });
        }

        violations
    }
}

/// Case-insensitive containment; needles shorter than 3 characters are
/// ignored so rules like `not_username` don't reject half the dictionary.
fn contains_ci(lowered_password: &str, needle: &str) -> bool {
    let needle = needle.trim().to_lowercase();
    needle.chars().count() >= 3 && lowered_password.contains(&needle)
}

fn has_run_longer_than(password: &str, max: usize) -> bool {
    let mut run = 0usize;
    let mut previous = None;
    for c in password.chars() {
        if Some(c) == previous {
            run += 1;
        } else {
            run = 1;
            previous = Some(c);
        }
        if run > max {
            return true;
        }
    }
    false
}

/// A single failed policy rule. `code()` is stable for machine consumption
/// (i18n keys, frontend checklists); `Display` is the human message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyViolation {
    TooManyBytes {
        max: usize,
    },
    TooShort {
        min: usize,
    },
    TooLong {
        max: usize,
    },
    MissingLowercase,
    MissingUppercase,
    MissingDigit,
    MissingSpecialChar,
    ContainsUsername,
    ContainsEmail,
    BannedPassword,
    PatternMismatch,
    RepeatedCharacters {
        max: usize,
    },
    /// Emitted by the application's stateful history check, never by
    /// `validate_all`.
    RecentlyUsed,
}

impl PolicyViolation {
    pub fn code(&self) -> &'static str {
        match self {
            PolicyViolation::TooManyBytes { .. } => "too_many_bytes",
            PolicyViolation::TooShort { .. } => "too_short",
            PolicyViolation::TooLong { .. } => "too_long",
            PolicyViolation::MissingLowercase => "missing_lowercase",
            PolicyViolation::MissingUppercase => "missing_uppercase",
            PolicyViolation::MissingDigit => "missing_digit",
            PolicyViolation::MissingSpecialChar => "missing_special_char",
            PolicyViolation::ContainsUsername => "contains_username",
            PolicyViolation::ContainsEmail => "contains_email",
            PolicyViolation::BannedPassword => "banned_password",
            PolicyViolation::PatternMismatch => "pattern_mismatch",
            PolicyViolation::RepeatedCharacters { .. } => "repeated_characters",
            PolicyViolation::RecentlyUsed => "recently_used",
        }
    }
}

impl std::fmt::Display for PolicyViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            PolicyViolation::TooManyBytes { max } => {
                write!(f, "Password must be at most {} bytes", max)
            }
            PolicyViolation::TooShort { min } => {
                write!(f, "Password must be at least {} characters long", min)
            }
            PolicyViolation::TooLong { max } => {
                write!(f, "Password must be at most {} characters long", max)
            }
            PolicyViolation::MissingLowercase => {
                write!(f, "Password must contain at least one lowercase letter")
            }
            PolicyViolation::MissingUppercase => {
                write!(f, "Password must contain at least one uppercase letter")
            }
            PolicyViolation::MissingDigit => {
                write!(f, "Password must contain at least one digit")
            }
            PolicyViolation::MissingSpecialChar => {
                write!(f, "Password must contain at least one special character")
            }
            PolicyViolation::ContainsUsername => {
                write!(f, "Password must not contain the username")
            }
            PolicyViolation::ContainsEmail => {
                write!(f, "Password must not contain the email address")
            }
            PolicyViolation::BannedPassword => {
                write!(f, "Password is too common; choose a less guessable one")
            }
            PolicyViolation::PatternMismatch => {
                write!(f, "Password does not match the required pattern")
            }
            PolicyViolation::RepeatedCharacters { max } => {
                write!(
                    f,
                    "Password must not repeat the same character more than {} times in a row",
                    max
                )
            }
            PolicyViolation::RecentlyUsed => {
                write!(
                    f,
                    "Password was used recently; choose one you have not used before"
                )
            }
        }
    }
}

impl std::error::Error for PolicyViolation {}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> PolicyContext<'static> {
        PolicyContext::default()
    }

    #[test]
    fn standard_policy_validation() {
        let policy = PasswordPolicySet::standard();

        assert_eq!(
            policy.validate("pass", &ctx()),
            Err(PolicyViolation::TooShort { min: 8 })
        );
        assert_eq!(
            policy.validate("Password", &ctx()),
            Err(PolicyViolation::MissingDigit)
        );
        assert_eq!(policy.validate("Xk9$mQ2!vLp7", &ctx()), Ok(()));
    }

    #[test]
    fn validate_all_aggregates_violations() {
        let policy = PasswordPolicySet::standard();
        let violations = policy.validate_all("abc", &ctx());
        let codes: Vec<&str> = violations.iter().map(|v| v.code()).collect();
        assert_eq!(
            codes,
            vec![
                "too_short",
                "missing_uppercase",
                "missing_digit",
                "missing_special_char",
            ]
        );
    }

    #[test]
    fn length_rules_count_characters_not_bytes() {
        let policy = PasswordPolicySet {
            min_length: Some(8),
            max_length: Some(64),
            ..permissive()
        };

        // 4 chars, 8 UTF-8 bytes: still too short.
        assert_eq!(
            policy.validate(&"ä".repeat(4), &ctx()),
            Err(PolicyViolation::TooShort { min: 8 })
        );
        // 40 chars, 80 UTF-8 bytes: within the 64-char maximum.
        assert_eq!(policy.validate(&"ä".repeat(40), &ctx()), Ok(()));
    }

    #[test]
    fn byte_cap_is_always_enforced() {
        let policy = permissive();
        assert_eq!(
            policy.validate(&"a".repeat(MAX_PASSWORD_BYTES + 1), &ctx()),
            Err(PolicyViolation::TooManyBytes {
                max: MAX_PASSWORD_BYTES
            })
        );
    }

    #[test]
    fn not_username_rejects_containment() {
        let policy = PasswordPolicySet {
            not_username: true,
            ..permissive()
        };
        let ctx = PolicyContext {
            username: Some("alessio"),
            ..Default::default()
        };

        assert_eq!(
            policy.validate("xxAlEsSiOxx1!", &ctx),
            Err(PolicyViolation::ContainsUsername)
        );
        assert_eq!(policy.validate("Xk9$mQ2!vLp7", &ctx), Ok(()));
    }

    #[test]
    fn short_usernames_do_not_reject() {
        let policy = PasswordPolicySet {
            not_username: true,
            ..permissive()
        };
        let ctx = PolicyContext {
            username: Some("al"),
            ..Default::default()
        };
        assert_eq!(policy.validate("normal-password-al", &ctx), Ok(()));
    }

    #[test]
    fn not_email_rejects_full_email_and_local_part() {
        let policy = PasswordPolicySet {
            not_email: true,
            ..permissive()
        };
        let ctx = PolicyContext {
            email: Some("forte@example.com"),
            ..Default::default()
        };

        assert_eq!(
            policy.validate("Forte@example.com1!", &ctx),
            Err(PolicyViolation::ContainsEmail)
        );
        assert_eq!(
            policy.validate("xxFORTExx9$", &ctx),
            Err(PolicyViolation::ContainsEmail)
        );
        assert_eq!(policy.validate("Xk9$mQ2!vLp7", &ctx), Ok(()));
    }

    #[test]
    fn banned_passwords_are_rejected() {
        let policy = PasswordPolicySet {
            banned_passwords: true,
            ..permissive()
        };
        assert_eq!(
            policy.validate("P@ssw0rd", &ctx()),
            Err(PolicyViolation::BannedPassword)
        );
        assert_eq!(policy.validate("Xk9$mQ2!vLp7", &ctx()), Ok(()));
    }

    #[test]
    fn regex_pattern_must_match() {
        let policy = PasswordPolicySet {
            regex_pattern: Some("^[A-Za-z0-9!@#$%^&*]+$".to_string()),
            ..permissive()
        };
        assert_eq!(
            policy.validate("has spaces 1!", &ctx()),
            Err(PolicyViolation::PatternMismatch)
        );
        assert_eq!(policy.validate("NoSpaces1!", &ctx()), Ok(()));
    }

    #[test]
    fn max_repeats_limits_runs() {
        let policy = PasswordPolicySet {
            max_repeats: Some(2),
            ..permissive()
        };
        assert_eq!(
            policy.validate("aaab", &ctx()),
            Err(PolicyViolation::RepeatedCharacters { max: 2 })
        );
        assert_eq!(policy.validate("aabaa", &ctx()), Ok(()));
    }

    #[test]
    fn deserializes_partial_config_with_standard_defaults() {
        let policy: PasswordPolicySet =
            serde_json::from_str(r#"{ "min_length": 12, "specialChars": false }"#).unwrap();
        assert_eq!(policy.min_length, Some(12));
        assert!(!policy.special_chars);
        // Missing fields keep the standard values.
        assert!(policy.uppercase);
        assert!(policy.banned_passwords);
        assert_eq!(policy.max_length, Some(64));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let result = serde_json::from_str::<PasswordPolicySet>(r#"{ "no_such_rule": true }"#);
        assert!(result.is_err());
    }

    #[test]
    fn check_catches_misconfiguration() {
        let mut policy = PasswordPolicySet::standard();
        assert_eq!(policy.check(), Ok(()));

        policy.min_length = Some(100);
        assert!(policy.check().is_err());

        let mut policy = PasswordPolicySet::standard();
        policy.regex_pattern = Some("(unclosed".to_string());
        assert!(policy.check().is_err());

        let mut policy = PasswordPolicySet::standard();
        policy.max_repeats = Some(0);
        assert!(policy.check().is_err());

        let mut policy = PasswordPolicySet::standard();
        policy.max_age_days = Some(0);
        assert!(policy.check().is_err());

        let mut policy = PasswordPolicySet::standard();
        policy.history = Some(MAX_HISTORY_DEPTH + 1);
        assert!(policy.check().is_err());
        policy.history = Some(MAX_HISTORY_DEPTH);
        assert_eq!(policy.check(), Ok(()));
    }

    #[test]
    fn stateful_fields_are_not_string_validated() {
        let policy = PasswordPolicySet {
            max_age_days: Some(30),
            history: Some(5),
            ..permissive()
        };
        // validate_all ignores stateful rules; they are enforced by the app.
        assert_eq!(policy.validate("anything", &ctx()), Ok(()));
    }

    #[test]
    fn stateful_fields_deserialize() {
        let policy: PasswordPolicySet =
            serde_json::from_str(r#"{ "maxAgeDays": 90, "history": 5 }"#).unwrap();
        assert_eq!(policy.max_age_days, Some(90));
        assert_eq!(policy.history, Some(5));
    }

    /// A policy with every rule disabled, for testing rules in isolation.
    fn permissive() -> PasswordPolicySet {
        PasswordPolicySet {
            min_length: None,
            max_length: None,
            lowercase: false,
            uppercase: false,
            digits: false,
            special_chars: false,
            not_username: false,
            not_email: false,
            banned_passwords: false,
            regex_pattern: None,
            max_repeats: None,
            max_age_days: None,
            history: None,
        }
    }
}
