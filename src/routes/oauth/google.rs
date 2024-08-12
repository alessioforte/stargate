use crate::data::AppData;
use crate::errors::{ErrorResponse, HttpError};
use crate::models::oauth2_providers::Oauth2Provider;
use crate::models::tokens::Token;
use crate::models::users::{Payload as UserPayload, User};
use crate::modules::auth::{create_tokens, Claims};
use crate::modules::hash::Hash;
use crate::services::google_oauth::{get_google_oauth_token, get_google_user};
use actix_web::{get, web, HttpResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryCode {
    pub code: String,
    // pub state: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
}

#[get("")]
async fn login(data: AppData, query: web::Query<QueryCode>) -> Result<HttpResponse, ErrorResponse> {
    let code = &query.code;
    // let state = &query.state;

    if code.is_empty() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "code is required".to_string(),
        )));
    }

    let token_response = get_google_oauth_token(code).await;
    if token_response.is_err() {
        let message = format!(
            "Error getting token: {:?}",
            token_response.err().unwrap().to_string()
        );
        return Err(ErrorResponse::from(HttpError::BadGateway(
            message.to_string(),
        )));
    }

    let token = token_response.unwrap();
    let google_user = get_google_user(&token.access_token, &token.id_token).await;

    if google_user.is_err() {
        let message = format!(
            "Error getting user: {:?}",
            google_user.err().unwrap().to_string()
        );
        return Err(ErrorResponse::from(HttpError::BadGateway(
            message.to_string(),
        )));
    }

    let google_user = google_user.unwrap();
    // TODO: now we have the user, we need to create if it doesn't exist and return a JWT token
    let mut user = User::get_by_email(google_user.email.clone()).await.unwrap();
    if user.is_none() {
        // create user
        let new_user = UserPayload {
            email: google_user.email.clone(),
            name: google_user.name.clone(),
            picture: Some(google_user.picture.clone()),
            nickname: None,
            password: None,
            phone_number: None,
        };

        let response = User::create(new_user).await;
        if response.is_err() {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                response.err().unwrap().to_string(),
            )));
        }

        let record = response.unwrap();

        let oauth2_provider = Oauth2Provider {
            user_id: record.id.clone(),
            provider: "google".to_string(),
            provider_id: google_user.id.clone(),
        };

        let response = Oauth2Provider::create(oauth2_provider).await;
        if response.is_err() {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                response.err().unwrap().to_string(),
            )));
        }

        user = User::get_by_email(google_user.email.clone()).await.unwrap();
    } else {
        let user = user.as_mut().unwrap();
        let response = User::update(
            user.id.clone(),
            UserPayload {
                email: google_user.email.clone(),
                name: google_user.name.clone(),
                picture: Some(google_user.picture.clone()),
                nickname: user.nickname.clone(),
                password: user.password.clone(),
                phone_number: user.phone_number.clone(),
            },
        )
        .await;

        if response.is_err() {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                response.err().unwrap().to_string(),
            )));
        }
    }

    let user = user.unwrap();

    let (access_token, refresh_token) = create_tokens(
        Claims {
            sub: user.email.to_owned(),
            sub_id: user.id.to_owned(),
            name: Some(user.name.clone()),
            nickname: user.nickname.clone(),
            email_verified: google_user.verified_email,
            ..Claims::default()
        },
        data.access_token_expiration,
        data.refresh_token_expiration,
    )
    .unwrap();

    let refresh_token_hash = Hash::encode(&refresh_token).unwrap();

    let upsert_token = Token::upsert(Token {
        id: user.id.clone(),
        value: refresh_token_hash.clone(),
    })
    .await;
    if upsert_token.is_err() {
        log::error!("Could not create token: {:?}", upsert_token.err());
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not create token".to_string(),
        )));
    }

    Ok(HttpResponse::Ok().json(web::Json(AuthResponse {
        access_token,
        refresh_token,
    })))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/google").service(login)
}
