use super::github::AuthResponse;
use crate::act::oauth_state;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::jwt::jwt_config;
use crate::etc::reqctx::take_audit_context_from;
use crate::fun::build_jwt_cookie;
use crate::fun::format_name;
use axum::Json;
use axum::extract::{Query, Request};
use axum::response::{IntoResponse, Response};
use db::ent::{CredentialType, Profile};
use http::header::SET_COOKIE;
use jwt::Claims;
use oauth::google::{get_google_oauth_token, get_google_user};
use serde::{Deserialize, Serialize};
use store::Store;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryCode {
    pub code: String,
    pub state: String,
}

#[utoipa::path(
    get,
    path = "/oauth/google",
    tags = ["OAuth"],
    description = "Login with Google",
    params(
        ("code" = String, Query, description = "Authorization code"),
        ("state" = String, Query, description = "OAuth state token"),
    ),
    responses(
        (status = 200, description = "OK", body = AuthResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
        (status = 502, description = "Bad Gateway", body = ErrorResponse),
    )
)]
pub async fn get_google(mut req: Request) -> Result<Response, ErrorResponse> {
    let ctx = take_audit_context_from(req.extensions_mut());

    let Query(query): Query<QueryCode> = Query::try_from_uri(req.uri())
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;

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

    let token = get_google_oauth_token(code).await.map_err(|e| {
        tracing::error!("Google OAuth token error: {}", e);
        ErrorResponse::from(HttpError::BadGateway(
            "failed to retrieve access token from Google".to_string(),
        ))
    })?;

    let google_user = get_google_user(&token.access_token, &token.id_token)
        .await
        .map_err(|e| {
            tracing::error!("Google user info error: {}", e);
            ErrorResponse::from(HttpError::BadGateway(
                "failed to retrieve user info from Google".to_string(),
            ))
        })?;

    let mut user = crate::db::get_user_by_username(&google_user.email)
        .await
        .map_err(|e| {
            tracing::error!("Database error during Google OAuth: {}", e);
            ErrorResponse::from(HttpError::InternalServerError("internal error".to_string()))
        })?;

    if user.is_none() {
        let new_user = Profile::new(google_user.email.clone(), google_user.email.clone())
            .given_name(Some(google_user.name.clone()))
            .picture(Some(google_user.picture.clone()));

        let value = format!("google:{}", google_user.id);
        user = Some(
            crate::db::create_user(new_user, CredentialType::Oauth, &value, ctx.clone())
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create Google OAuth user: {}", e);
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
        let updated = user.clone().picture(Some(google_user.picture.clone()));
        crate::db::update_user(updated, ctx).await.map_err(|e| {
            tracing::error!("Failed to update Google OAuth user picture: {}", e);
            ErrorResponse::from(HttpError::InternalServerError("internal error".to_string()))
        })?;
    }

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);

    let sid = ulid::Ulid::new().to_string();
    let mut claims = Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name)
        .email(user.email.to_owned())
        .email_verified(google_user.verified_email)
        .sid(sid.clone());

    let is_super_admin = crate::fun::is_super_admin_user_id(&user.id)
        .await
        .map_err(ErrorResponse::internal)?;

    if is_super_admin {
        claims = claims.role(crate::fun::SUPER_ADMIN_ROLE.to_string());
    }

    let (access_token, refresh_token) = crate::fun::generate_tokens(claims).map_err(|e| {
        tracing::error!("Token generation error: {}", e);
        ErrorResponse::from(HttpError::InternalServerError("internal error".to_string()))
    })?;

    let store = etc::store::use_store();
    let subject = etc::sub::Subject::from(user.clone());
    let refresh_exp = jwt_config().refresh_exp;
    let access_exp = jwt_config().access_exp;
    let sttl: u64 = refresh_exp.as_seconds_f64() as u64;
    let cttl: i64 = access_exp.as_seconds_f64() as i64;

    store.set(&sid, &subject, Some(sttl)).await.map_err(|e| {
        tracing::error!("Failed to store OAuth session: {}", e);
        ErrorResponse::from(HttpError::InternalServerError("internal error".to_string()))
    })?;

    let cookie = build_jwt_cookie(&access_token, cttl);
    let body = AuthResponse {
        access_token,
        refresh_token,
    };

    let mut resp = Json(body).into_response();
    resp.headers_mut().insert(
        SET_COOKIE,
        cookie
            .parse()
            .map_err(|_| ErrorResponse::internal("invalid cookie header"))?,
    );
    Ok(resp)
}
