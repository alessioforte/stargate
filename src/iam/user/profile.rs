use db::ent::User;
use serde_json::Value;
use tools::Locale;

pub fn format_name(given_name: &str, family_name: &str) -> String {
    if given_name.is_empty() && family_name.is_empty() {
        return "Anonymous".to_string();
    }
    if given_name.is_empty() {
        return family_name.to_string();
    }

    if family_name.is_empty() {
        return given_name.to_string();
    }

    format!("{} {}", given_name, family_name).trim().to_string()
}

/// The validated locale stored in `users.attrs.locale`, if any.
pub fn locale(user: &User) -> Option<Locale> {
    user.attrs
        .get("locale")
        .and_then(Value::as_str)
        .and_then(Locale::parse)
}

/// Store a canonical locale in a user's attrs, preserving every other key.
pub fn with_locale(mut attrs: Value, locale: Locale) -> Value {
    if !attrs.is_object() {
        attrs = Value::Object(serde_json::Map::new());
    }
    attrs.as_object_mut().expect("attrs is an object").insert(
        "locale".to_string(),
        Value::String(locale.as_str().to_string()),
    );
    attrs
}

/// Email locale order: the account's saved language first (even for anonymous
/// flows), then the request's `Accept-Language`, then the installation
/// default (which itself falls back to English).
pub fn resolve_email_locale(
    account: Option<Locale>,
    requested: Option<Locale>,
    default: Locale,
) -> Locale {
    account.or(requested).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::{locale, resolve_email_locale, with_locale};
    use serde_json::json;
    use tools::Locale;

    #[test]
    fn stores_and_reads_a_validated_locale() {
        let user = db::ent::User::new("alice@example.com".to_string(), "alice".to_string())
            .attrs(json!({ "default_org": "org-1" }));
        let attrs = with_locale(user.attrs.clone(), Locale::It);
        let user = user.attrs(attrs);

        assert_eq!(locale(&user), Some(Locale::It));
        assert_eq!(user.attrs["default_org"], "org-1");
    }

    #[test]
    fn ignores_unknown_stored_locales() {
        let user = db::ent::User::new("alice@example.com".to_string(), "alice".to_string())
            .attrs(json!({ "locale": "fr" }));

        assert_eq!(locale(&user), None);
    }

    #[test]
    fn account_locale_wins_in_anonymous_flows() {
        assert_eq!(
            resolve_email_locale(Some(Locale::It), Some(Locale::En), Locale::En),
            Locale::It
        );
    }

    #[test]
    fn request_then_default_then_english() {
        assert_eq!(
            resolve_email_locale(None, Some(Locale::It), Locale::En),
            Locale::It
        );
        assert_eq!(resolve_email_locale(None, None, Locale::It), Locale::It);
        assert_eq!(
            resolve_email_locale(None, None, Locale::FALLBACK),
            Locale::En
        );
    }
}
