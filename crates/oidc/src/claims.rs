use crate::scopes::{SCOPE_EMAIL, SCOPE_PROFILE, contains};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserClaimsProfile {
    pub id: String,
    pub email: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: String,
    pub picture: Option<String>,
}

fn format_name(given_name: &str, family_name: &str) -> String {
    if given_name.is_empty() && family_name.is_empty() {
        return "Anonymous".to_string();
    }
    if given_name.is_empty() {
        return family_name.to_string();
    }
    if family_name.is_empty() {
        return given_name.to_string();
    }
    format!("{given_name} {family_name}").trim().to_string()
}

pub fn client_credentials_access_claims(
    client_id: &str,
    scope: Option<String>,
    audience: Option<String>,
) -> jwt::Claims {
    let mut claims = jwt::Claims::default().subject(client_id.to_string());
    claims.azp = Some(client_id.to_string());
    claims.scope = scope;
    claims.aud = audience;
    claims
}

pub fn user_access_claims(
    user: &UserClaimsProfile,
    client_id: &str,
    sid: Option<&str>,
    scope: Option<String>,
    audience: Option<String>,
    auth_time: chrono::DateTime<chrono::Utc>,
) -> jwt::Claims {
    let mut claims = jwt::Claims::default()
        .subject(user.id.clone())
        .sub_id(user.id.clone());
    claims.azp = Some(client_id.to_string());
    claims.sid = sid.map(str::to_string);
    claims.scope = scope;
    claims.aud = audience;
    claims.auth_time = Some(auth_time.timestamp() as usize);
    claims
}

pub fn id_token_claims(
    user: &UserClaimsProfile,
    client_id: &str,
    sid: Option<&str>,
    scopes: &[String],
    auth_time: chrono::DateTime<chrono::Utc>,
    nonce: Option<String>,
) -> jwt::Claims {
    let mut claims = jwt::Claims::default()
        .subject(user.id.clone())
        .sub_id(user.id.clone())
        .aud(client_id.to_string());
    claims.azp = Some(client_id.to_string());
    claims.sid = sid.map(str::to_string);
    claims.auth_time = Some(auth_time.timestamp() as usize);
    claims.nonce = nonce;

    if contains(scopes, SCOPE_EMAIL) {
        claims.email = Some(user.email.clone());
        claims.email_verified = Some(true);
    }

    if contains(scopes, SCOPE_PROFILE) {
        let given_name = user.given_name.clone().unwrap_or_default();
        let family_name = user.family_name.clone().unwrap_or_default();
        claims.name = Some(format_name(&given_name, &family_name));
        claims.preferred_username = Some(user.nickname.clone());
        claims.picture = user.picture.clone();
    }

    claims
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scopes::{SCOPE_EMAIL, SCOPE_OPENID, SCOPE_PROFILE};
    use chrono::Utc;

    fn user() -> UserClaimsProfile {
        UserClaimsProfile {
            id: "user-1".to_string(),
            email: "alice@example.com".to_string(),
            given_name: Some("Alice".to_string()),
            family_name: Some("Example".to_string()),
            nickname: "alice".to_string(),
            picture: Some("https://app.example.com/alice.png".to_string()),
        }
    }

    #[test]
    fn id_claims_filter_by_scope() {
        let claims = id_token_claims(
            &user(),
            "client-1",
            Some("session-1"),
            &[SCOPE_OPENID.to_string()],
            Utc::now(),
            None,
        );

        assert_eq!(claims.sid.as_deref(), Some("session-1"));
        assert!(claims.email.is_none());
        assert!(claims.name.is_none());
    }

    #[test]
    fn id_claims_include_email_and_profile_when_scoped() {
        let claims = id_token_claims(
            &user(),
            "client-1",
            Some("session-1"),
            &[
                SCOPE_OPENID.to_string(),
                SCOPE_EMAIL.to_string(),
                SCOPE_PROFILE.to_string(),
            ],
            Utc::now(),
            Some("nonce-1".to_string()),
        );

        assert_eq!(claims.email.as_deref(), Some("alice@example.com"));
        assert_eq!(claims.email_verified, Some(true));
        assert_eq!(claims.name.as_deref(), Some("Alice Example"));
        assert_eq!(claims.preferred_username.as_deref(), Some("alice"));
        assert_eq!(
            claims.picture.as_deref(),
            Some("https://app.example.com/alice.png")
        );
        assert_eq!(claims.nonce.as_deref(), Some("nonce-1"));
    }
}
