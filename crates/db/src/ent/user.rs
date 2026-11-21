use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Longest address the schema stores (`VARCHAR(254)`). RFC 5321 §4.5.3.1.3
/// limits a forward-path to 256 octets including the angle brackets, leaving
/// 254 for the address itself.
pub const MAX_EMAIL_LEN: usize = 254;

/// The stored and compared form of an email address. One mailbox is one
/// account, whatever casing is used at signup, login, or recovery.
pub fn canonical_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Whether the canonical address fits the column. SQLite ignores `VARCHAR`
/// lengths, so every backend checks this before writing.
pub fn email_fits_column(raw: &str) -> bool {
    canonical_email(raw).len() <= MAX_EMAIL_LEN
}

/// The stored and compared form of a nickname: handles match
/// case-insensitively, so `Admin` and `admin` are one handle.
pub fn canonical_nickname(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Nicknames and emails share the login field, so a nickname may look like an
/// email address only when it is the account's own address: no account can
/// take another person's email as its handle.
pub fn nickname_is_valid(nickname: &str, email: &str) -> bool {
    let nickname = canonical_nickname(nickname);
    !nickname.is_empty() && (!nickname.contains('@') || nickname == canonical_email(email))
}

#[cfg(test)]
mod identity_tests {
    use super::{MAX_EMAIL_LEN, canonical_nickname, email_fits_column, nickname_is_valid};

    #[test]
    fn an_address_fits_the_column_up_to_254_octets() {
        assert!(email_fits_column("ada@example.com"));
        assert!(email_fits_column(&format!(
            "{}@{}",
            "a".repeat(64),
            "b".repeat(MAX_EMAIL_LEN - 64 - 1)
        )));
        assert!(!email_fits_column(&format!(
            "{}@{}",
            "a".repeat(64),
            "b".repeat(MAX_EMAIL_LEN - 64)
        )));
    }

    #[test]
    fn nicknames_are_case_insensitive_and_cannot_claim_another_address() {
        assert_eq!(canonical_nickname("  Admin "), "admin");
        assert!(nickname_is_valid("Alice", "alice@example.com"));
        assert!(nickname_is_valid("Alice@Example.com", "alice@example.com"));
        assert!(!nickname_is_valid(
            "victim@example.com",
            "mallory@example.com"
        ));
        assert!(!nickname_is_valid("   ", "alice@example.com"));
    }
}

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub email: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: String,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub attrs: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    pub fn new(email: String, nickname: String) -> Self {
        let id = ulid::Ulid::generate().to_string();
        let now = Utc::now();
        User {
            id,
            email,
            given_name: None,
            family_name: None,
            nickname,
            picture: None,
            phone_number: None,
            attrs: Value::Object(serde_json::Map::new()),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn given_name(mut self, given_name: Option<String>) -> Self {
        self.given_name = given_name;
        self
    }

    pub fn family_name(mut self, family_name: Option<String>) -> Self {
        self.family_name = family_name;
        self
    }

    pub fn email(mut self, email: String) -> Self {
        self.email = email;
        self
    }

    pub fn nickname(mut self, nickname: String) -> Self {
        self.nickname = nickname;
        self
    }

    pub fn picture(mut self, picture: Option<String>) -> Self {
        self.picture = picture;
        self
    }

    pub fn phone_number(mut self, phone_number: Option<String>) -> Self {
        self.phone_number = phone_number;
        self
    }

    pub fn attrs(mut self, attrs: Value) -> Self {
        self.attrs = attrs;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub email: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: String,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    pub attrs: Value,
}

impl Profile {
    pub fn new(email: String, nickname: String) -> Self {
        Profile {
            email,
            given_name: None,
            family_name: None,
            nickname,
            picture: None,
            phone_number: None,
            attrs: Value::Object(serde_json::Map::new()),
        }
    }

    pub fn given_name(mut self, given_name: Option<String>) -> Self {
        self.given_name = given_name;
        self
    }

    pub fn family_name(mut self, family_name: Option<String>) -> Self {
        self.family_name = family_name;
        self
    }

    pub fn nickname(mut self, nickname: String) -> Self {
        self.nickname = nickname;
        self
    }

    pub fn picture(mut self, picture: Option<String>) -> Self {
        self.picture = picture;
        self
    }

    pub fn phone_number(mut self, phone_number: Option<String>) -> Self {
        self.phone_number = phone_number;
        self
    }

    pub fn attrs(mut self, attrs: Value) -> Self {
        self.attrs = attrs;
        self
    }
}
