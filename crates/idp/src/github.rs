use reqwest::Client;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::env;
use std::error::Error;

#[derive(Deserialize)]
pub struct GitHubOauthToken {
    pub access_token: String,
}

/// The GitHub account behind an access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
    /// `None` when the user has not set a display name.
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    /// The account's primary email, when GitHub has verified it. It comes
    /// from `/user/emails` (OAuth scope `user:email`): the profile's own
    /// `email` is the public address, `null` for users who keep it private.
    pub verified_email: Option<String>,
}

#[derive(Deserialize)]
struct GitHubProfile {
    id: i64,
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct GitHubEmail {
    email: String,
    primary: bool,
    verified: bool,
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

/// The account behind `access_token`: its profile (`/user`) and its primary
/// verified email (`/user/emails`).
pub async fn get_github_user(
    access_token: &str,
) -> Result<GitHubUser, Box<dyn Error + Send + Sync>> {
    let profile: GitHubProfile = github_get(&github_user_url(), access_token).await?;
    let emails: Vec<GitHubEmail> = github_get(&github_emails_url(), access_token)
        .await
        .map_err(|error| format!("{error} (the authorization needs the user:email scope)"))?;
    let verified_email = emails
        .into_iter()
        .find(|email| email.primary && email.verified)
        .map(|email| email.email);
    Ok(GitHubUser {
        id: profile.id,
        login: profile.login,
        name: profile.name.filter(|name| !name.trim().is_empty()),
        avatar_url: profile.avatar_url,
        verified_email,
    })
}

async fn github_get<T: DeserializeOwned>(
    url: &str,
    access_token: &str,
) -> Result<T, Box<dyn Error + Send + Sync>> {
    let response = Client::new()
        .get(url)
        .bearer_auth(access_token)
        .header("User-Agent", "stargate")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;

    let status = response.status();
    if status.is_success() {
        Ok(response.json::<T>().await?)
    } else {
        let body = response.text().await.unwrap_or_default();
        tracing::error!(url, %status, "GitHub API error: {body}");
        Err(format!("GitHub answered {status} for {url}").into())
    }
}

fn github_token_url() -> String {
    env::var("GITHUB_OAUTH_TOKEN_URL")
        .unwrap_or_else(|_| "https://github.com/login/oauth/access_token".to_string())
}

fn github_user_url() -> String {
    env::var("GITHUB_OAUTH_USER_URL").unwrap_or_else(|_| "https://api.github.com/user".to_string())
}

fn github_emails_url() -> String {
    env::var("GITHUB_OAUTH_EMAILS_URL")
        .unwrap_or_else(|_| "https://api.github.com/user/emails".to_string())
}

#[cfg(test)]
// `env_lock` intentionally serializes process-env access for the whole async
// test; the guard must stay live across the `.await`s. Each `#[tokio::test]`
// runs on its own single-threaded runtime, so there is no deadlock risk.
#[allow(clippy::await_holding_lock)]
mod tests {
    use std::collections::HashMap;

    use super::{GitHubUser, get_github_oauth_token, get_github_user};
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
    async fn github_user_reads_the_profile_and_the_primary_verified_email() {
        let _env_lock = env_lock();
        let (user_url, user_rx) = spawn_json_server(
            "/user",
            r#"{"id":7,"name":null,"bio":null,"login":"ada","avatar_url":"https://img","email":null}"#,
        )
        .await
        .unwrap();
        let (emails_url, emails_rx) = spawn_json_server(
            "/user/emails",
            r#"[{"email":"old@example.com","primary":false,"verified":true,"visibility":null},
                {"email":"ada@example.com","primary":true,"verified":true,"visibility":"private"}]"#,
        )
        .await
        .unwrap();
        let _user_url = ScopedEnv::set("GITHUB_OAUTH_USER_URL", &user_url);
        let _emails_url = ScopedEnv::set("GITHUB_OAUTH_EMAILS_URL", &emails_url);

        let user = get_github_user("gh_token").await.unwrap();

        assert_eq!(
            user,
            GitHubUser {
                id: 7,
                login: "ada".to_string(),
                name: None,
                avatar_url: Some("https://img".to_string()),
                verified_email: Some("ada@example.com".to_string()),
            }
        );
        for request in [user_rx.await.unwrap(), emails_rx.await.unwrap()] {
            assert_eq!(request.method, "GET");
            assert_eq!(request.header("authorization"), Some("Bearer gh_token"));
            assert_eq!(request.header("user-agent"), Some("stargate"));
            assert_eq!(
                request.header("accept"),
                Some("application/vnd.github+json")
            );
        }
    }

    #[tokio::test]
    async fn an_unverified_primary_email_is_not_used() {
        let _env_lock = env_lock();
        let (user_url, _user_rx) = spawn_json_server(
            "/user",
            r#"{"id":7,"name":"Ada","login":"ada","avatar_url":"https://img"}"#,
        )
        .await
        .unwrap();
        let (emails_url, _emails_rx) = spawn_json_server(
            "/user/emails",
            r#"[{"email":"ada@example.com","primary":true,"verified":false},
                {"email":"other@example.com","primary":false,"verified":true}]"#,
        )
        .await
        .unwrap();
        let _user_url = ScopedEnv::set("GITHUB_OAUTH_USER_URL", &user_url);
        let _emails_url = ScopedEnv::set("GITHUB_OAUTH_EMAILS_URL", &emails_url);

        let user = get_github_user("gh_token").await.unwrap();

        assert_eq!(user.name.as_deref(), Some("Ada"));
        assert_eq!(user.verified_email, None);
    }
}
