use crate::act::oauth_state;
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
    pub state: String,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
}

#[utoipa::path(
    context_path = "/oauth",
    path = "/github",
    tags = ["OAuth"],
    description = "Login with GitHub",
    responses(
        (status = 200, description = "OK", body = AuthResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
        (status = 502, description = "Bad Gateway", body = ErrorResponse),
    )
)]
#[get("")]
pub async fn login(
    req: HttpRequest,
    query: web::Query<QueryCode>,
) -> Result<HttpResponse, ErrorResponse> {
    let ctx = req
        .extensions_mut()
        .remove::<AuditContext>()
        .unwrap_or_else(AuditContext::anonymous);
    let code = &query.code;
    let state = &query.state;

    if code.is_empty() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "code is required".to_string(),
        )));
    }

    let valid_state = oauth_state::validate_oauth_state(state)
        .await
        .unwrap_or(false);
    if !valid_state {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "invalid or expired oauth state".to_string(),
        )));
    }

    let token = get_github_oauth_token(code).await.map_err(|e| {
        tracing::error!("GitHub OAuth token error: {}", e);
        ErrorResponse::from(HttpError::BadGateway(
            "failed to retrieve access token from GitHub".to_string(),
        ))
    })?;

    let github_user = get_github_user(&token.access_token).await.map_err(|e| {
        tracing::error!("GitHub user info error: {}", e);
        ErrorResponse::from(HttpError::BadGateway(
            "failed to retrieve user info from GitHub".to_string(),
        ))
    })?;

    let mut user = crate::db::get_user_by_username(&github_user.email)
        .await
        .map_err(|e| {
            tracing::error!("Database error during GitHub OAuth: {}", e);
            ErrorResponse::from(HttpError::InternalServerError("internal error".to_string()))
        })?;

    if user.is_none() {
        let new_user = Profile::new(github_user.email.clone(), github_user.login.clone())
            .given_name(Some(github_user.name.clone()))
            .picture(Some(github_user.avatar_url.clone()));

        let value = format!("github:{}", github_user.id);
        user = Some(
            crate::db::create_user(new_user, CredentialType::Oauth, &value, ctx.clone())
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create GitHub OAuth user: {}", e);
                    ErrorResponse::from(HttpError::InternalServerError(
                        "internal error".to_string(),
                    ))
                })?,
        );
    }

    let user = user.ok_or_else(|| {
        ErrorResponse::from(HttpError::InternalServerError("internal error".to_string()))
    })?;

    if user.picture.is_none() {
        user.clone().picture(Some(github_user.avatar_url.clone()));
        crate::db::update_user(user.clone(), ctx)
            .await
            .map_err(|e| {
                tracing::error!("Failed to update GitHub OAuth user picture: {}", e);
                ErrorResponse::from(HttpError::InternalServerError("internal error".to_string()))
            })?;
    }

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);
    let claims = Claims::default()
        .subject("github-oauth2".to_string())
        .sub_id(user.id.clone())
        .name(name.clone())
        .email(user.email.to_owned())
        .email_verified(true);

    let (access_token, refresh_token) = crate::fun::generate_tokens(claims).map_err(|e| {
        tracing::error!("Token generation error: {}", e);
        ErrorResponse::from(HttpError::InternalServerError("internal error".to_string()))
    })?;

    Ok(HttpResponse::Ok().json(web::Json(AuthResponse {
        access_token,
        refresh_token,
    })))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/github").service(login)
}
