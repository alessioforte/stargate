use super::{SUPER_ADMIN, extract_json, extract_path, extract_query};
use crate::api::admin::take_admin_audit_context;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::msg::{MessageCode, MessageResponse};
use crate::require_grants;
use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use db::ent::OAuthClient;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;
const OAUTH_CLIENTS_GRANT: &str = "oauth_clients";
const AUTH_METHOD_NONE: &str = "none";
const AUTH_METHOD_CLIENT_SECRET_BASIC: &str = "client_secret_basic";
const AUTH_METHOD_CLIENT_SECRET_POST: &str = "client_secret_post";
const GRANT_CLIENT_CREDENTIALS: &str = "client_credentials";
const GRANT_AUTHORIZATION_CODE: &str = "authorization_code";
const GRANT_REFRESH_TOKEN: &str = "refresh_token";
const RESPONSE_CODE: &str = "code";
const SCOPE_OFFLINE_ACCESS: &str = "offline_access";

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OAuthClientSchema {
    pub client_id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub token_endpoint_auth_method: String,
    pub grant_types: Vec<String>,
    pub response_types: Vec<String>,
    pub redirect_uris: Vec<String>,
    pub scopes: Vec<String>,
    pub audiences: Vec<String>,
    pub attrs: Value,
    pub created_at: String,
    pub updated_at: String,
}

impl From<OAuthClient> for OAuthClientSchema {
    fn from(client: OAuthClient) -> Self {
        let created_at = client.created_at.to_rfc3339();
        let updated_at = client.updated_at.to_rfc3339();
        Self {
            client_id: client.client_id,
            name: client.name,
            description: client.description,
            enabled: client.enabled,
            token_endpoint_auth_method: client.token_endpoint_auth_method,
            grant_types: client.grant_types.0,
            response_types: client.response_types.0,
            redirect_uris: client.redirect_uris.0,
            scopes: client.scopes.0,
            audiences: client.audiences.0,
            attrs: client.attrs,
            created_at,
            updated_at,
        }
    }
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOAuthClientResponse {
    #[serde(flatten)]
    pub client: OAuthClientSchema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RotateOAuthClientSecretResponse {
    #[serde(flatten)]
    pub client: OAuthClientSchema,
    pub client_secret: String,
}

#[derive(Serialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Deserialize, Debug, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListOAuthClientsQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
    #[serde(default)]
    pub q: Option<String>,
}

#[derive(Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOAuthClientRequest {
    #[serde(default)]
    pub client_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub token_endpoint_auth_method: Option<String>,
    pub grant_types: Vec<String>,
    #[serde(default)]
    pub response_types: Option<Vec<String>>,
    #[serde(default)]
    pub redirect_uris: Option<Vec<String>>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    #[serde(default)]
    pub audiences: Option<Vec<String>>,
    #[serde(default)]
    pub attrs: Option<Value>,
}

#[derive(Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOAuthClientRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub token_endpoint_auth_method: String,
    pub grant_types: Vec<String>,
    #[serde(default)]
    pub response_types: Vec<String>,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub audiences: Vec<String>,
    #[serde(default)]
    pub attrs: Option<Value>,
}

#[derive(Deserialize, Debug, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchOAuthClientRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub token_endpoint_auth_method: Option<String>,
    #[serde(default)]
    pub grant_types: Option<Vec<String>>,
    #[serde(default)]
    pub response_types: Option<Vec<String>>,
    #[serde(default)]
    pub redirect_uris: Option<Vec<String>>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
    #[serde(default)]
    pub audiences: Option<Vec<String>>,
    #[serde(default)]
    pub attrs: Option<Value>,
}

struct NormalizedClientInput {
    name: String,
    description: Option<String>,
    token_endpoint_auth_method: String,
    grant_types: Vec<String>,
    response_types: Vec<String>,
    redirect_uris: Vec<String>,
    scopes: Vec<String>,
    audiences: Vec<String>,
    attrs: Value,
}

