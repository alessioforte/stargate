use crate::act::oauth_state;
use crate::err::{ErrorCode, ErrorResponse};
use crate::etc;
use crate::etc::auth::jwt::jwt_config;
use crate::etc::request_context::audit_request_from;
use crate::fun::build_jwt_cookie;
use crate::fun::format_name;
use axum::Json;
use axum::extract::{Query, Request};
use axum::response::{IntoResponse, Response};
use db::ent::{CredentialType, Profile, TrustedAuditActor, TrustedAuditContext};
use http::header::SET_COOKIE;
use idp::github::{GitHubUser, get_github_oauth_token, get_github_user};
use jwt::Claims;
use serde::{Deserialize, Serialize};
use store::Store;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryCode {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
}

#[utoipa::path(
    get,
    path = "/oauth/github",
    tags = ["OAuth"],
    description = "Login with GitHub. The GitHub authorization must include the `user:email` scope: the account is matched or created by its primary verified email.",
    params(
        ("code" = String, Query, description = "Authorization code"),
        ("state" = String, Query, description = "OAuth state token"),
    ),
    responses(
        (status = 200, description = "OK", body = AuthResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "The GitHub account has no verified primary email", body = ErrorResponse),
        (status = 404, description = "GitHub sign-in is not configured on this server (`feature.disabled`)", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
        (status = 502, description = "Bad Gateway", body = ErrorResponse),
    )
)]
pub async fn get_github(req: Request) -> Result<Response, ErrorResponse> {
    let audit_request = audit_request_from(req.extensions());

    let Query(query): Query<QueryCode> = Query::try_from_uri(req.uri()).map_err(|error| {
        ErrorResponse::new(ErrorCode::RequestInvalidQuery).with_message(error.body_text())
    })?;

    let code = &query.code;
    let state = &query.state;

    if code.is_empty() {
        return Err(ErrorResponse::new(
            ErrorCode::SocialAuthorizationCodeMissing,
        ));
    }

    let valid_state = oauth_state::validate_oauth_state(state)
        .await
        .unwrap_or(false);
    if !valid_state {
        return Err(ErrorResponse::new(ErrorCode::SocialStateInvalid));
    }

    let token = get_github_oauth_token(code).await.map_err(|e| {
        tracing::error!("GitHub OAuth token error: {}", e);
        ErrorResponse::new(ErrorCode::SocialTokenExchangeFailed).with_param("provider", "GitHub")
    })?;

    let github_user = get_github_user(&token.access_token).await.map_err(|e| {
        tracing::error!("GitHub user info error: {}", e);
        ErrorResponse::new(ErrorCode::SocialUserInfoFailed).with_param("provider", "GitHub")
    })?;
    // Accounts are matched and created by email, so only an address GitHub
    // has verified as the account's own will do.
    let Some(email) = github_user.verified_email.clone() else {
        return Err(
            ErrorResponse::new(ErrorCode::SocialEmailUnverified).with_param("provider", "GitHub")
        );
    };
    let audit_context = TrustedAuditContext::application(
        TrustedAuditActor::external_identity(format!("github:{}", github_user.id)),
        audit_request,
    );

    let mut user = crate::db::get_user_by_email(&email).await.map_err(|e| {
        tracing::error!("Database error during GitHub OAuth: {}", e);
        ErrorResponse::new(ErrorCode::SystemInternal)
    })?;

    if user.is_none() {
        user = Some(create_github_user(&github_user, &email, audit_context.clone()).await?);
    }

    let user = user.ok_or_else(|| ErrorResponse::new(ErrorCode::SystemInternal))?;

    if user.picture.is_none()
        && let Some(avatar_url) = github_user.avatar_url.clone()
    {
        let updated = user.clone().picture(Some(avatar_url));
        crate::db::update_user(updated, audit_context)
            .await
            .map_err(|e| {
                tracing::error!("Failed to update GitHub OAuth user picture: {}", e);
                ErrorResponse::new(ErrorCode::SystemInternal)
            })?;
    }

    let given_name = user.given_name.clone().unwrap_or_default();
    let family_name = user.family_name.clone().unwrap_or_default();
    let name = format_name(&given_name, &family_name);

    let sid = ulid::Ulid::generate().to_string();
    let auth_time = chrono::Utc::now().timestamp() as usize;
    let mut claims = Claims::default()
        .subject(user.email.to_owned())
        .sub_id(user.id.to_owned())
        .name(name)
        .email(user.email.to_owned())
        .email_verified(true)
        .sid(sid.clone());
    claims.auth_time = Some(auth_time);

    let is_super_admin = crate::fun::is_super_admin_user_id(&user.id)
        .await
        .map_err(ErrorResponse::internal)?;

    if is_super_admin {
        claims = claims.role(crate::fun::SUPER_ADMIN_ROLE.to_string());
    }

    let org = crate::act::sessions::resolve_org_context(&user, None).await?;
    claims.org_id = org.as_ref().map(|org| org.org_id.clone());

    let tokens = crate::fun::generate_tokens(claims).map_err(|e| {
        tracing::error!("Token generation error: {}", e);
        ErrorResponse::new(ErrorCode::SystemInternal)
    })?;

    let store = etc::store::use_store();
    let mut subject = etc::auth::subject::Subject::from(user.clone());
    subject.org_id = org.as_ref().map(|org| org.org_id.clone());
    subject.org_role = org.as_ref().map(|org| org.role.clone());
    let refresh_exp = jwt_config().refresh_exp;
    let access_exp = jwt_config().access_exp;
    let sttl: u64 = refresh_exp.as_seconds_f64() as u64;
    let cttl: i64 = access_exp.as_seconds_f64() as i64;

    store.set(&sid, &subject, Some(sttl)).await.map_err(|e| {
        tracing::error!("Failed to store OAuth session: {}", e);
        ErrorResponse::new(ErrorCode::SystemInternal)
    })?;
    if let Err(error) = crate::act::sessions::register_session(
        crate::act::sessions::NewSession {
            user_id: &user.id,
            sid: &sid,
            org_id: subject.org_id.as_deref(),
            authenticated_at_unix: auth_time as u64,
            amr: vec![oidc::authentication::AMR_FEDERATED.to_string()],
        },
        &tokens.refresh_jti,
        sttl,
    )
    .await
    {
        let _ = store.delete(&sid).await;
        return Err(ErrorResponse::internal(error));
    }

    let cookie = build_jwt_cookie(&tokens.access_token, cttl);
    let body = AuthResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
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

/// Times the account creation is retried when another sign-up takes the
/// chosen nickname between the check and the insert.
const CREATE_ATTEMPTS: usize = 3;

/// The account for a first GitHub sign-in. Its nickname is the GitHub
/// username, suffixed when another account already has it.
async fn create_github_user(
    github_user: &GitHubUser,
    email: &str,
    audit_context: TrustedAuditContext,
) -> Result<db::ent::User, ErrorResponse> {
    let credential = format!("github:{}", github_user.id);
    let mut attempt = 1;
    loop {
        let nickname = crate::act::nicknames::available_nickname(&github_user.login, email).await?;
        let profile = Profile::new(email.to_string(), nickname.clone())
            .given_name(github_user.name.clone())
            .picture(github_user.avatar_url.clone());
        let error = match crate::db::create_user(
            profile,
            CredentialType::Oauth,
            &credential,
            None,
            audit_context.clone(),
        )
        .await
        {
            Ok(user) => return Ok(user),
            Err(error) => error,
        };

        // A concurrent sign-in of the same GitHub user created the account.
        if let Some(user) = crate::db::get_user_by_email(email)
            .await
            .map_err(ErrorResponse::internal)?
        {
            return Ok(user);
        }
        if attempt < CREATE_ATTEMPTS && crate::act::nicknames::nickname_taken(&nickname).await? {
            tracing::debug!(%nickname, "nickname taken during GitHub sign-up, retrying");
            attempt += 1;
            continue;
        }
        tracing::error!("Failed to create GitHub OAuth user: {}", error);
        return Err(ErrorResponse::new(ErrorCode::SystemInternal));
    }
}
