use super::Provider;
use crate::err::{ErrorCode, ErrorResponse};
use axum::extract::Request;
use axum::response::Response;
use db::ent::{
    CredentialType, Profile, TrustedAuditActor, TrustedAuditContext, TrustedAuditRequest,
};
use idp::github::{GitHubUser, get_github_oauth_token, get_github_user};

#[utoipa::path(
    get,
    path = "/oauth/github/start",
    tags = ["OAuth"],
    description = "Start signing in with GitHub in the browser: redirects to GitHub, which returns to `/oauth/github`. After signing in the browser goes to the auth app's organization picker, then to `return_to`.",
    params(
        ("return_to" = Option<String>, Query, description = "Where to go once signed in: a path on this server, or a URL on the issuer's or the auth app's origin"),
    ),
    responses(
        (status = 302, description = "Redirect to GitHub"),
        (status = 400, description = "`return_to` is not allowed", body = ErrorResponse),
        (status = 404, description = "GitHub sign-in is not configured on this server (`feature.disabled`)", body = ErrorResponse),
    )
)]
pub async fn get_github_start(req: Request) -> Result<Response, ErrorResponse> {
    let own_origin = super::request_origin(&req);
    super::start(Provider::GitHub, req.uri().clone(), own_origin).await
}

#[utoipa::path(
    get,
    path = "/oauth/github",
    tags = ["OAuth"],
    description = "GitHub's callback. A sign-in started at `/oauth/github/start` redirects to the auth app (the organization picker, or the login page with `error`). A `state` from `POST /oauth/state` gets the tokens as JSON. The GitHub authorization must include the `user:email` scope: the account is matched or created by its primary verified email.",
    params(
        ("code" = Option<String>, Query, description = "Authorization code"),
        ("state" = Option<String>, Query, description = "OAuth state token"),
        ("error" = Option<String>, Query, description = "Set by GitHub when the user did not authorize"),
    ),
    responses(
        (status = 200, description = "OK (`POST /oauth/state` sign-ins)", body = crate::api::account::AuthResponse),
        (status = 302, description = "Browser sign-ins: to the auth app"),
        (status = 400, description = "Missing code or invalid state", body = ErrorResponse),
        (status = 403, description = "The GitHub account has no verified primary email, or no Stargate account uses it and `SIGNUP_MODE` does not create one (`social.account_not_provisioned`, `signup.domain_not_allowed`)", body = ErrorResponse),
        (status = 404, description = "GitHub sign-in is not configured on this server (`feature.disabled`)", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
        (status = 502, description = "Bad Gateway", body = ErrorResponse),
    )
)]
pub async fn get_github(req: Request) -> Response {
    super::callback(req, Provider::GitHub).await
}

/// The account behind a GitHub authorization `code`: the one using the
/// GitHub account's primary verified email, created on a first sign-in when
/// `SIGNUP_MODE` allows.
pub(super) async fn sign_in(
    code: &str,
    audit_request: TrustedAuditRequest,
) -> Result<db::ent::User, ErrorResponse> {
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

    let user = match crate::data::db::get_user_by_email(&email)
        .await
        .map_err(|e| {
            tracing::error!("Database error during GitHub OAuth: {}", e);
            ErrorResponse::new(ErrorCode::SystemInternal)
        })? {
        Some(user) => user,
        None => {
            super::ensure_provisioning("GitHub", &email)?;
            create_github_user(&github_user, &email, audit_context.clone()).await?
        }
    };

    if user.picture.is_none()
        && let Some(avatar_url) = github_user.avatar_url.clone()
    {
        let updated = user.clone().picture(Some(avatar_url));
        crate::data::db::update_user(updated, audit_context)
            .await
            .map_err(|e| {
                tracing::error!("Failed to update GitHub OAuth user picture: {}", e);
                ErrorResponse::new(ErrorCode::SystemInternal)
            })?;
    }
    Ok(user)
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
        let error = match crate::data::db::create_user(
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
        if let Some(user) = crate::data::db::get_user_by_email(email)
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
