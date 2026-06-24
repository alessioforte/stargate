use super::shared::{
    AUTH_METHOD_NONE, ClientCredentials, OAUTH_CAN_INTROSPECT_ATTR, OAUTH_CAN_REVOKE_ATTR,
    OAUTH_INTROSPECT_SCOPE, OAUTH_REVOKE_SCOPE, OAUTH_TOKENS_GRANT, OAuthResult,
    authenticate_oauth_client, extract_basic_client_credentials, oauth_bad_request,
    oauth_invalid_client, resolve_client_credentials_parts,
};
use crate::err::OAuthErrorResponse;
use crate::etc::reqctx::take_audit_context_from;
use axum::Json;
use axum::extract::{Form, FromRequest, Request};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use store::Store;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TokenForm {
    token: String,
    token_type_hint: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
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

enum TokenOperationCaller {
    Admin,
    Client(Box<db::ent::OAuthClient>),
}

#[utoipa::path(
    post,
    path = "/oauth/introspect",
    tags = ["OAuth"],
    request_body(content = TokenForm, content_type = "application/x-www-form-urlencoded"),
    responses(
        (status = 200, description = "OK", body = IntrospectionResponse),
        (status = 400, description = "Bad Request", body = OAuthErrorResponse),
        (status = 403, description = "Forbidden", body = OAuthErrorResponse),
        (status = 500, description = "Internal Server Error", body = OAuthErrorResponse),
    )
)]
pub async fn post_introspect(req: Request) -> OAuthResult<Json<IntrospectionResponse>> {
    let basic = extract_basic_client_credentials(&req)?;
    let has_admin_grant = has_token_operation_grant(&req);
    let form = extract_token_form(req).await?;
    let _ = form.token_type_hint.as_deref();
    let caller = authorize_token_operation(has_admin_grant, &form, basic).await?;
    let body = introspect_token(&form.token).await?;

    let body = match caller {
        TokenOperationCaller::Admin => body,
        TokenOperationCaller::Client(client) => {
            if body.active
                && client_can_access_token_response(
                    &client,
                    &body,
                    OAUTH_INTROSPECT_SCOPE,
                    OAUTH_CAN_INTROSPECT_ATTR,
                )
            {
                body
            } else {
                IntrospectionResponse::inactive()
            }
        }
    };

    Ok(Json(body))
}

#[utoipa::path(
    post,
    path = "/oauth/revoke",
    tags = ["OAuth"],
    request_body(content = TokenForm, content_type = "application/x-www-form-urlencoded"),
    responses(
        (status = 200, description = "OK"),
        (status = 400, description = "Bad Request", body = OAuthErrorResponse),
        (status = 403, description = "Forbidden", body = OAuthErrorResponse),
        (status = 500, description = "Internal Server Error", body = OAuthErrorResponse),
    )
)]
pub async fn post_revoke(mut req: Request) -> OAuthResult<StatusCode> {
    let basic = extract_basic_client_credentials(&req)?;
    let has_admin_grant = has_token_operation_grant(&req);
    let ctx = take_audit_context_from(req.extensions_mut());
    let form = extract_token_form(req).await?;
    let _ = form.token_type_hint.as_deref();
    let caller = authorize_token_operation(has_admin_grant, &form, basic).await?;

    let visible = match &caller {
        TokenOperationCaller::Admin => true,
        TokenOperationCaller::Client(client) => {
            let body = introspect_token(&form.token).await?;
            body.active
                && client_can_access_token_response(
                    client,
                    &body,
                    OAUTH_REVOKE_SCOPE,
                    OAUTH_CAN_REVOKE_ATTR,
                )
        }
    };

    if !visible {
        return Ok(StatusCode::OK);
    }

    match validate_revocable_jwt(&form.token) {
        Ok(claims) => {
            crate::act::token_revocation::revoke_claims(&claims)
                .await
                .map_err(OAuthErrorResponse::internal)?;
        }
        Err(_) => {
            revoke_api_key_token(&form.token, ctx).await?;
        }
    }

    Ok(StatusCode::OK)
}

