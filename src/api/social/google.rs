use super::github::AuthResponse;
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
use idp::google::{GoogleUser, get_google_oauth_token, get_google_user};
use jwt::Claims;
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
    description = "Login with Google. The Google authorization must include the `email` scope: the account is matched or created by its verified email.",
    params(
        ("code" = String, Query, description = "Authorization code"),
        ("state" = String, Query, description = "OAuth state token"),
    ),
    responses(
        (status = 200, description = "OK", body = AuthResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "The Google account's email is not verified, or no Stargate account uses it and `SIGNUP_MODE` does not create one (`social.account_not_provisioned`, `signup.domain_not_allowed`)", body = ErrorResponse),
        (status = 404, description = "Google sign-in is not configured on this server (`feature.disabled`)", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
        (status = 502, description = "Bad Gateway", body = ErrorResponse),
    )
)]
pub async fn get_google(req: Request) -> Result<Response, ErrorResponse> {
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

    let token = get_google_oauth_token(code).await.map_err(|e| {
        tracing::error!("Google OAuth token error: {}", e);
        ErrorResponse::new(ErrorCode::SocialTokenExchangeFailed).with_param("provider", "Google")
    })?;

    let google_user = get_google_user(&token.access_token).await.map_err(|e| {
        tracing::error!("Google user info error: {}", e);
        ErrorResponse::new(ErrorCode::SocialUserInfoFailed).with_param("provider", "Google")
    })?;
    // Accounts are matched and created by email, so only an address Google
    // has verified as the account's own will do.
    let Some(email) = google_user.verified_email.clone() else {
        return Err(
            ErrorResponse::new(ErrorCode::SocialEmailUnverified).with_param("provider", "Google")
        );
    };
    let audit_context = TrustedAuditContext::application(
        TrustedAuditActor::external_identity(format!("google:{}", google_user.id)),
        audit_request,
    );

    let mut user = crate::db::get_user_by_email(&email).await.map_err(|e| {
        tracing::error!("Database error during Google OAuth: {}", e);
        ErrorResponse::new(ErrorCode::SystemInternal)
    })?;

    if user.is_none() {
        super::ensure_provisioning("Google", &email)?;
        user = Some(create_google_user(&google_user, &email, audit_context.clone()).await?);
    }

    let user = user.ok_or_else(|| ErrorResponse::new(ErrorCode::SystemInternal))?;

    if user.picture.is_none()
        && let Some(picture) = google_user.picture.clone()
    {
        let updated = user.clone().picture(Some(picture));
        crate::db::update_user(updated, audit_context)
            .await
            .map_err(|e| {
                tracing::error!("Failed to update Google OAuth user picture: {}", e);
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

/// The account for a first Google sign-in; its nickname is the email.
async fn create_google_user(
    google_user: &GoogleUser,
    email: &str,
    audit_context: TrustedAuditContext,
) -> Result<db::ent::User, ErrorResponse> {
    // Google's full name is the given name when it sends no name parts.
    let (given_name, family_name) = match (&google_user.given_name, &google_user.family_name) {
        (None, None) => (google_user.name.clone(), None),
        (given_name, family_name) => (given_name.clone(), family_name.clone()),
    };
    let profile = Profile::new(email.to_string(), email.to_string())
        .given_name(given_name)
        .family_name(family_name)
        .picture(google_user.picture.clone());
    let credential = format!("google:{}", google_user.id);
    match crate::db::create_user(
        profile,
        CredentialType::Oauth,
        &credential,
        None,
        audit_context,
    )
    .await
    {
        Ok(user) => Ok(user),
        Err(error) => {
            // A concurrent sign-in of the same Google user created the account.
            if let Some(user) = crate::db::get_user_by_email(email)
                .await
                .map_err(ErrorResponse::internal)?
            {
                return Ok(user);
            }
            tracing::error!("Failed to create Google OAuth user: {}", error);
            Err(ErrorResponse::new(ErrorCode::SystemInternal))
        }
    }
}