fn generate_client_id() -> String {
    format!("client_{}", ulid::Ulid::new())
}

fn validate_client_id(client_id: &str) -> Result<(), ErrorResponse> {
    let trimmed = client_id.trim();
    if trimmed.is_empty() || trimmed.len() > 128 || trimmed != client_id {
        return Err(ErrorResponse::new(ErrorCode::OAuthClientIdInvalid));
    }

    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return Err(ErrorResponse::new(ErrorCode::OAuthClientIdInvalid));
    }

    Ok(())
}

fn normalize_name(name: String) -> Result<String, ErrorResponse> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(ErrorResponse::new(ErrorCode::OAuthClientNameRequired));
    }
    if name.len() > 100 {
        return Err(
            ErrorResponse::new(ErrorCode::OAuthClientNameTooLong).with_param("maxLength", 100)
        );
    }
    Ok(name)
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_list(values: Vec<String>, field: &str) -> Result<Vec<String>, ErrorResponse> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        if value.is_empty() {
            return Err(
                ErrorResponse::new(ErrorCode::OAuthClientListValueEmpty).with_param("field", field)
            );
        }
        if seen.insert(value.clone()) {
            out.push(value);
        }
    }
    Ok(out)
}

fn validate_allowed_values(
    field: &str,
    values: &[String],
    allowed: &[&str],
) -> Result<(), ErrorResponse> {
    for value in values {
        if !allowed.contains(&value.as_str()) {
            return Err(ErrorResponse::new(ErrorCode::OAuthClientValueUnsupported)
                .with_param("field", field)
                .with_param("value", value.clone()));
        }
    }
    Ok(())
}

fn validate_auth_method(method: &str) -> Result<(), ErrorResponse> {
    match method {
        AUTH_METHOD_NONE | AUTH_METHOD_CLIENT_SECRET_BASIC | AUTH_METHOD_CLIENT_SECRET_POST => {
            Ok(())
        }
        _ => Err(
            ErrorResponse::new(ErrorCode::OAuthClientAuthMethodUnsupported)
                .with_param("method", method),
        ),
    }
}

fn validate_redirect_uris(redirect_uris: &[String]) -> Result<(), ErrorResponse> {
    for uri in redirect_uris {
        if uri.contains('*') || uri.contains('#') || uri.chars().any(char::is_whitespace) {
            return Err(ErrorResponse::new(ErrorCode::OAuthClientRedirectUriInvalid)
                .with_param("uri", uri.clone()));
        }

        let parsed = url::Url::parse(uri).map_err(|_| {
            ErrorResponse::new(ErrorCode::OAuthClientRedirectUriInvalid)
                .with_param("uri", uri.clone())
        })?;

        match parsed.scheme() {
            "https" => {}
            "http" if is_loopback_redirect_host(parsed.host_str()) => {}
            _ => {
                return Err(
                    ErrorResponse::new(ErrorCode::OAuthClientRedirectUriInsecure)
                        .with_param("uri", uri.clone()),
                );
            }
        }
    }
    Ok(())
}

fn is_loopback_redirect_host(host: Option<&str>) -> bool {
    matches!(host, Some("localhost" | "127.0.0.1" | "::1"))
}

