use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::ext::RequestExt;
use crate::etc::jwt::jwt_config;
use crate::etc::sub::Subject;
use axum::Json;
use axum::extract::Request;
use serde::Serialize;
use store::Store;

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
}

impl From<db::ent::User> for UserSchema {
    fn from(user: db::ent::User) -> Self {
        UserSchema {
            id: user.id,
            email: user.email,
            given_name: user.given_name,
            family_name: user.family_name,
            nickname: user.nickname,
            picture: user.picture,
            phone_number: user.phone_number,
        }
    }
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
    let token = req.get_token().ok_or_else(|| {
        ErrorResponse::from(HttpError::Unauthorized("Token not found".to_string()))
    })?;

    let claims = match jwt_config().validate_session_access_token(&token) {
        Ok(c) => c,
        _ => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )));
        }
    };

    if crate::act::token_revocation::is_revoked(&claims)
        .await
        .map_err(ErrorResponse::internal)?
    {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid Token".to_string(),
        )));
    }

    let sid = claims.sid.clone().unwrap_or_default();
    let session = etc::store::use_store()
        .get::<Subject>(&sid)
        .await
        .unwrap_or(None);
    if session.is_none() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid Token".to_string(),
        )));
    }

    let user = crate::db::get_user_by_username(&claims.sub)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::from(HttpError::DocumentNotFound("User not found".to_string()))
        })?;

    Ok(Json(user.into()))
}
