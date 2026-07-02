use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::error::Error;

#[derive(Deserialize)]
pub struct GitHubOauthToken {
    pub access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubUserResult {
    pub id: i64,
    pub name: String,
    pub bio: Option<String>,
    pub login: String,
    pub avatar_url: String,
    pub email: String,
    pub email_verified: bool,
}

pub async fn get_github_oauth_token(
    authorization_code: &str,
) -> Result<GitHubOauthToken, Box<dyn Error + Send + Sync>> {
    let client_secret = env::var("GITHUB_OAUTH_CLIENT_SECRET")
        .map_err(|_| "GitHub OAuth not configured: missing GITHUB_OAUTH_CLIENT_SECRET")?;
    let client_id = env::var("GITHUB_OAUTH_CLIENT_ID")
        .map_err(|_| "GitHub OAuth not configured: missing GITHUB_OAUTH_CLIENT_ID")?;

    let root_url = github_token_url();

    let params = [
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("code", authorization_code),
        ("accept", "json"),
    ];

    let response = Client::new()
        .post(&root_url)
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json::<GitHubOauthToken>().await?)
    } else {
        let res: serde_json::Value = response.json().await?;
        tracing::error!("get_github_oauth_token: {}", res);
        Err("An error occurred while trying to retrieve the access token.".into())
    }
}

pub async fn get_github_user(
    access_token: &str,
) -> Result<GitHubUserResult, Box<dyn Error + Send + Sync>> {
    let root_url = github_user_url();

    let response = Client::new()
        .get(&root_url)
        .bearer_auth(access_token)
        .header("User-Agent", "stargate")
        .send()
        .await?;

    if response.status().is_success() {
        Ok(response.json::<GitHubUserResult>().await?)
    } else {
        let res: serde_json::Value = response.json().await?;
        tracing::error!("get_github_user: {}", res);
        Err("An error occurred while trying to retrieve user information.".into())
    }
}

fn github_token_url() -> String {
    env::var("GITHUB_OAUTH_TOKEN_URL")
        .unwrap_or_else(|_| "https://github.com/login/oauth/access_token".to_string())
}

fn github_user_url() -> String {
    env::var("GITHUB_OAUTH_USER_URL").unwrap_or_else(|_| "https://api.github.com/user".to_string())
}

#[cfg(test)]
// `env_lock` intentionally serializes process-env access for the whole async
// test; the guard must stay live across the `.await`s. Each `#[tokio::test]`
// runs on its own single-threaded runtime, so there is no deadlock risk.
#[allow(clippy::await_holding_lock)]
mod tests {
    use std::collections::HashMap;

    use super::{get_github_oauth_token, get_github_user};
    use crate::test_support::{ScopedEnv, env_lock, spawn_json_server};

    #[tokio::test]
    async fn github_oauth_token_posts_form_and_decodes_json() {
        let _env_lock = env_lock();
        let (url, request_rx) = spawn_json_server(
            "/login/oauth/access_token",
            r#"{"access_token":"gh_access"}"#,
        )
        .await
        .unwrap();
        let _token_url = ScopedEnv::set("GITHUB_OAUTH_TOKEN_URL", &url);
        let _client_id = ScopedEnv::set("GITHUB_OAUTH_CLIENT_ID", "client-id");
        let _client_secret = ScopedEnv::set("GITHUB_OAUTH_CLIENT_SECRET", "client-secret");

        let token = get_github_oauth_token("oauth-code").await.unwrap();
        let request = request_rx.await.unwrap();
        let body: HashMap<String, String> = url::form_urlencoded::parse(&request.body)
            .into_owned()
            .collect();

        assert_eq!(token.access_token, "gh_access");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/login/oauth/access_token");
        assert_eq!(request.header("accept"), Some("application/json"));
        assert_eq!(
            request.header("content-type"),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(body.get("client_id"), Some(&"client-id".to_string()));
        assert_eq!(
            body.get("client_secret"),
            Some(&"client-secret".to_string())
        );
        assert_eq!(body.get("code"), Some(&"oauth-code".to_string()));
        assert_eq!(body.get("accept"), Some(&"json".to_string()));
    }

    #[tokio::test]
    async fn github_user_uses_bearer_auth_and_user_agent() {
        let _env_lock = env_lock();
        let (url, request_rx) = spawn_json_server(
            "/user",
            r#"{"id":7,"name":"Ada","bio":null,"login":"ada","avatar_url":"https://img","email":"ada@example.com","email_verified":true}"#,
        )
        .await
        .unwrap();
        let _user_url = ScopedEnv::set("GITHUB_OAUTH_USER_URL", &url);

        let user = get_github_user("gh_token").await.unwrap();
        let request = request_rx.await.unwrap();

        assert_eq!(user.login, "ada");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/user");
        assert_eq!(request.header("authorization"), Some("Bearer gh_token"));
        assert_eq!(request.header("user-agent"), Some("stargate"));
    }
}
