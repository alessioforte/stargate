use serde_json::Value;
use std::collections::HashSet;

pub fn client_is_first_party(attrs: &Value) -> bool {
    ["first_party", "firstParty", "trusted"]
        .into_iter()
        .any(|key| attrs.get(key).and_then(Value::as_bool) == Some(true))
}

pub fn consent_covers(
    granted_scopes: &[String],
    granted_audiences: &[String],
    requested_scopes: &[String],
    requested_audience: Option<&str>,
) -> bool {
    let granted_scopes = granted_scopes
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if !requested_scopes
        .iter()
        .all(|scope| granted_scopes.contains(scope.as_str()))
    {
        return false;
    }

    let Some(audience) = requested_audience else {
        return true;
    };

    granted_audiences.iter().any(|granted| granted == audience)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_party_accepts_legacy_trusted_attr_names() {
        assert!(client_is_first_party(
            &serde_json::json!({"first_party": true})
        ));
        assert!(client_is_first_party(
            &serde_json::json!({"firstParty": true})
        ));
        assert!(client_is_first_party(&serde_json::json!({"trusted": true})));
    }

    #[test]
    fn consent_requires_all_requested_scopes_and_audience() {
        let granted_scopes = vec!["openid".to_string(), "email".to_string()];
        let granted_audiences = vec!["gateway".to_string()];

        assert!(consent_covers(
            &granted_scopes,
            &granted_audiences,
            &["openid".to_string()],
            Some("gateway")
        ));
        assert!(!consent_covers(
            &granted_scopes,
            &granted_audiences,
            &["profile".to_string()],
            Some("gateway")
        ));
    }
}
