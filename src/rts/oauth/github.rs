use crate::err::{ErrorResponse, HttpError};
use crate::fun::format_name;
use actix_web::{HttpMessage, HttpRequest, HttpResponse, get, web};
use db::ent::{AuditContext, CredentialType, Profile};
use jwt::Claims;
use oauth::github::{get_github_oauth_token, get_github_user};
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
async fn login(
    req: HttpRequest,
    query: web::Query<QueryCode>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = match req.extensions().get::<AuditContext>().cloned() {
        Some(c) => c,
        None => AuditContext::anonymous(),
    };
    let code = &query.code;

    if code.is_empty() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "code is required".to_string(),
        )));
    }

    let token_response = get_github_oauth_token(code).await;
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
    let github_user = get_github_user(&token.access_token).await;

    if github_user.is_err() {
        let message = format!(
            "Error getting user: {:?}",
            github_user.err().unwrap().to_string()
        );
        return Err(ErrorResponse::from(HttpError::BadGateway(
            message.to_string(),
        )));
    }

    let github_user = github_user.unwrap();

    let mut user = match crate::db::get_user_by_username(&github_user.email).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    if user.is_none() {
        let new_user = Profile::new(github_user.email.clone())
            .given_name(Some(github_user.name.clone()))
            .nickname(Some(github_user.login.clone()))
            .picture(Some(github_user.avatar_url.clone()));

        let value = format!("github:{}", github_user.id);
        user = match crate::db::create_user(new_user, CredentialType::Oauth, &value, ctx.clone())
            .await
        {
            Ok(user) => Some(user),
            Err(e) => {
                return Err(ErrorResponse::from(HttpError::InternalServerError(
                    e.to_string(),
                )));
            }
        };
    }

    let user = user.unwrap();

    if user.picture.is_none() {
        user.clone().picture(Some(github_user.avatar_url.clone()));
        match crate::db::update_user(user.clone(), ctx).await {
            Ok(updated_user) => updated_user,
            Err(e) => {
                return Err(ErrorResponse::from(HttpError::InternalServerError(
                    e.to_string(),
                )));
            }
        };
    }

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);
    let claims = Claims::default()
        .subject("github-oauth2".to_string())
        .sub_id(user.account_id)
        .name(name.clone())
        .email(user.email.to_owned())
        .email_verified(true);

    let (access_token, refresh_token) = crate::fun::generate_tokens(claims).unwrap();

    Ok(HttpResponse::Ok().json(web::Json(AuthResponse {
        access_token,
        refresh_token,
    })))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/github").service(login)
}