async fn extract_token_form(req: Request) -> OAuthResult<TokenForm> {
    let Form(form) = Form::<TokenForm>::from_request(req, &())
        .await
        .map_err(|e| oauth_bad_request(e.body_text()))?;

    if form.token.trim().is_empty() {
        return Err(oauth_bad_request("token is required"));
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

async fn jwt_session_active(claims: &jwt::Claims) -> OAuthResult<bool> {
    if let Some(sid) = claims.sid.as_deref() {
        return crate::etc::store::use_store()
            .exists(sid)
            .await
            .map_err(OAuthErrorResponse::internal);
    }

    let Some(client_id) = claims.azp.as_deref() else {
        return Ok(false);
    };
    let Some(client) = crate::db::get_oauth_client_by_client_id(client_id)
        .await
        .map_err(OAuthErrorResponse::internal)?
    else {
        return Ok(false);
    };
    Ok(client.enabled)
}

async fn introspect_jwt_claims(claims: jwt::Claims) -> OAuthResult<IntrospectionResponse> {
    if !matches!(claims.typ.as_deref(), Some("bearer" | "refresh")) {
        return Ok(IntrospectionResponse::inactive());
    }

    if crate::act::token_revocation::is_revoked(&claims)
        .await
        .map_err(OAuthErrorResponse::internal)?
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

async fn introspect_api_key(token: &str) -> OAuthResult<IntrospectionResponse> {
    if !looks_like_api_key(token) {
        return Ok(IntrospectionResponse::inactive());
    }

    let hash = pw::hash_api_key(token);
    let Some(key) = crate::db::get_api_key_by_hash(&hash)
        .await
        .map_err(OAuthErrorResponse::internal)?
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

async fn introspect_token(token: &str) -> OAuthResult<IntrospectionResponse> {
    match validate_introspectable_jwt(token) {
        Ok(claims) => introspect_jwt_claims(claims).await,
        Err(_) => introspect_api_key(token).await,
    }
}

fn validate_introspectable_jwt(token: &str) -> Result<jwt::Claims, jwt::JwtValidationError> {
    let jwt = crate::etc::jwt::jwt_config();
    jwt.validate_oauth_access_token(token, None)
        .or_else(|_| jwt.validate_session_access_token(token))
        .or_else(|_| jwt.validate_session_refresh_token(token))
}

fn validate_revocable_jwt(token: &str) -> Result<jwt::Claims, jwt::JwtValidationError> {
    validate_introspectable_jwt(token)
}

fn token_operation_missing_permission() -> OAuthErrorResponse {
    OAuthErrorResponse::insufficient_scope("insufficient permissions")
}

fn has_oauth_client_permission(client: &db::ent::OAuthClient, scope: &str, attr: &str) -> bool {
    client
        .scopes
        .iter()
        .any(|client_scope| client_scope == scope)
        || client
            .attrs
            .get(attr)
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
}

fn client_can_access_token_response(
    client: &db::ent::OAuthClient,
    response: &IntrospectionResponse,
    scope: &str,
    attr: &str,
) -> bool {
    if response.client_id.as_deref() == Some(client.client_id.as_str()) {
        return true;
    }

    if !has_oauth_client_permission(client, scope, attr) {
        return false;
    }

    let Some(audience) = response.aud.as_deref() else {
        return false;
    };

    client.audiences.iter().any(|allowed| allowed == audience)
}

fn has_token_operation_grant(req: &Request) -> bool {
    req.extensions()
        .get::<crate::api::admin::Grants>()
        .cloned()
        .unwrap_or_default()
        .has_any(&[crate::api::admin::SUPER_ADMIN, OAUTH_TOKENS_GRANT])
}

async fn authorize_token_operation(
    has_admin_grant: bool,
    form: &TokenForm,
    basic: Option<ClientCredentials>,
) -> OAuthResult<TokenOperationCaller> {
    if has_admin_grant {
        return Ok(TokenOperationCaller::Admin);
    }

    let client = resolve_token_operation_client(basic, form).await?;
    Ok(TokenOperationCaller::Client(Box::new(client)))
}

async fn resolve_token_operation_client(
    basic: Option<ClientCredentials>,
    form: &TokenForm,
) -> OAuthResult<db::ent::OAuthClient> {
    if basic.is_none() && form.client_id.is_none() && form.client_secret.is_none() {
        return Err(token_operation_missing_permission());
    }

    if basic.is_some() || form.client_secret.is_some() {
        let credentials = resolve_client_credentials_parts(
            basic,
            form.client_id.as_deref(),
            form.client_secret.as_deref(),
        )?;
        return authenticate_oauth_client(&credentials).await;
    }

    let Some(client_id) = form
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|client_id| !client_id.is_empty())
    else {
        return Err(oauth_invalid_client("client authentication is required"));
    };

    let Some(client) = crate::db::get_oauth_client_by_client_id(client_id)
        .await
        .map_err(OAuthErrorResponse::internal)?
    else {
        return Err(oauth_invalid_client("invalid client"));
    };

    if !client.enabled {
        return Err(oauth_invalid_client("client is disabled"));
    }

    if client.token_endpoint_auth_method != AUTH_METHOD_NONE {
        return Err(oauth_invalid_client("client authentication is required"));
    }

    Ok(client)
}

async fn revoke_api_key_token(token: &str, ctx: db::ent::AuditContext) -> OAuthResult<()> {
    if !looks_like_api_key(token) {
        return Ok(());
    }

    let hash = pw::hash_api_key(token);
    let Some(key) = crate::db::get_api_key_by_hash(&hash)
        .await
        .map_err(OAuthErrorResponse::internal)?
    else {
        return Ok(());
    };

    if key.revoked {
        return Ok(());
    }

    crate::db::revoke_api_key(&key.id, ctx)
        .await
        .map_err(OAuthErrorResponse::internal)?;
    crate::etc::store::use_store()
        .delete(&hash)
        .await
        .map_err(OAuthErrorResponse::internal)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::shared::{AUTH_METHOD_CLIENT_SECRET_BASIC, GRANT_CLIENT_CREDENTIALS};
    use super::*;

    fn test_client(client_id: &str) -> db::ent::OAuthClient {
        db::ent::OAuthClient::new(
            client_id.to_string(),
            None,
            "Test Client".to_string(),
            None,
            None,
            None,
            AUTH_METHOD_CLIENT_SECRET_BASIC.to_string(),
            vec![GRANT_CLIENT_CREDENTIALS.to_string()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec!["gateway".to_string()],
            serde_json::json!({}),
        )
    }

    #[test]
    fn oauth_client_can_access_own_token_response() {
        let client = test_client("client-1");
        let response = IntrospectionResponse {
            active: true,
            client_id: Some("client-1".to_string()),
            aud: None,
            ..IntrospectionResponse::inactive()
        };

        assert!(client_can_access_token_response(
            &client,
            &response,
            OAUTH_INTROSPECT_SCOPE,
            OAUTH_CAN_INTROSPECT_ATTR
        ));
    }

    #[test]
    fn oauth_client_can_access_configured_audience_with_permission() {
        let mut client = test_client("client-1");
        client.scopes.0.push(OAUTH_INTROSPECT_SCOPE.to_string());
        let response = IntrospectionResponse {
            active: true,
            client_id: Some("client-2".to_string()),
            aud: Some("gateway".to_string()),
            ..IntrospectionResponse::inactive()
        };

        assert!(client_can_access_token_response(
            &client,
            &response,
            OAUTH_INTROSPECT_SCOPE,
            OAUTH_CAN_INTROSPECT_ATTR
        ));
    }

    #[test]
    fn oauth_client_cannot_access_unconfigured_audience() {
        let mut client = test_client("client-1");
        client.scopes.0.push(OAUTH_INTROSPECT_SCOPE.to_string());
        let response = IntrospectionResponse {
            active: true,
            client_id: Some("client-2".to_string()),
            aud: Some("admin-api".to_string()),
            ..IntrospectionResponse::inactive()
        };

        assert!(!client_can_access_token_response(
            &client,
            &response,
            OAUTH_INTROSPECT_SCOPE,
            OAUTH_CAN_INTROSPECT_ATTR
        ));
    }
}
