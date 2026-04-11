use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use crate::etc::ext::RequestExt;
use crate::etc::sub::Subject;
use actix_web::{HttpRequest, HttpResponse, get, web};
use etc::jwt::jwt_config;
use serde::Serialize;
use serde_json::Value;
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
    pub attrs: Value,
}

#[utoipa::path(
    context_path = "/account",
    path = "/profile",
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
#[get("/profile")]
pub async fn handler(req: HttpRequest) -> Result<HttpResponse, ErrorResponse> {
    let token = match req.get_token() {
        Some(t) => t,
        None => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Token not found".to_string(),
            )));
        }
    };

    let jwt = jwt_config();
    let claims = match jwt.validate_token(&token) {
        Ok(claims) if claims.typ.as_deref() == Some("bearer") => claims,
        Ok(_) | Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )));
        }
    };

    // Validate session store — reject revoked/logged-out tokens
    let sid = claims.sid.clone().unwrap_or_default();
    let store = etc::store::use_store();
    let session = store.get::<Subject>(&sid).await.unwrap_or(None);
    if session.is_none() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid Token".to_string(),
        )));
    }

    let user = match crate::db::get_user_by_username(&claims.sub).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::internal(e));
        }
    };

    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();
    Ok(HttpResponse::Ok().json(web::Json(user)))
}