fn validate_client_config(input: &NormalizedClientInput) -> Result<(), ErrorResponse> {
    validate_auth_method(&input.token_endpoint_auth_method)?;
    validate_allowed_values(
        "grant_types",
        &input.grant_types,
        &[
            GRANT_CLIENT_CREDENTIALS,
            GRANT_AUTHORIZATION_CODE,
            GRANT_REFRESH_TOKEN,
        ],
    )?;
    validate_allowed_values("response_types", &input.response_types, &[RESPONSE_CODE])?;
    validate_redirect_uris(&input.redirect_uris)?;

    if input.grant_types.is_empty() {
        return Err(ErrorResponse::new(ErrorCode::OAuthClientGrantTypesRequired));
    }

    let grants = input
        .grant_types
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let responses = input
        .response_types
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();

    if grants.contains(GRANT_CLIENT_CREDENTIALS)
        && input.token_endpoint_auth_method == AUTH_METHOD_NONE
    {
        return Err(ErrorResponse::new(
            ErrorCode::OAuthClientConfidentialRequired,
        ));
    }

    if grants.contains(GRANT_AUTHORIZATION_CODE) {
        if !responses.contains(RESPONSE_CODE) {
            return Err(ErrorResponse::new(
                ErrorCode::OAuthClientCodeResponseTypeRequired,
            ));
        }
        if input.redirect_uris.is_empty() {
            return Err(ErrorResponse::new(
                ErrorCode::OAuthClientRedirectUriRequired,
            ));
        }
    }

    if responses.contains(RESPONSE_CODE) && !grants.contains(GRANT_AUTHORIZATION_CODE) {
        return Err(ErrorResponse::new(
            ErrorCode::OAuthClientAuthorizationCodeGrantRequired,
        ));
    }

    if grants.contains(GRANT_REFRESH_TOKEN) && !grants.contains(GRANT_AUTHORIZATION_CODE) {
        return Err(ErrorResponse::new(
            ErrorCode::OAuthClientRefreshTokenGrantInvalid,
        ));
    }

    if input
        .scopes
        .iter()
        .any(|scope| scope == SCOPE_OFFLINE_ACCESS)
        && !grants.contains(GRANT_REFRESH_TOKEN)
    {
        return Err(ErrorResponse::new(
            ErrorCode::OAuthClientOfflineAccessGrantInvalid,
        ));
    }

    Ok(())
}

fn is_confidential(method: &str) -> bool {
    method != AUTH_METHOD_NONE
}

fn normalize_create_payload(
    payload: CreateOAuthClientRequest,
) -> Result<NormalizedClientInput, ErrorResponse> {
    let input = NormalizedClientInput {
        name: normalize_name(payload.name)?,
        description: normalize_optional_string(payload.description),
        token_endpoint_auth_method: payload
            .token_endpoint_auth_method
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| AUTH_METHOD_CLIENT_SECRET_BASIC.to_string()),
        grant_types: normalize_list(payload.grant_types, "grant_types")?,
        response_types: normalize_list(
            payload.response_types.unwrap_or_default(),
            "response_types",
        )?,
        redirect_uris: normalize_list(payload.redirect_uris.unwrap_or_default(), "redirect_uris")?,
        scopes: normalize_list(payload.scopes.unwrap_or_default(), "scopes")?,
        audiences: normalize_list(payload.audiences.unwrap_or_default(), "audiences")?,
        attrs: payload.attrs.unwrap_or_else(|| serde_json::json!({})),
    };
    validate_client_config(&input)?;
    Ok(input)
}

fn normalize_update_payload(
    payload: UpdateOAuthClientRequest,
) -> Result<NormalizedClientInput, ErrorResponse> {
    let input = NormalizedClientInput {
        name: normalize_name(payload.name)?,
        description: normalize_optional_string(payload.description),
        token_endpoint_auth_method: payload.token_endpoint_auth_method.trim().to_string(),
        grant_types: normalize_list(payload.grant_types, "grant_types")?,
        response_types: normalize_list(payload.response_types, "response_types")?,
        redirect_uris: normalize_list(payload.redirect_uris, "redirect_uris")?,
        scopes: normalize_list(payload.scopes, "scopes")?,
        audiences: normalize_list(payload.audiences, "audiences")?,
        attrs: payload.attrs.unwrap_or_else(|| serde_json::json!({})),
    };
    validate_client_config(&input)?;
    Ok(input)
}

