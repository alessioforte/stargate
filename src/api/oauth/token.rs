use super::authorization_codes::{self, AuthorizationCodeRecord};
use super::pkce;
use super::refresh_tokens;
use super::shared::{
    AUTH_METHOD_NONE, ClientCredentials, GRANT_AUTHORIZATION_CODE, GRANT_CLIENT_CREDENTIALS,
    GRANT_REFRESH_TOKEN, SCOPE_EMAIL, SCOPE_OFFLINE_ACCESS, SCOPE_OPENID, SCOPE_PROFILE,
    authenticate_oauth_client, extract_basic_client_credentials, oauth_bad_request,
    oauth_invalid_client, parse_space_delimited, resolve_audience,
    resolve_client_credentials_parts, resolve_scopes, scope_contains,
};
use crate::err::{ErrorResponse, HttpError};
use axum::Json;
use axum::extract::{Form, FromRequest, Request};
use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TokenEndpointForm {
    grant_type: String,
    client_id: Option<String>,
    client_secret: Option<String>,
    scope: Option<String>,
    audience: Option<String>,
    code: Option<String>,
    redirect_uri: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TokenResponse {
    access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    token_type: String,
    expires_in: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
}

#[utoipa::path(
    post,
    path = "/oauth/token",
    tags = ["OAuth"],
    request_body(content = TokenEndpointForm, content_type = "application/x-www-form-urlencoded"),
    responses(
        (status = 200, description = "OK", body = TokenResponse),
        (status = 400, description = "Bad Request", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
    )
)]
pub async fn post_token(req: Request) -> Result<Json<TokenResponse>, ErrorResponse> {
    let basic = extract_basic_client_credentials(&req)?;
    let form = extract_token_endpoint_form(req).await?;

    let body = match form.grant_type.trim() {
        GRANT_CLIENT_CREDENTIALS => {
            let credentials = resolve_client_credentials(basic, &form)?;
            let client = validate_client_credentials_client(&credentials).await?;
            let scopes = resolve_scopes(form.scope.as_deref(), &client.scopes)?;
            let audience = resolve_audience(form.audience.as_deref(), &client.audiences)?;
            issue_client_credentials_token(&client, &scopes, audience)?
        }
        GRANT_AUTHORIZATION_CODE => {
            let client = resolve_authorization_code_client(basic, &form).await?;
            issue_authorization_code_grant(&client, &form).await?
        }
        GRANT_REFRESH_TOKEN => {
            let client = resolve_refresh_token_client(basic, &form).await?;
            issue_refresh_token_grant(&client, &form).await?
        }
        _ => return Err(oauth_bad_request("unsupported grant_type")),
    };

    Ok(Json(body))
}

async fn extract_token_endpoint_form(req: Request) -> Result<TokenEndpointForm, ErrorResponse> {
    let Form(form) = Form::<TokenEndpointForm>::from_request(req, &())
        .await
        .map_err(|e| ErrorResponse::from(HttpError::BadRequest(e.body_text())))?;

    if form.grant_type.trim().is_empty() {
        return Err(oauth_bad_request("grant_type is required"));
    }

    Ok(form)
}

fn resolve_client_credentials(
    basic: Option<ClientCredentials>,
    form: &TokenEndpointForm,
) -> Result<ClientCredentials, ErrorResponse> {
    resolve_client_credentials_parts(
        basic,
        form.client_id.as_deref(),
        form.client_secret.as_deref(),
    )
}

async fn validate_client_credentials_client(
    credentials: &ClientCredentials,
) -> Result<db::ent::OAuthClient, ErrorResponse> {
    let client = authenticate_oauth_client(credentials).await?;

    if !client
        .grant_types
        .iter()
        .any(|grant_type| grant_type == GRANT_CLIENT_CREDENTIALS)
    {
        return Err(oauth_bad_request(
            "client_credentials grant is not allowed for this client",
        ));
    }

    Ok(client)
}

fn issue_client_credentials_token(
    client: &db::ent::OAuthClient,
    scopes: &[String],
    audience: Option<String>,
) -> Result<TokenResponse, ErrorResponse> {
    let jwt = crate::etc::jwt::jwt_config();
    let sub = client
        .service_account_id
        .clone()
        .unwrap_or_else(|| client.client_id.clone());
    let scope = (!scopes.is_empty()).then(|| scopes.join(" "));

    let mut claims = jwt::Claims::default().subject(sub);
    claims.azp = Some(client.client_id.clone());
    claims.scope = scope.clone();
    claims.aud = audience;
    claims.sub_id = client.service_account_id.clone();

    let access_token = jwt
        .generate_oauth_access_token(claims)
        .map_err(ErrorResponse::internal)?;
    Ok(TokenResponse {
        access_token,
        id_token: None,
        refresh_token: None,
        token_type: "Bearer".to_string(),
        expires_in: jwt.access_exp.as_seconds_f64() as u64,
        scope,
    })
}

