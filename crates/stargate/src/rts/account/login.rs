use super::{AuthResponse, UserCredentials};
use crate::act::format_name;
use crate::err::{ErrorResponse, HttpError};
use crate::etc;
use actix_session::Session;
use actix_web::{post, web, HttpResponse};
use db::ent::CredentialType;
use db::Transaction;
use password::Hash;

#[utoipa::path(
    context_path = "/account",
    path = "/login",

    responses(
        (status = 200, description = "OK", body = AuthResponse)
    )
)]
#[post("/login")]
pub async fn handler(
    session: Session,
    credentials: web::Json<UserCredentials>,
) -> Result<HttpResponse, ErrorResponse> {
    let service = etc::db::service();
    let user = match service.get_user_by_username(&credentials.username).await {
        Ok(user) => user,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )))
        }
    };

    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid username".to_string(),
        )));
    }

    let user = user.unwrap();
    let user_credential = match service
        .get_credential(&user.id, CredentialType::Password)
        .await
    {
        Ok(credential) => credential,
        Err(e) => {
            return Err(ErrorResponse::from(HttpError::InternalServerError(
                e.to_string(),
            )));
        }
    };

    let password = user_credential.unwrap().value;

    if Hash::verify(&credentials.password, &password).is_err() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid password".to_string(),
        )));
    }

    let first_name = user.first_name.clone().unwrap_or_default();
    let last_name = user.last_name.clone().unwrap_or_default();
    let name = format_name(&first_name, &last_name);

    let (access_token, refresh_token) = crate::act::generate_tokens(jwt::Claims {
        sub: user.email.to_owned(),
        sub_id: Some(user.id.to_owned()),
        name: Some(name),
        email: user.email.clone(),
        nickname: user.nickname.clone(),
        email_verified: true,
        ..jwt::Claims::default()
    })
    .unwrap();

    session.insert("token", access_token.clone()).unwrap();

    Ok(HttpResponse::Ok().json(web::Json(AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
    })))
}
