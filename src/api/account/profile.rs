use crate::ctx::audit_request_from;
use crate::data::store::use_store;
use crate::err::{ErrorCode, ErrorResponse};
use crate::iam::auth::jwt::jwt_config;
use crate::iam::auth::subject::Subject;
use crate::iam::user::profile::{locale as user_locale, with_locale};
use crate::web::request::RequestExt;
use axum::Json;
use axum::extract::{FromRequest, Request};
use db::ent::{TrustedAuditActor, TrustedAuditContext};
use serde::{Deserialize, Serialize};
use store::Store;
use tools::Locale;

#[derive(Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserSchema {
    pub id: String,
    pub email: String,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub nickname: String,
    pub picture: Option<String>,
    pub phone_number: Option<String>,
    /// The saved language (`users.attrs.locale`), when one is set.
    pub locale: Option<String>,
}

impl From<db::ent::User> for UserSchema {
    fn from(user: db::ent::User) -> Self {
        let locale = user_locale(&user).map(|locale| locale.as_str().to_string());
        UserSchema {
            id: user.id,
            email: user.email,
            given_name: user.given_name,
            family_name: user.family_name,
            nickname: user.nickname,
            picture: user.picture,
            phone_number: user.phone_number,
            locale,
        }
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProfileRequest {
    pub locale: String,
}

/// Authenticate a profile request with a native session token or, for the
/// Console, an OAuth access token. Both must belong to a live session.
async fn session_user(token: String) -> Result<db::ent::User, ErrorResponse> {
    let jwt = jwt_config();
    let claims = jwt
        .validate_session_access_token(&token)
        .or_else(|_| jwt.validate_oauth_access_token(&token, None))
        .map_err(|_| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;

    if crate::iam::auth::revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }

    let sid = claims
        .sid
        .as_deref()
        .filter(|sid| !sid.is_empty())
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
    let session = use_store().get::<Subject>(sid).await.unwrap_or(None);
    if session.is_none() {
        return Err(ErrorResponse::new(ErrorCode::AuthTokenInvalid));
    }

    // By id: the email in `sub` may have changed, and a lookup by address
    // must never resolve to another account's nickname.
    let user_id = claims
        .sub_id
        .as_deref()
        .filter(|user_id| !user_id.is_empty())
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenInvalid))?;
    crate::data::db::get_user_by_id(user_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| ErrorResponse::new(ErrorCode::UserNotFound))
}

#[utoipa::path(
    get,
    path = "/account/profile",
    tags = ["Account"],
    summary = "Get User Profile",
    description = "Retrieve the profile information of the currently authenticated user using their access token.",
    responses(
        (status = 200, description = "OK", body = UserSchema),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 404, description = "Not Found", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn get_profile(req: Request) -> Result<Json<UserSchema>, ErrorResponse> {
    let token = req
        .get_token()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenMissing))?;
    Ok(Json(session_user(token).await?.into()))
}

#[utoipa::path(
    put,
    path = "/account/profile",
    tags = ["Account"],
    summary = "Update User Profile",
    description = "Save the authenticated user's preferred language as `users.attrs.locale`.",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "OK", body = UserSchema),
        (status = 400, description = "Unsupported locale", body = ErrorResponse),
        (status = 401, description = "Unauthorized - Invalid Token", body = ErrorResponse),
        (status = 404, description = "Not Found", body = ErrorResponse),
        (status = 500, description = "Internal Server Error", body = ErrorResponse)
    )
)]
pub async fn put_profile(req: Request) -> Result<Json<UserSchema>, ErrorResponse> {
    let audit_request = audit_request_from(req.extensions());
    let token = req
        .get_token()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthTokenMissing))?;
    let user = session_user(token).await?;

    let Json(body) = Json::<UpdateProfileRequest>::from_request(req, &())
        .await
        .map_err(|error| {
            ErrorResponse::new(ErrorCode::RequestInvalidJson).with_message(error.body_text())
        })?;
    let locale = Locale::parse(&body.locale).ok_or_else(|| {
        ErrorResponse::new(ErrorCode::RequestInvalid).with_param("field", "locale")
    })?;

    let mut updated = user;
    updated.attrs = with_locale(updated.attrs, locale);
    let audit_context =
        TrustedAuditContext::application(TrustedAuditActor::user(&updated.id), audit_request);
    let user = crate::data::db::update_user(updated, audit_context)
        .await
        .map_err(ErrorResponse::internal)?;

    Ok(Json(user.into()))
}