fn client_allows_grant(client: &db::ent::OAuthClient, grant_type: &str) -> bool {
    client
        .grant_types
        .iter()
        .any(|candidate| candidate == grant_type)
}

fn validate_client_grant(
    client: db::ent::OAuthClient,
    grant_type: &str,
) -> Result<db::ent::OAuthClient, ErrorResponse> {
    if !client_allows_grant(&client, grant_type) {
        return Err(oauth_bad_request(format!(
            "{grant_type} grant is not allowed for this client"
        )));
    }

    Ok(client)
}

async fn resolve_public_or_confidential_client(
    basic: Option<ClientCredentials>,
    form: &TokenEndpointForm,
    grant_type: &str,
) -> Result<db::ent::OAuthClient, ErrorResponse> {
    if basic.is_some() && form.client_secret.is_some() {
        return Err(oauth_bad_request(
            "multiple client authentication methods are not allowed",
        ));
    }

    if let Some(credentials) = basic {
        if let Some(form_client_id) = form.client_id.as_deref().map(str::trim)
            && !form_client_id.is_empty()
            && form_client_id != credentials.client_id
        {
            return Err(oauth_invalid_client(
                "client_id does not match authentication",
            ));
        }
        let client = authenticate_oauth_client(&credentials).await?;
        return validate_client_grant(client, grant_type);
    }

    if form.client_secret.is_some() {
        let credentials = resolve_client_credentials_parts(
            None,
            form.client_id.as_deref(),
            form.client_secret.as_deref(),
        )?;
        let client = authenticate_oauth_client(&credentials).await?;
        return validate_client_grant(client, grant_type);
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
        .map_err(ErrorResponse::internal)?
    else {
        return Err(oauth_invalid_client("invalid client"));
    };

    if !client.enabled {
        return Err(oauth_invalid_client("client is disabled"));
    }

    if client.token_endpoint_auth_method != AUTH_METHOD_NONE {
        return Err(oauth_invalid_client("client authentication is required"));
    }

    validate_client_grant(client, grant_type)
}

async fn resolve_authorization_code_client(
    basic: Option<ClientCredentials>,
    form: &TokenEndpointForm,
) -> Result<db::ent::OAuthClient, ErrorResponse> {
    resolve_public_or_confidential_client(basic, form, GRANT_AUTHORIZATION_CODE).await
}

async fn resolve_refresh_token_client(
    basic: Option<ClientCredentials>,
    form: &TokenEndpointForm,
) -> Result<db::ent::OAuthClient, ErrorResponse> {
    resolve_public_or_confidential_client(basic, form, GRANT_REFRESH_TOKEN).await
}

#[allow(clippy::too_many_arguments)]
fn issue_user_token_response(
    client: &db::ent::OAuthClient,
    user: &db::ent::User,
    scopes: &[String],
    audience: Option<String>,
    auth_time: chrono::DateTime<Utc>,
    nonce: Option<String>,
    refresh_token: Option<String>,
) -> Result<TokenResponse, ErrorResponse> {
    let jwt = crate::etc::jwt::jwt_config();
    let scope = (!scopes.is_empty()).then(|| scopes.join(" "));
    let include_email = scope_contains(scopes, SCOPE_EMAIL);
    let include_profile = scope_contains(scopes, SCOPE_PROFILE);

    let mut access_claims = jwt::Claims::default()
        .subject(user.id.clone())
        .sub_id(user.id.clone());
    access_claims.azp = Some(client.client_id.clone());
    access_claims.scope = scope.clone();
    access_claims.aud = audience;
    access_claims.auth_time = Some(auth_time.timestamp() as usize);

    let mut id_claims = jwt::Claims::default()
        .subject(user.id.clone())
        .sub_id(user.id.clone())
        .aud(client.client_id.clone());
    id_claims.azp = Some(client.client_id.clone());
    id_claims.auth_time = Some(auth_time.timestamp() as usize);
    id_claims.nonce = nonce;

    if include_email {
        id_claims.email = Some(user.email.clone());
        id_claims.email_verified = Some(true);
    }

    if include_profile {
        let given_name = user.given_name.clone().unwrap_or_default();
        let family_name = user.family_name.clone().unwrap_or_default();
        id_claims.name = Some(crate::fun::format_name(&given_name, &family_name));
        id_claims.preferred_username = Some(user.nickname.clone());
        id_claims.picture = user.picture.clone();
    }

    let access_token = jwt
        .generate_oauth_access_token(access_claims)
        .map_err(ErrorResponse::internal)?;

    let id_token = if scope_contains(scopes, SCOPE_OPENID) {
        Some(
            jwt.generate_oidc_id_token(id_claims)
                .map_err(ErrorResponse::internal)?,
        )
    } else {
        None
    };

    Ok(TokenResponse {
        access_token,
        id_token,
        refresh_token,
        token_type: "Bearer".to_string(),
        expires_in: jwt.access_exp.as_seconds_f64() as u64,
        scope,
    })
}

async fn issue_authorization_code_token(
    client: &db::ent::OAuthClient,
    code: &AuthorizationCodeRecord,
    user: &db::ent::User,
    scopes: &[String],
) -> Result<TokenResponse, ErrorResponse> {
    let refresh_token = if scope_contains(scopes, SCOPE_OFFLINE_ACCESS) {
        if !client_allows_grant(client, GRANT_REFRESH_TOKEN) {
            return Err(oauth_bad_request(
                "invalid_grant: refresh_token grant is not allowed for this client",
            ));
        }
        Some(
            refresh_tokens::issue(
                client.client_id.clone(),
                user.id.clone(),
                scopes.join(" "),
                code.audience.clone(),
                code.auth_time,
                refresh_token_ttl_secs(),
            )
            .await?,
        )
    } else {
        None
    };

    issue_user_token_response(
        client,
        user,
        scopes,
        code.audience.clone(),
        code.auth_time,
        code.nonce.clone(),
        refresh_token,
    )
}

fn refresh_token_ttl_secs() -> u64 {
    crate::etc::jwt::jwt_config()
        .refresh_exp
        .as_seconds_f64()
        .max(1.0) as u64
}

async fn issue_authorization_code_grant(
    client: &db::ent::OAuthClient,
    form: &TokenEndpointForm,
) -> Result<TokenResponse, ErrorResponse> {
    let code = form
        .code
        .as_deref()
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .ok_or_else(|| oauth_bad_request("code is required"))?;

    let redirect_uri = form
        .redirect_uri
        .as_deref()
        .map(str::trim)
        .filter(|redirect_uri| !redirect_uri.is_empty())
        .ok_or_else(|| oauth_bad_request("redirect_uri is required"))?;

    let verifier = form
        .code_verifier
        .as_deref()
        .map(str::trim)
        .filter(|verifier| !verifier.is_empty())
        .ok_or_else(|| oauth_bad_request("code_verifier is required"))?;

    let code_hash = pw::hash_api_key(code);

    let Some(record) = authorization_codes::get(&code_hash).await? else {
        return Err(oauth_bad_request(
            "invalid_grant: invalid authorization code",
        ));
    };

    if record.client_id != client.client_id {
        return Err(oauth_bad_request("invalid_grant: client mismatch"));
    }

    if record.redirect_uri != redirect_uri {
        return Err(oauth_bad_request("invalid_grant: redirect_uri mismatch"));
    }

    pkce::verify(&record, verifier)?;

    let Some(user) = crate::db::get_user_by_id(&record.user_id)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Err(oauth_bad_request("invalid_grant: user not found"));
    };

    let scopes = parse_space_delimited(Some(record.scope.as_str()), "scope")?;

    if !authorization_codes::consume(&code_hash, &record).await? {
        return Err(oauth_bad_request(
            "invalid_grant: invalid authorization code",
        ));
    }

    issue_authorization_code_token(client, &record, &user, &scopes).await
}

async fn issue_refresh_token_grant(
    client: &db::ent::OAuthClient,
    form: &TokenEndpointForm,
) -> Result<TokenResponse, ErrorResponse> {
    let token = form
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| oauth_bad_request("refresh_token is required"))?;

    let Some((new_refresh_token, family)) =
        refresh_tokens::rotate(token, &client.client_id).await?
    else {
        return Err(oauth_bad_request("invalid_grant: invalid refresh token"));
    };

    let Some(user) = crate::db::get_user_by_id(&family.user_id)
        .await
        .map_err(ErrorResponse::internal)?
    else {
        return Err(oauth_bad_request("invalid_grant: user not found"));
    };

    let scopes = parse_space_delimited(Some(family.scope.as_str()), "scope")?;
    issue_user_token_response(
        client,
        &user,
        &scopes,
        family.audience.clone(),
        family.auth_time,
        None,
        Some(new_refresh_token),
    )
}
