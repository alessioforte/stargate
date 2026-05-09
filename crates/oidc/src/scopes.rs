use crate::OAuthError;
use std::collections::HashSet;

pub const GRANT_CLIENT_CREDENTIALS: &str = "client_credentials";
pub const GRANT_AUTHORIZATION_CODE: &str = "authorization_code";
pub const GRANT_REFRESH_TOKEN: &str = "refresh_token";
pub const AUTH_METHOD_CLIENT_SECRET_BASIC: &str = "client_secret_basic";
pub const AUTH_METHOD_CLIENT_SECRET_POST: &str = "client_secret_post";
pub const AUTH_METHOD_NONE: &str = "none";
pub const RESPONSE_CODE: &str = "code";
pub const SCOPE_OPENID: &str = "openid";
pub const SCOPE_EMAIL: &str = "email";
pub const SCOPE_PROFILE: &str = "profile";
pub const SCOPE_OFFLINE_ACCESS: &str = "offline_access";
pub const PKCE_METHOD_S256: &str = "S256";
pub const AUTHORIZATION_CODE_TTL_SECS: i64 = 600;
pub const OAUTH_TOKENS_GRANT: &str = "oauth_tokens";
pub const OAUTH_INTROSPECT_SCOPE: &str = "oauth:introspect";
pub const OAUTH_REVOKE_SCOPE: &str = "oauth:revoke";
pub const OAUTH_CAN_INTROSPECT_ATTR: &str = "can_introspect";
pub const OAUTH_CAN_REVOKE_ATTR: &str = "can_revoke";

pub fn parse_space_delimited(value: Option<&str>, field: &str) -> Result<Vec<String>, OAuthError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in value.split_whitespace() {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if seen.insert(item.to_string()) {
            out.push(item.to_string());
        }
    }

    if out
        .iter()
        .any(|item| item.contains('"') || item.contains('\\'))
    {
        return Err(OAuthError::invalid_request(format!(
            "{field} contains unsupported characters"
        )));
    }

    Ok(out)
}

pub fn resolve_scopes(
    requested_scope: Option<&str>,
    allowed_scopes: &[String],
) -> Result<Vec<String>, OAuthError> {
    let requested = parse_space_delimited(requested_scope, "scope")?;
    if requested.is_empty() {
        return Ok(Vec::new());
    }

    let allowed = allowed_scopes
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    for scope in &requested {
        if !allowed.contains(scope.as_str()) {
            return Err(OAuthError::invalid_scope(format!(
                "requested scope '{scope}' is not allowed for this client"
            )));
        }
    }

    Ok(requested)
}

pub fn resolve_audience(
    requested_audience: Option<&str>,
    allowed_audiences: &[String],
) -> Result<Option<String>, OAuthError> {
    let requested = requested_audience
        .map(str::trim)
        .filter(|audience| !audience.is_empty());

    if let Some(audience) = requested {
        if audience.split_whitespace().count() != 1 {
            return Err(OAuthError::invalid_request(
                "audience must be a single value",
            ));
        }
        if allowed_audiences.is_empty() {
            return Err(OAuthError::invalid_request(
                "client has no registered audiences for requested audience",
            ));
        }
        if !allowed_audiences.iter().any(|allowed| allowed == audience) {
            return Err(OAuthError::invalid_request(format!(
                "requested audience '{audience}' is not allowed for this client"
            )));
        }
        return Ok(Some(audience.to_string()));
    }

    if allowed_audiences.len() == 1 {
        Ok(Some(allowed_audiences[0].clone()))
    } else {
        Ok(None)
    }
}

pub fn contains(scopes: &[String], scope: &str) -> bool {
    scopes.iter().any(|candidate| candidate == scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_scopes_accepts_registered_subset() {
        let allowed = vec!["read:users".to_string(), "write:users".to_string()];
        let scopes = resolve_scopes(Some("read:users read:users"), &allowed).unwrap();

        assert_eq!(scopes, vec!["read:users"]);
    }

    #[test]
    fn resolve_audience_defaults_single_registered_audience() {
        let audience = resolve_audience(None, &["gateway".to_string()]).unwrap();

        assert_eq!(audience.as_deref(), Some("gateway"));
    }
}