fn normalize_patch_payload(
    existing: &OAuthClient,
    payload: PatchOAuthClientRequest,
) -> Result<NormalizedClientInput, ErrorResponse> {
    let input = NormalizedClientInput {
        name: match payload.name {
            Some(name) => normalize_name(name)?,
            None => existing.name.clone(),
        },
        description: payload
            .description
            .map(Some)
            .unwrap_or_else(|| existing.description.clone()),
        token_endpoint_auth_method: payload
            .token_endpoint_auth_method
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|| existing.token_endpoint_auth_method.clone()),
        grant_types: match payload.grant_types {
            Some(values) => normalize_list(values, "grant_types")?,
            None => existing.grant_types.0.clone(),
        },
        response_types: match payload.response_types {
            Some(values) => normalize_list(values, "response_types")?,
            None => existing.response_types.0.clone(),
        },
        redirect_uris: match payload.redirect_uris {
            Some(values) => normalize_list(values, "redirect_uris")?,
            None => existing.redirect_uris.0.clone(),
        },
        scopes: match payload.scopes {
            Some(values) => normalize_list(values, "scopes")?,
            None => existing.scopes.0.clone(),
        },
        audiences: match payload.audiences {
            Some(values) => normalize_list(values, "audiences")?,
            None => existing.audiences.0.clone(),
        },
        attrs: payload.attrs.unwrap_or_else(|| existing.attrs.clone()),
    };
    validate_client_config(&input)?;
    Ok(input)
}

fn apply_input(
    existing: &mut OAuthClient,
    input: NormalizedClientInput,
) -> Result<(), ErrorResponse> {
    if is_confidential(&input.token_endpoint_auth_method) && existing.client_secret_hash.is_none() {
        return Err(ErrorResponse::new(
            ErrorCode::OAuthClientTypeChangeForbidden,
        ));
    }

    existing.name = input.name;
    existing.description = input.description;
    existing.token_endpoint_auth_method = input.token_endpoint_auth_method;
    existing.grant_types.0 = input.grant_types;
    existing.response_types.0 = input.response_types;
    existing.redirect_uris.0 = input.redirect_uris;
    existing.scopes.0 = input.scopes;
    existing.audiences.0 = input.audiences;
    existing.attrs = input.attrs;

    if existing.token_endpoint_auth_method == AUTH_METHOD_NONE {
        existing.client_secret_hash = None;
    }

    Ok(())
}

async fn load_client(client_id: &str) -> Result<OAuthClient, ErrorResponse> {
    crate::db::get_oauth_client_by_client_id(client_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::new(ErrorCode::OAuthClientNotFound).with_param("clientId", client_id)
        })
}

#[utoipa::path(
    get,
    path = "/admin/oauth/clients",
    tags = ["Admin", "OAuth Clients"],
    params(ListOAuthClientsQuery),
    responses(
        (status = 200, description = "OAuth clients retrieved successfully", body = PaginatedResponse<OAuthClientSchema>),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_oauth_clients(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, OAUTH_CLIENTS_GRANT);

    let query: ListOAuthClientsQuery = extract_query(&req)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);

    let (clients, total) = match query.q.as_deref().map(str::trim) {
        Some(q) if !q.is_empty() => {
            let clients = crate::db::search_oauth_clients(q, limit, offset)
                .await
                .map_err(ErrorResponse::internal)?;
            let total = crate::db::count_search_oauth_clients(q)
                .await
                .map_err(ErrorResponse::internal)?;
            (clients, total)
        }
        _ => {
            let clients = crate::db::get_all_oauth_clients(limit, offset)
                .await
                .map_err(ErrorResponse::internal)?;
            let total = crate::db::count_oauth_clients()
                .await
                .map_err(ErrorResponse::internal)?;
            (clients, total)
        }
    };

    let data = clients.into_iter().map(OAuthClientSchema::from).collect();
    Ok(Json(PaginatedResponse {
        data,
        total,
        limit,
        offset,
    })
    .into_response())
}

