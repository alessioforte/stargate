use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::error::Error;

#[derive(Deserialize)]
pub struct OAuthResponse {
    pub access_token: String,
}

/// The Google account behind an access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleUser {
    pub id: String,
    /// Profile fields Google omits when the account has none (no family
    /// name, ...) or the `profile` scope was not granted.
    pub name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub picture: Option<String>,
    /// The account's email, when Google has verified it (scope `email`).
    pub verified_email: Option<String>,
}

/// The `userinfo` v2 response.
#[derive(Deserialize)]
struct GoogleUserInfo {
    id: String,
    email: Option<String>,
    #[serde(default)]
    verified_email: bool,
    name: Option<String>,
    given_name: Option<String>,
    family_name: Option<String>,
    picture: Option<String>,
}

pub async fn get_google_oauth_token(
    authorization_code: &str,
) -> Result<OAuthResponse, Box<dyn Error + Send + Sync>> {
    let redirect_uri = env::var("GOOGLE_OAUTH_REDIRECT_URI")
        .unwrap_or_else(|_| "http://localhost:5050/oauth/google".to_string());
    let client_secret = env::var("GOOGLE_OAUTH_CLIENT_SECRET")
        .map_err(|_| "Google OAuth not configured: missing GOOGLE_OAUTH_CLIENT_SECRET")?;
    let client_id = env::var("GOOGLE_OAUTH_CLIENT_ID")
        .map_err(|_| "Google OAuth not configured: missing GOOGLE_OAUTH_CLIENT_ID")?;

    let root_url = google_token_url();

    let params = [
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", client_id.as_str()),
        ("code", authorization_code),
        ("client_secret", client_secret.as_str()),
    ];

    let response = Client::new().post(&root_url).form(&params).send().await?;

    if response.status().is_success() {
        Ok(response.json::<OAuthResponse>().await?)
    } else {
        let res: serde_json::Value = response.json().await?;
        tracing::error!("get_google_oauth_token: {}", res);
        Err("An error occurred while trying to retrieve access token.".into())
    }
}

/// The account behind `access_token`, from the `userinfo` endpoint.
pub async fn get_google_user(
    access_token: &str,
) -> Result<GoogleUser, Box<dyn Error + Send + Sync>> {
    let response = Client::new()
        .get(google_user_url())
        .bearer_auth(access_token)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        tracing::error!(%status, "get_google_user: {body}");
        return Err(format!("Google userinfo answered {status}").into());
    }
    let info = response.json::<GoogleUserInfo>().await?;
    let present = |value: Option<String>| value.filter(|value| !value.trim().is_empty());
    Ok(GoogleUser {
        id: info.id,
        name: present(info.name),
        given_name: present(info.given_name),
        family_name: present(info.family_name),
        picture: present(info.picture),
        verified_email: present(info.email).filter(|_| info.verified_email),
    })
}

fn google_token_url() -> String {
    env::var("GOOGLE_OAUTH_TOKEN_URL")
        .unwrap_or_else(|_| "https://www.googleapis.com/oauth2/v4/token".to_string())
}

fn google_user_url() -> String {
    env::var("GOOGLE_OAUTH_USER_URL")
        .unwrap_or_else(|_| "https://www.googleapis.com/oauth2/v2/userinfo".to_string())
}

#[cfg(test)]
// `env_lock` intentionally serializes process-env access for the whole async
// test; the guard must stay live across the `.await`s. Each `#[tokio::test]`
// runs on its own single-threaded runtime, so there is no deadlock risk.
#[allow(clippy::await_holding_lock)]
mod tests {
    use std::collections::HashMap;

    use super::{GoogleUser, get_google_oauth_token, get_google_user};
    use crate::test_support::{ScopedEnv, env_lock, spawn_json_server};

    #[tokio::test]
    async fn google_oauth_token_posts_expected_form_data() {
        let _env_lock = env_lock();
        let (url, request_rx) = spawn_json_server(
            "/oauth2/v4/token",
            r#"{"access_token":"google_access","expires_in":3599,"token_type":"Bearer"}"#,
        )
        .await
        .unwrap();
        let _token_url = ScopedEnv::set("GOOGLE_OAUTH_TOKEN_URL", &url);
        let _redirect_uri = ScopedEnv::set(
            "GOOGLE_OAUTH_REDIRECT_URI",
            "https://app.local/oauth/google",
        );
        let _client_id = ScopedEnv::set("GOOGLE_OAUTH_CLIENT_ID", "google-client");
        let _client_secret = ScopedEnv::set("GOOGLE_OAUTH_CLIENT_SECRET", "google-secret");

        let token = get_google_oauth_token("oauth-code").await.unwrap();
        let request = request_rx.await.unwrap();
        let body: HashMap<String, String> = url::form_urlencoded::parse(&request.body)
            .into_owned()
            .collect();

        assert_eq!(token.access_token, "google_access");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/oauth2/v4/token");
        assert_eq!(
            request.header("content-type"),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(
            body.get("grant_type"),
            Some(&"authorization_code".to_string())
        );
        assert_eq!(
            body.get("redirect_uri"),
            Some(&"https://app.local/oauth/google".to_string())
        );
        assert_eq!(body.get("client_id"), Some(&"google-client".to_string()));
        assert_eq!(
            body.get("client_secret"),
            Some(&"google-secret".to_string())
        );
        assert_eq!(body.get("code"), Some(&"oauth-code".to_string()));
    }

    #[tokio::test]
    async fn google_user_uses_bearer_auth_and_keeps_the_verified_email() {
        let _env_lock = env_lock();
        let (url, request_rx) = spawn_json_server(
            "/oauth2/v2/userinfo",
            r#"{"id":"42","email":"ada@example.com","verified_email":true,"name":"Ada","given_name":"Ada","picture":"https://img"}"#,
        )
        .await
        .unwrap();
        let _user_url = ScopedEnv::set("GOOGLE_OAUTH_USER_URL", &url);

        let user = get_google_user("google_access").await.unwrap();
        let request = request_rx.await.unwrap();

        assert_eq!(
            user,
            GoogleUser {
                id: "42".to_string(),
                name: Some("Ada".to_string()),
                given_name: Some("Ada".to_string()),
                family_name: None,
                picture: Some("https://img".to_string()),
                verified_email: Some("ada@example.com".to_string()),
            }
        );
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/oauth2/v2/userinfo");
        assert_eq!(
            request.header("authorization"),
            Some("Bearer google_access")
        );
    }

    #[tokio::test]
    async fn an_unverified_google_email_is_not_used() {
        let _env_lock = env_lock();
        let (url, _request_rx) = spawn_json_server(
            "/oauth2/v2/userinfo",
            r#"{"id":"42","email":"ada@example.com","verified_email":false}"#,
        )
        .await
        .unwrap();
        let _user_url = ScopedEnv::set("GOOGLE_OAUTH_USER_URL", &url);

        let user = get_google_user("google_access").await.unwrap();

        assert_eq!(user.verified_email, None);
        assert_eq!(user.name, None);
    }
}
