use std::collections::HashSet;
use std::sync::LazyLock;

/// Built-in list of very common passwords, matched case-insensitively.
///
/// This is a small last-line-of-defense list; production deployments should
/// extend it with a real breached/common-password list (e.g. a top-10k file)
/// via [`BannedPasswords::with_extra`]. Entries must be lowercase.
const BUILTIN: &[&str] = &[
    // Most common passwords (rockyou-style top entries).
    "123456", "password", "123456789", "12345678", "12345", "1234567",
    "1234567890", "qwerty", "abc123", "111111", "123123", "654321",
    "666666", "121212", "000000", "00000000", "555555", "696969",
    "7777777", "888888", "112233", "123321", "159753", "222222",
    "qwertyuiop", "qwerty123", "1q2w3e4r", "1qaz2wsx", "zaq12wsx",
    "qazwsx", "asdfgh", "asdfghjkl", "zxcvbnm", "123qwe", "q1w2e3r4",
    "iloveyou", "letmein", "monkey", "dragon", "sunshine", "princess",
    "football", "baseball", "soccer", "superman", "batman", "starwars",
    "trustno1", "master", "shadow", "welcome", "freedom", "whatever",
    "secret", "internet", "computer", "matrix", "cookie", "pepper",
    "ginger", "summer", "flower", "lovely", "loveme", "hottie",
    "charlie", "michael", "jessica", "ashley", "nicole", "daniel",
    "andrew", "michelle", "jordan", "hunter", "harley", "buster",
    "tigger", "access", "banana", "killer", "mustang", "maggie",
    "hello", "hello123", "test", "test123", "testing", "temp123",
    "admin", "admin123", "administrator", "root", "toor", "user",
    "guest", "default", "changeme", "letmein1", "welcome1", "welcome123",
    // Composition-rule-passing variants: these satisfy upper+lower+digit+
    // special checks, which is exactly why they must be banned explicitly.
    "password1", "password123", "password1!", "password123!", "p@ssword",
    "p@ssword1", "p@ssw0rd", "p@ssw0rd1", "passw0rd", "passw0rd!",
    "pa$$w0rd", "pa55word", "welcome1!", "admin@123", "admin123!",
    "qwerty123!", "abc123456", "aa123456", "abcd1234", "a1b2c3d4",
    "iloveyou1", "sunshine1", "charlie1", "monkey123", "dragon123",
];

static DEFAULT: LazyLock<BannedPasswords> = LazyLock::new(BannedPasswords::builtin);

/// A banned-password list: the built-in common passwords, optionally extended
/// with operator-supplied entries. Lookups are case-insensitive.
#[derive(Debug)]
pub struct BannedPasswords {
    set: HashSet<String>,
}

impl BannedPasswords {
    /// The built-in list only.
    pub fn builtin() -> Self {
        Self {
            set: BUILTIN.iter().map(|p| p.to_string()).collect(),
        }
    }

    /// The built-in list extended with additional entries
    /// (stored lowercase; blank entries are ignored).
    pub fn with_extra<I>(extra: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let mut banned = Self::builtin();
        for entry in extra {
            let entry = entry.as_ref().trim();
            if !entry.is_empty() {
                banned.set.insert(entry.to_lowercase());
            }
        }
        banned
    }

    /// Shared instance of the built-in list.
    pub fn default_list() -> &'static BannedPasswords {
        &DEFAULT
    }

    pub fn contains(&self, password: &str) -> bool {
        self.set.contains(&password.to_lowercase())
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_matches_case_insensitively() {
        let banned = BannedPasswords::builtin();
        assert!(banned.contains("password"));
        assert!(banned.contains("PASSWORD"));
        assert!(banned.contains("P@ssw0rd"));
        assert!(!banned.contains("correct horse battery staple"));
    }

    #[test]
    fn extra_entries_are_included() {
        let banned = BannedPasswords::with_extra(["CompanyName2026", "  ", ""]);
        assert!(banned.contains("companyname2026"));
        assert!(banned.contains("password"));
        assert!(!banned.contains(""));
    }
}
