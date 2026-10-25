use super::Provider;
use crate::err::{ErrorCode, ErrorResponse};
use axum::extract::Request;
use axum::response::Response;
use db::ent::{
    CredentialType, Profile, TrustedAuditActor, TrustedAuditContext, TrustedAuditRequest,
};
use idp::google::{GoogleUser, get_google_oauth_token, get_google_user};

#[utoipa::path(
    get,
    path = "/oauth/google/start",
    tags = ["OAuth"],
    description = "Start signing in with Google in the browser: redirects to Google, which returns to `GOOGLE_OAUTH_REDIRECT_URI` (`/oauth/google`). After signing in the browser goes to the auth app's organization picker, then to `return_to`.",
    params(
        ("return_to" = Option<String>, Query, description = "Where to go once signed in: a path on this server, or a URL on the issuer's or the auth app's origin"),
    ),
    responses(
        (status = 302, description = "Redirect to Google"),
        (status = 400, description = "`return_to` is not allowed", body = ErrorResponse),
        (status = 404, description = "Google sign-in is not configured on this server (`feature.disabled`)", body = ErrorResponse),
    )
)]
pub async fn get_google_start(req: Request) -> Result<Response, ErrorResponse> {
    let own_origin = super::request_origin(&req);
    super::start(Provider::Google, req.uri().clone(), own_origin).await
}

#[utoipa::path(
    get,
    path = "/oauth/google",
    tags = ["OAuth"],
    description = "Google's callback. A sign-in started at `/oauth/google/start` redirects to the auth app (the organization picker, or the login page with `error`). A `state` from `POST /oauth/state` gets the tokens as JSON. The Google authorization must include the `email` scope: the account is matched or created by its verified email.",
    params(
        ("code" = Option<String>, Query, description = "Authorization code"),
        ("state" = Option<String>, Query, description = "OAuth state token"),
        ("error" = Option<String>, Query, description = "Set by Google when the user did not consent"),
    ),
    responses(
        (status = 200, description = "OK (`POST /oauth/state` sign-ins)", body = crate::api::account::AuthResponse),
        (status = 302, description = "Browser sign-ins: to the auth app"),
        (status = 400, description = "Missing code or invalid state", body = ErrorResponse),
        (status = 403, description = "The Google account's email is not verified, or no Stargate account uses it and `SIGNUP_MODE` does not create one (`social.account_not_provisioned`, `signup.domain_not_allowed`)", body = ErrorResponse),
        (status = 404, description = "Google sign-in is not configured on this server (`feature.disabled`)", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse),
        (status = 502, description = "Bad Gateway", body = ErrorResponse),
    )
)]
pub async fn get_google(req: Request) -> Response {
    super::callback(req, Provider::Google).await
}

/// The account behind a Google authorization `code`: the one using the
/// Google account's verified email, created on a first sign-in when
/// `SIGNUP_MODE` allows.
pub(super) async fn sign_in(
    code: &str,
    audit_request: TrustedAuditRequest,
) -> Result<db::ent::User, ErrorResponse> {
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

    let user = match crate::db::get_user_by_email(&email).await.map_err(|e| {
        tracing::error!("Database error during Google OAuth: {}", e);
        ErrorResponse::new(ErrorCode::SystemInternal)
    })? {
        Some(user) => user,
        None => {
            super::ensure_provisioning("Google", &email)?;
            create_google_user(&google_user, &email, audit_context.clone()).await?
        }
    };

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
    Ok(user)
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