#[utoipa::path(
    get,
    path = "/admin/oauth/clients/{client_id}",
    tags = ["Admin", "OAuth Clients"],
    params(("client_id" = String, Path, description = "OAuth client ID")),
    responses(
        (status = 200, description = "OAuth client retrieved successfully", body = OAuthClientSchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "OAuth client not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_oauth_client(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, OAUTH_CLIENTS_GRANT);

    let client_id: String = extract_path(&mut req).await?;
    let client = load_client(&client_id).await?;
    Ok(Json(OAuthClientSchema::from(client)).into_response())
}

#[utoipa::path(
    post,
    path = "/admin/oauth/clients",
    tags = ["Admin", "OAuth Clients"],
    request_body = CreateOAuthClientRequest,
    responses(
        (status = 201, description = "OAuth client created successfully", body = CreateOAuthClientResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 409, description = "OAuth client already exists", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn create_oauth_client(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, OAUTH_CLIENTS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let payload: CreateOAuthClientRequest = extract_json(req).await?;
    let client_id = payload.client_id.clone().unwrap_or_else(generate_client_id);
    validate_client_id(&client_id)?;
    let input = normalize_create_payload(payload)?;

    if crate::db::get_oauth_client_by_client_id(&client_id)
        .await
        .map_err(ErrorResponse::internal)?
        .is_some()
    {
        return Err(ErrorResponse::new(ErrorCode::OAuthClientAlreadyExists)
            .with_param("clientId", client_id.clone()));
    }

    let client_secret =
        is_confidential(&input.token_endpoint_auth_method).then(pw::generate_api_key);
    let client_secret_hash = client_secret.as_deref().map(pw::hash_api_key);
    let client = OAuthClient::new(
        client_id,
        client_secret_hash,
        input.name,
        input.description,
        input.token_endpoint_auth_method,
        input.grant_types,
        input.response_types,
        input.redirect_uris,
        input.scopes,
        input.audiences,
        input.attrs,
    );

    let client = crate::db::create_oauth_client(client, ctx)
        .await
        .map_err(ErrorResponse::internal)?;
    let response = CreateOAuthClientResponse {
        client: OAuthClientSchema::from(client),
        client_secret,
    };

    Ok((StatusCode::CREATED, Json(response)).into_response())
}

#[utoipa::path(
    put,
    path = "/admin/oauth/clients/{client_id}",
    tags = ["Admin", "OAuth Clients"],
    params(("client_id" = String, Path, description = "OAuth client ID")),
    request_body = UpdateOAuthClientRequest,
    responses(
        (status = 200, description = "OAuth client updated successfully", body = OAuthClientSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "OAuth client not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_oauth_client(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, OAUTH_CLIENTS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let client_id: String = extract_path(&mut req).await?;
    let payload: UpdateOAuthClientRequest = extract_json(req).await?;
    let input = normalize_update_payload(payload)?;

    let mut existing = load_client(&client_id).await?;
    apply_input(&mut existing, input)?;

    let updated = crate::db::update_oauth_client(existing, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(OAuthClientSchema::from(updated)).into_response())
}

#[utoipa::path(
    patch,
    path = "/admin/oauth/clients/{client_id}",
    tags = ["Admin", "OAuth Clients"],
    params(("client_id" = String, Path, description = "OAuth client ID")),
    request_body = PatchOAuthClientRequest,
    responses(
        (status = 200, description = "OAuth client patched successfully", body = OAuthClientSchema),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "OAuth client not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn patch_oauth_client(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, OAUTH_CLIENTS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let client_id: String = extract_path(&mut req).await?;
    let payload: PatchOAuthClientRequest = extract_json(req).await?;

    let mut existing = load_client(&client_id).await?;
    let input = normalize_patch_payload(&existing, payload)?;
    apply_input(&mut existing, input)?;

    let updated = crate::db::update_oauth_client(existing, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(OAuthClientSchema::from(updated)).into_response())
}

#[utoipa::path(
    put,
    path = "/admin/oauth/clients/{client_id}/disable",
    tags = ["Admin", "OAuth Clients"],
    params(("client_id" = String, Path, description = "OAuth client ID")),
    responses(
        (status = 200, description = "OAuth client disabled successfully", body = OAuthClientSchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "OAuth client not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn disable_oauth_client(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, OAUTH_CLIENTS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let client_id: String = extract_path(&mut req).await?;
    let _ = load_client(&client_id).await?;
    let updated = crate::db::set_oauth_client_enabled(&client_id, false, ctx)
        .await
        .map_err(ErrorResponse::internal)?;
    Ok(Json(OAuthClientSchema::from(updated)).into_response())
}

#[utoipa::path(
    put,
    path = "/admin/oauth/clients/{client_id}/enable",
    tags = ["Admin", "OAuth Clients"],
    params(("client_id" = String, Path, description = "OAuth client ID")),
    responses(
        (status = 200, description = "OAuth client enabled successfully", body = OAuthClientSchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "OAuth client not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn enable_oauth_client(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, OAUTH_CLIENTS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let client_id: String = extract_path(&mut req).await?;
    let _ = load_client(&client_id).await?;
    let updated = crate::db::set_oauth_client_enabled(&client_id, true, ctx)
        .await
        .map_err(ErrorResponse::internal)?;
    Ok(Json(OAuthClientSchema::from(updated)).into_response())
}

#[utoipa::path(
    put,
    path = "/admin/oauth/clients/{client_id}/rotate-secret",
    tags = ["Admin", "OAuth Clients"],
    params(("client_id" = String, Path, description = "OAuth client ID")),
    responses(
        (status = 200, description = "OAuth client secret rotated successfully", body = RotateOAuthClientSecretResponse),
        (status = 400, description = "Bad request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "OAuth client not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn rotate_oauth_client_secret(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, OAUTH_CLIENTS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let client_id: String = extract_path(&mut req).await?;
    let client = load_client(&client_id).await?;
    if !is_confidential(&client.token_endpoint_auth_method) {
        return Err(ErrorResponse::new(ErrorCode::OAuthClientSecretUnsupported));
    }

    let client_secret = pw::generate_api_key();
    let client_secret_hash = pw::hash_api_key(&client_secret);
    let updated =
        crate::db::update_oauth_client_secret_hash(&client_id, Some(&client_secret_hash), ctx)
            .await
            .map_err(ErrorResponse::internal)?;

    Ok(Json(RotateOAuthClientSecretResponse {
        client: OAuthClientSchema::from(updated),
        client_secret,
    })
    .into_response())
}

#[utoipa::path(
    delete,
    path = "/admin/oauth/clients/{client_id}",
    tags = ["Admin", "OAuth Clients"],
    params(("client_id" = String, Path, description = "OAuth client ID")),
    responses(
        (status = 200, description = "OAuth client deleted successfully", body = MessageResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "OAuth client not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn delete_oauth_client(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN, OAUTH_CLIENTS_GRANT);

    let ctx = take_admin_audit_context(req.extensions_mut())?;
    let client_id: String = extract_path(&mut req).await?;
    let _ = load_client(&client_id).await?;

    crate::db::delete_oauth_client(&client_id, ctx)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(MessageResponse::new(MessageCode::OAuthClientDeleted)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uris_reject_public_http() {
        let uris = vec!["http://evil.example.com/cb".to_string()];

        assert!(validate_redirect_uris(&uris).is_err());
    }

    #[test]
    fn redirect_uris_allow_https_and_loopback_http() {
        let uris = vec![
            "https://app.example.com/cb".to_string(),
            "http://localhost:3000/cb".to_string(),
            "http://127.0.0.1:3000/cb".to_string(),
        ];

        assert!(validate_redirect_uris(&uris).is_ok());
    }
}
