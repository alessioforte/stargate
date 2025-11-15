use awc::Client;
use serde::{Deserialize, Serialize};
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
) -> Result<OAuthResponse, Box<dyn Error>> {
    let redirect_uri = "http://localhost:5050/oauth/google".to_string();
    let client_secret =
        std::env::var("GOOGLE_OAUTH_CLIENT_SECRET").unwrap_or_else(|_| "".to_string());
    let client_id = std::env::var("GOOGLE_OAUTH_CLIENT_ID").unwrap_or_else(|_| "".to_string());

    let root_url = "https://www.googleapis.com/oauth2/v4/token";
    let client = Client::new();

    let params = [
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", client_id.as_str()),
        ("code", authorization_code),
        ("client_secret", client_secret.as_str()),
    ];
    let mut response = client.post(root_url).send_form(&params).await?;
    if response.status().is_success() {
        let oauth_response = response.json::<OAuthResponse>().await?;
        Ok(oauth_response)
    } else {
        let res: serde_json::Value = response.json().await?;
        tracing::error!("get_google_oauth_token: {}", res);
        let message = "An error occurred while trying to retrieve access token.";
        Err(From::from(message))
    }
}

pub async fn get_google_user(
    access_token: &str,
    id_token: &str,
) -> Result<GoogleUserResult, Box<dyn Error>> {
    let client = Client::new();
    let mut url = Url::parse("https://www.googleapis.com/oauth2/v2/userinfo").unwrap();
    url.query_pairs_mut().append_pair("alt", "json");
    url.query_pairs_mut()
        .append_pair("access_token", access_token);

    let mut response = client
        .get(url.to_string())
        .bearer_auth(id_token)
        .send()
        .await?;
    if response.status().is_success() {
        let user_info = response.json::<GoogleUserResult>().await?;
        Ok(user_info)
    } else {
        let res: serde_json::Value = response.json().await?;
        tracing::error!("get_google_user: {}", res);
        let message = "An error occurred while trying to retrieve user information.";
        Err(From::from(message))
    }
}
