use anyhow::{Context, Result, bail, ensure};
use db::InstanceBootstrapResult;
use db::ent::{
    OAuthClient, Profile, TrustedAuditBoundary, TrustedAuditContext, TrustedBackgroundActor,
};
use tracing::info;
use url::Url;

pub const SUPER_ADMIN_ROLE: &str = "super_admin";
pub const DEFAULT_ADMIN_OAUTH_CLIENT_ID: &str = "stargate_admin";
const DEFAULT_ADMIN_APP_BASE_PATH: &str = "/stargate";

pub async fn super_admin_exists() -> Result<bool> {
    crate::db::super_admin_exists().await
}

pub async fn is_super_admin_user_id(user_id: &str) -> Result<bool> {
    crate::db::is_super_admin_user_id(user_id).await
}

pub fn resolve_admin_oauth_client_id(explicit: Option<String>) -> Result<String> {
    let client_id = explicit
        .or_else(|| nonempty_env("ADMIN_OAUTH_CLIENT_ID"))
        .unwrap_or_else(|| DEFAULT_ADMIN_OAUTH_CLIENT_ID.to_string());
    let client_id = client_id.trim().to_string();

    ensure!(
        !client_id.is_empty()
            && client_id.len() <= 128
            && client_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')),
        "admin OAuth client ID must contain only ASCII letters, numbers, '.', '-', or '_'"
    );
    Ok(client_id)
}

pub fn resolve_admin_oauth_redirect_uri(explicit: Option<String>) -> Result<String> {
    let redirect_uri = explicit
        .or_else(|| nonempty_env("ADMIN_OAUTH_REDIRECT_URI"))
        .unwrap_or_else(default_admin_oauth_redirect_uri);
    validate_admin_oauth_redirect_uri(&redirect_uri)?;
    Ok(redirect_uri)
}

pub async fn bootstrap_instance(
    email: Option<String>,
    password: Option<String>,
    name: Option<String>,
    nickname: Option<String>,
    client_id: String,
    redirect_uri: String,
) -> Result<InstanceBootstrapResult> {
    let (profile, password_hash) = match (email, password) {
        (Some(email), Some(password)) => {
            if password.trim().is_empty() {
                bail!("password must not be empty");
            }
            let hash = crate::etc::pw::hash_password(password)
                .await
                .context("failed to hash bootstrap password")?;
            let nickname = nickname.unwrap_or_else(|| email.clone());
            (
                Some(Profile::new(email, nickname).given_name(name)),
                Some(hash),
            )
        }
        (None, None) => (None, None),
        _ => bail!("bootstrap requires both an email and a password for the first super admin"),
    };

    let client = OAuthClient::new(
        client_id.clone(),
        None,
        "Stargate Admin".to_string(),
        Some("Built-in public client for the Stargate Admin UI".to_string()),
        "none".to_string(),
        vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        vec!["code".to_string()],
        vec![redirect_uri],
        vec![
            "openid".to_string(),
            "email".to_string(),
            "profile".to_string(),
            "offline_access".to_string(),
        ],
        Vec::new(),
        serde_json::json!({
            "first_party": true,
            "trusted": true,
            "system": true,
        }),
    );
    let result = crate::db::bootstrap_instance(
        profile,
        password_hash.as_deref(),
        client,
        TrustedAuditContext::background(
            TrustedAuditBoundary::control_plane(),
            TrustedBackgroundActor::system(None),
        ),
    )
    .await?;

    if result.super_admin_created {
        info!("First super admin bootstrapped");
    }
    if result.oauth_client_created {
        info!("Admin OAuth client '{}' bootstrapped", client_id);
    }
    Ok(result)
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn default_admin_oauth_redirect_uri() -> String {
    let public_url = nonempty_env("ADMIN_PUBLIC_URL")
        .or_else(|| nonempty_env("OAUTH_BASE_URL"))
        .unwrap_or_else(jwt::issuer_from_env);
    let base_path = nonempty_env("ADMIN_APP_BASE_PATH")
        .map(|value| normalize_base_path(&value))
        .unwrap_or_else(|| DEFAULT_ADMIN_APP_BASE_PATH.to_string());
    format!(
        "{}{}/auth/callback",
        public_url.trim_end_matches('/'),
        base_path
    )
}

fn normalize_base_path(value: &str) -> String {
    let path = value.trim().trim_matches('/');
    if path.is_empty() {
        DEFAULT_ADMIN_APP_BASE_PATH.to_string()
    } else {
        format!("/{path}")
    }
}

fn validate_admin_oauth_redirect_uri(redirect_uri: &str) -> Result<()> {
    ensure!(
        redirect_uri == redirect_uri.trim()
            && !redirect_uri.contains('*')
            && !redirect_uri.contains('#')
            && !redirect_uri.chars().any(char::is_whitespace),
        "admin OAuth redirect URI is invalid"
    );
    let parsed = Url::parse(redirect_uri).context("admin OAuth redirect URI must be absolute")?;
    let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    ensure!(
        parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback),
        "admin OAuth redirect URI must use HTTPS; HTTP is allowed only for loopback hosts"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_admin_client_id() {
        assert_eq!(
            resolve_admin_oauth_client_id(Some("stargate_admin".to_string())).unwrap(),
            "stargate_admin"
        );
        assert!(resolve_admin_oauth_client_id(Some("bad client".to_string())).is_err());
    }

    #[test]
    fn redirect_uri_allows_https_and_loopback_http() {
        assert!(
            validate_admin_oauth_redirect_uri(
                "https://identity.example.com/stargate/auth/callback"
            )
            .is_ok()
        );
        assert!(
            validate_admin_oauth_redirect_uri("http://localhost:5050/stargate/auth/callback")
                .is_ok()
        );
        assert!(
            validate_admin_oauth_redirect_uri("http://identity.example.com/stargate/auth/callback")
                .is_err()
        );
    }
}
