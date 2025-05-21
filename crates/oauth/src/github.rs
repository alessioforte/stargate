use awc::Client;
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
) -> Result<GitHubOauthToken, Box<dyn Error>> {
    let client_secret = env::var("GITHUB_OAUTH_CLIENT_SECRET").unwrap_or_else(|_| "".to_owned());
    let client_id = env::var("GITHUB_OAUTH_CLIENT_ID").unwrap_or_else(|_| "".to_owned());

    let root_url = "https://github.com/login/oauth/access_token";

    let client = Client::new();

    let params = [
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("code", authorization_code),
        ("accept", "json"),
    ];

    let mut response = client
        .post(root_url)
        .append_header(("Accept", "application/json"))
        .send_form(&params)
        .await?;

    if response.status().is_success() {
        let oauth_response = response.json::<GitHubOauthToken>().await?;
        Ok(oauth_response)
    } else {
        let res: serde_json::Value = response.json().await?;
        log::error!("get_github_oauth_token: {}", res.to_string());
        let message = "An error occurred while trying to retrieve the access token.";
        Err(From::from(message))
    }
}

pub async fn get_github_user(access_token: &str) -> Result<GitHubUserResult, Box<dyn Error>> {
    let root_url = "https://api.github.com/user";

    let client = Client::new();

    let mut response = client
        .get(root_url)
        .bearer_auth(access_token)
        .append_header(("User-Agent", "stargate"))
        .send()
        .await?;

    if response.status().is_success() {
        let user_info = response.json::<GitHubUserResult>().await?;
        Ok(user_info)
    } else {
        let res: serde_json::Value = response.json().await?;
        log::error!("get_github_user: {}", res.to_string());
        let message = "An error occurred while trying to retrieve user information.";
        Err(From::from(message))
    }
}
