pub mod github;
pub mod google;

use crate::err::{ErrorResponse, HttpError};
use crate::etc::reqctx::take_audit_context_from;
use axum::Json;
use axum::extract::{Form, FromRequest, Request};
use axum::http::StatusCode;
use axum::middleware::from_fn;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use store::Store;

const OAUTH_TOKENS_GRANT: &str = "oauth_tokens";

#[derive(Serialize)]
pub struct StateResponse {
    pub state: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TokenForm {
    token: String,
    token_type_hint: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct IntrospectionResponse {
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exp: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iat: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aud: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jti: Option<String>,
}

impl IntrospectionResponse {
    fn inactive() -> Self {
        Self {
            active: false,
            scope: None,
            client_id: None,
            username: None,
            token_type: None,
            exp: None,
            iat: None,
            sub: None,
            aud: None,
            iss: None,
            jti: None,
        }
    }
}

#[utoipa::path(
    post,
    path = "/oauth/state",
    tags = ["OAuth"],
    responses(
        (status = 200, description = "OK"),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_state() -> Result<Json<StateResponse>, ErrorResponse> {
    let state = crate::act::oauth_state::create_oauth_state()
        .await
        .map_err(|e| {
            tracing::error!("Failed to create OAuth state: {}", e);
            ErrorResponse::from(HttpError::InternalServerError(
                "failed to generate oauth state".to_string(),
            ))
        })?;
    Ok(Json(StateResponse { state }))
}

pub(super) fn build_jwt_cookie(access_token: &str, max_age_secs: i64) -> String {
    let secure = crate::etc::tls::enabled().unwrap_or(false);
    let mut parts = vec![
        format!("jwt={}", access_token),
        "Path=/".to_string(),
        "HttpOnly".to_string(),
        "SameSite=Strict".to_string(),
        format!("Max-Age={}", max_age_secs),
    ];
    if secure {
        parts.push("Secure".to_string());
    }
    parts.join("; ")
}

async fn extract_token_form(req: Request) -> Result<TokenForm, ErrorResponse> {
    let Form(form) = Form::<TokenForm>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;

    if form.token.trim().is_empty() {
        return Err(ErrorResponse::from(HttpError::BadRequest(
            "token is required".to_string(),
        )));
    }

    Ok(form)
}

fn looks_like_api_key(token: &str) -> bool {
    token.starts_with("sk_live_") || token.starts_with("sk_test_")
}

fn scope_from_attrs(attrs: &JsonValue) -> Option<String> {
    if let Some(scope) = attrs.get("scope").and_then(JsonValue::as_str)
        && !scope.trim().is_empty()
    {
        return Some(scope.to_string());
    }

    attrs
        .get("scopes")
        .and_then(JsonValue::as_array)
        .map(|scopes| {
            scopes
                .iter()
                .filter_map(JsonValue::as_str)
                .filter(|scope| !scope.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|scope| !scope.is_empty())
}

async fn jwt_session_active(claims: &jwt::Claims) -> Result<bool, ErrorResponse> {
    let Some(sid) = claims.sid.as_deref() else {
        return Ok(false);
    };
    crate::etc::store::use_store()
        .exists(sid)
        .await
        .map_err(ErrorResponse::internal)
}

async fn introspect_jwt_claims(
    claims: jwt::Claims,
) -> Result<IntrospectionResponse, ErrorResponse> {
    if !matches!(claims.typ.as_deref(), Some("bearer" | "refresh")) {
        return Ok(IntrospectionResponse::inactive());
    }

    if crate::act::token_revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
        || !jwt_session_active(&claims).await?
    {
        return Ok(IntrospectionResponse::inactive());
    }

    Ok(IntrospectionResponse {
        active: true,
        scope: claims.scope.clone(),
        client_id: claims.azp.clone(),
        username: claims.email.clone(),
        token_type: (claims.typ.as_deref() == Some("bearer")).then(|| "Bearer".to_string()),
        exp: Some(claims.exp),
        iat: Some(claims.iat),
        sub: Some(claims.sub.clone()),
        aud: claims.aud.clone(),
        iss: Some(claims.iss.clone()),
        jti: claims.jti.clone(),
    })
}

async fn introspect_api_key(token: &str) -> Result<IntrospectionResponse, ErrorResponse> {
    if !looks_like_api_key(token) {
        return Ok(IntrospectionResponse::inactive());
    }

    let hash = pw::hash_api_key(token);
    let Some(key) = crate::db::get_api_key_by_hash(&hash)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Ok(IntrospectionResponse::inactive());
    };

    if key.revoked {
        return Ok(IntrospectionResponse::inactive());
    }

    Ok(IntrospectionResponse {
        active: true,
        scope: scope_from_attrs(&key.attrs),
        client_id: Some(key.id.clone()),
        username: None,
        token_type: Some("api_key".to_string()),
        exp: None,
        iat: None,
        sub: Some(key.id),
        aud: None,
        iss: Some(jwt::issuer_from_env()),
        jti: None,
    })
}

async fn introspect_token(token: &str) -> Result<IntrospectionResponse, ErrorResponse> {
    match crate::etc::jwt::jwt_config().validate_token(token) {
        Ok(claims) => introspect_jwt_claims(claims).await,
        Err(_) => introspect_api_key(token).await,
    }
}

async fn revoke_api_key_token(
    token: &str,
    ctx: db::ent::AuditContext,
) -> Result<(), ErrorResponse> {
    if !looks_like_api_key(token) {
        return Ok(());
    }

    let hash = pw::hash_api_key(token);
    let Some(key) = crate::db::get_api_key_by_hash(&hash)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Ok(());
    };

    if key.revoked {
        return Ok(());
    }

    crate::db::revoke_api_key(&key.id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;
    crate::etc::store::use_store()
        .delete(&hash)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(())
}

#[utoipa::path(
    post,
    path = "/oauth/introspect",
    tags = ["OAuth"],
    request_body(content = TokenForm, content_type = "application/x-www-form-urlencoded"),
    responses(
        (status = 200, description = "OK", body = IntrospectionResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_introspect(req: Request) -> Result<Json<IntrospectionResponse>, ErrorResponse> {
    crate::require_grants!(req, crate::api::admin::SUPER_ADMIN, OAUTH_TOKENS_GRANT);

    let form = extract_token_form(req).await?;
    let _ = form.token_type_hint.as_deref();
    let body = introspect_token(&form.token).await?;

    Ok(Json(body))
}

#[utoipa::path(
    post,
    path = "/oauth/revoke",
    tags = ["OAuth"],
    request_body(content = TokenForm, content_type = "application/x-www-form-urlencoded"),
    responses(
        (status = 200, description = "OK"),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_revoke(mut req: Request) -> Result<StatusCode, ErrorResponse> {
    crate::require_grants!(req, crate::api::admin::SUPER_ADMIN, OAUTH_TOKENS_GRANT);

    let ctx = take_audit_context_from(req.extensions_mut());
    let form = extract_token_form(req).await?;
    let _ = form.token_type_hint.as_deref();

    match crate::etc::jwt::jwt_config().validate_token(&form.token) {
        Ok(claims) => {
            crate::act::token_revocation::revoke_claims(&claims)
                .await
                .map_err(ErrorResponse::internal)?;
        }
        Err(_) => {
            revoke_api_key_token(&form.token, ctx).await?;
        }
    }

    Ok(StatusCode::OK)
}

pub fn router() -> axum::Router {
    use axum::routing::{get, post};

    let protected = axum::Router::new()
        .route("/oauth/introspect", post(post_introspect))
        .route("/oauth/revoke", post(post_revoke))
        .layer(from_fn(crate::api::admin::extract_grants));

    axum::Router::new()
        .route("/oauth/state", post(post_state))
        .route("/oauth/github", get(github::get_github))
        .route("/oauth/google", get(google::get_google))
        .merge(protected)
}
