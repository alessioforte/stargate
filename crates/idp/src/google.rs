use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::error::Error;
use url::Url;

#[derive(Deserialize)]
pub struct OAuthResponse {
    pub access_token: String,
    pub id_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleUserResult {
    pub id: String,
    pub email: String,
    pub verified_email: bool,
    pub name: String,
    pub given_name: String,
    pub family_name: String,
    pub picture: String,
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

pub async fn get_google_user(
    access_token: &str,
    id_token: &str,
) -> Result<GoogleUserResult, Box<dyn Error + Send + Sync>> {
    let mut url = Url::parse(&google_user_url()).unwrap();
    url.query_pairs_mut().append_pair("alt", "json");
    url.query_pairs_mut()
        .append_pair("access_token", access_token);

    let response = Client::new().get(url).bearer_auth(id_token).send().await?;

    if response.status().is_success() {
        Ok(response.json::<GoogleUserResult>().await?)
    } else {
        let res: serde_json::Value = response.json().await?;
        tracing::error!("get_google_user: {}", res);
        Err("An error occurred while trying to retrieve user information.".into())
    }
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

    use super::{get_google_oauth_token, get_google_user};
    use crate::test_support::{ScopedEnv, env_lock, spawn_json_server};

    #[tokio::test]
    async fn google_oauth_token_posts_expected_form_data() {
        let _env_lock = env_lock();
        let (url, request_rx) = spawn_json_server(
            "/oauth2/v4/token",
            r#"{"access_token":"google_access","id_token":"google_id"}"#,
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
        assert_eq!(token.id_token, "google_id");
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
    async fn google_user_uses_query_and_bearer_auth() {
        let _env_lock = env_lock();
        let (url, request_rx) = spawn_json_server(
            "/oauth2/v2/userinfo",
            r#"{"id":"42","email":"ada@example.com","verified_email":true,"name":"Ada Lovelace","given_name":"Ada","family_name":"Lovelace","picture":"https://img"}"#,
        )
        .await
        .unwrap();
        let _user_url = ScopedEnv::set("GOOGLE_OAUTH_USER_URL", &url);

        let user = get_google_user("google_access", "google_id").await.unwrap();
        let request = request_rx.await.unwrap();
        let (path, query) = request.path.split_once('?').unwrap();
        let query: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();

        assert_eq!(user.email, "ada@example.com");
        assert_eq!(request.method, "GET");
        assert_eq!(path, "/oauth2/v2/userinfo");
        assert_eq!(query.get("alt"), Some(&"json".to_string()));
        assert_eq!(
            query.get("access_token"),
            Some(&"google_access".to_string())
        );
        assert_eq!(request.header("authorization"), Some("Bearer google_id"));
    }
}
