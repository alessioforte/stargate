use anyhow::{Context, Result, bail, ensure};
use db::InstanceBootstrapResult;
use db::ent::{
    OAuthClient, Profile, TrustedAuditBoundary, TrustedAuditContext, TrustedBackgroundActor,
};
use tracing::info;
use url::Url;

pub const SUPER_ADMIN_ROLE: &str = "super_admin";
pub const DEFAULT_CONSOLE_OAUTH_CLIENT_ID: &str = "stargate_console";
const DEFAULT_CONSOLE_APP_BASE_PATH: &str = "/stargate";

pub async fn super_admin_exists() -> Result<bool> {
    crate::data::db::super_admin_exists().await
}

pub async fn is_super_admin_user_id(user_id: &str) -> Result<bool> {
    crate::data::db::is_super_admin_user_id(user_id).await
}

pub fn resolve_console_oauth_client_id(explicit: Option<String>) -> Result<String> {
    let client_id = explicit
        .or_else(|| nonempty_env("CONSOLE_OAUTH_CLIENT_ID"))
        .unwrap_or_else(|| DEFAULT_CONSOLE_OAUTH_CLIENT_ID.to_string());
    let client_id = client_id.trim().to_string();

    ensure!(
        !client_id.is_empty()
            && client_id.len() <= 128
            && client_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')),
        "console OAuth client ID must contain only ASCII letters, numbers, '.', '-', or '_'"
    );
    Ok(client_id)
}

pub fn resolve_console_oauth_redirect_uri(explicit: Option<String>) -> Result<String> {
    let redirect_uri = explicit
        .or_else(|| nonempty_env("CONSOLE_OAUTH_REDIRECT_URI"))
        .unwrap_or_else(default_console_oauth_redirect_uri);
    validate_console_oauth_redirect_uri(&redirect_uri)?;
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
            let hash = crate::etc::auth::password::hash_password(password)
                .await
                .context("failed to hash bootstrap password")?;
            let nickname = nickname.unwrap_or_else(|| email.clone());
            if !db::ent::nickname_is_valid(&nickname, &email) {
                bail!(
                    "nickname must not be empty and may contain '@' only as the account's own email"
                );
            }
            (
                Some(Profile::new(email, nickname).given_name(name)),
                Some(hash),
            )
        }
        (None, None) => (None, None),
        _ => bail!("bootstrap requires both an email and a password for the first super admin"),
    };

    let post_logout_redirect_uri = console_post_logout_redirect_uri(&redirect_uri);
    let mut client = OAuthClient::new(
        client_id.clone(),
        None,
        "Stargate Console".to_string(),
        Some("Built-in public client for the Stargate Console".to_string()),
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
            "system": true,
        }),
    );
    client.post_logout_redirect_uris.0 = post_logout_redirect_uri.into_iter().collect();
    let result = crate::data::db::bootstrap_instance(
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
        info!("Console OAuth client '{}' bootstrapped", client_id);
    }
    Ok(result)
}

/// The Console root, where it lands after RP-Initiated Logout: its callback
/// URI without `/auth/callback` (the Console derives the same value).
fn console_post_logout_redirect_uri(redirect_uri: &str) -> Option<String> {
    redirect_uri
        .strip_suffix("/auth/callback")
        .map(|base| format!("{base}/"))
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn default_console_oauth_redirect_uri() -> String {
    let public_url = nonempty_env("CONSOLE_PUBLIC_URL")
        .or_else(|| nonempty_env("OAUTH_BASE_URL"))
        .unwrap_or_else(jwt::issuer_from_env);
    let base_path = nonempty_env("CONSOLE_APP_BASE_PATH")
        .map(|value| normalize_base_path(&value))
        .unwrap_or_else(|| DEFAULT_CONSOLE_APP_BASE_PATH.to_string());
    format!(
        "{}{}/auth/callback",
        public_url.trim_end_matches('/'),
        base_path
    )
}

fn normalize_base_path(value: &str) -> String {
    let path = value.trim().trim_matches('/');
    if path.is_empty() {
        DEFAULT_CONSOLE_APP_BASE_PATH.to_string()
    } else {
        format!("/{path}")
    }
}

fn validate_console_oauth_redirect_uri(redirect_uri: &str) -> Result<()> {
    ensure!(
        redirect_uri == redirect_uri.trim()
            && !redirect_uri.contains('*')
            && !redirect_uri.contains('#')
            && !redirect_uri.chars().any(char::is_whitespace),
        "console OAuth redirect URI is invalid"
    );
    let parsed = Url::parse(redirect_uri).context("console OAuth redirect URI must be absolute")?;
    let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    ensure!(
        parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback),
        "console OAuth redirect URI must use HTTPS; HTTP is allowed only for loopback hosts"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_post_logout_redirect_uri_is_the_console_root() {
        assert_eq!(
            console_post_logout_redirect_uri("https://id.example.com/stargate/auth/callback")
                .as_deref(),
            Some("https://id.example.com/stargate/")
        );
        assert_eq!(
            console_post_logout_redirect_uri("https://id.example.com/callback"),
            None
        );
    }

    #[test]
    fn validates_console_client_id() {
        assert_eq!(
            resolve_console_oauth_client_id(Some("stargate_console".to_string())).unwrap(),
            "stargate_console"
        );
        assert!(resolve_console_oauth_client_id(Some("bad client".to_string())).is_err());
    }

    #[test]
    fn redirect_uri_allows_https_and_loopback_http() {
        assert!(
            validate_console_oauth_redirect_uri(
                "https://identity.example.com/stargate/auth/callback"
            )
            .is_ok()
        );
        assert!(
            validate_console_oauth_redirect_uri("http://localhost:5050/stargate/auth/callback")
                .is_ok()
        );
        assert!(
            validate_console_oauth_redirect_uri(
                "http://identity.example.com/stargate/auth/callback"
            )
            .is_err()
        );
    }
}
