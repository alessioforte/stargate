use crate::errors::{ErrorResponse, HttpError};
use crate::etc::AppData;
use crate::models::oauth2_providers::Oauth2Provider;
use crate::models::tokens::Token;
use crate::models::users::User;
use crate::modules::auth::{create_tokens, Claims};
use crate::modules::hash::Hash;
use crate::services::oauth::github::{get_github_oauth_token, get_github_user};
use actix_web::{get, web, HttpResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryCode {
    pub code: String,
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

    if code.is_empty() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "code is required".to_string(),
        )));
    }

    let token_response = get_github_oauth_token(code).await;
    if token_response.is_err() {
        return Err(ErrorResponse::from(HttpError::BadGateway(
            token_response.err().unwrap().to_string(),
        )));
    }

    let token = token_response.unwrap();
    let github_user = get_github_user(&token.access_token).await;
    if github_user.is_err() {
        return Err(ErrorResponse::from(HttpError::BadGateway(
            github_user.err().unwrap().to_string(),
        )));
    }

    let github_user = github_user.unwrap();

    let user = match User::get_by_email(&github_user.email).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    let user = match user {
        Some(user) => {
            let response = user
                .picture(Some(github_user.avatar_url.clone()))
                .update()
                .await;
            if response.is_err() {
                return Err(ErrorResponse::from(HttpError::InternalServerError(
                    response.err().unwrap().to_string(),
                )));
            }

            let record = response.unwrap();

            record.unwrap()
        }
        None => {
            let new_user = User::new()
                .email(github_user.email.clone())
                .name(github_user.name.clone())
                .picture(Some(github_user.avatar_url.clone()));

            let response = new_user.save().await;

            if response.is_err() {
                return Err(ErrorResponse::from(HttpError::InternalServerError(
                    response.err().unwrap().to_string(),
                )));
            }

            response.unwrap()
        }
    };

    let provider = Oauth2Provider::get_by_provider("github", &github_user.id.to_string()).await;
    if provider.is_err() {
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            provider.err().unwrap().to_string(),
        )));
    }

    let provider = provider.unwrap();
    if provider.is_none() {
        let oauth2_provider = Oauth2Provider {
            id: format!("github:{}", github_user.id.clone()),
            user_id: user.id.clone(),
        };
        let response = Oauth2Provider::save(oauth2_provider).await;
        if response.is_err() {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                response.err().unwrap().to_string(),
            )));
        }
    }

    let (access_token, refresh_token) = create_tokens(
        Claims {
            sub: user.email.to_owned(),
            sub_id: user.id.to_owned(),
            name: Some(user.name.clone()),
            nickname: user.nickname.clone(),
            email_verified: true,
            ..Claims::default()
        },
        data.access_token_expiration,
        data.refresh_token_expiration,
    )
    .unwrap();

    let refresh_token_hash = Hash::encode(&refresh_token).unwrap();

    let upsert_token = Token::save(Token {
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
    web::scope("/github").service(login)
}
