use super::{AuthResponse, UserCredentials};
use crate::ent::token::Token;
use crate::ent::user::User;
use crate::err::{ErrorResponse, HttpError};
use actix_session::Session;
use actix_web::{post, web, HttpResponse};
use jwt::{jwt_config, Claims};
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
    let user = match User::get_by_username(&credentials.username).await {
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
    let password = user.password.clone().unwrap();

    if Hash::verify(&credentials.password, &password).is_err() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid password".to_string(),
        )));
    }

    let jwt = jwt_config();
    let (access_token, refresh_token) = jwt
        .create_tokens(Claims {
            sub: user.email.to_owned(),
            sub_id: user.id.to_owned(),
            name: Some(user.name.clone()),
            email: user.email.clone(),
            nickname: user.nickname.clone(),
            email_verified: true,
            ..Claims::default()
        })
        .unwrap();

    let refresh_token_hash = Hash::encode(&refresh_token).unwrap();

    let upsert_token = Token::save(Token {
        id: user.id.clone(),
        value: refresh_token_hash.clone(),
    })
    .await;
    if upsert_token.is_err() {
        log::error!("Could not create token: {:?}", upsert_token.err());
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not create token".to_string(),
        )));
    }

    session.insert("token", access_token.clone()).unwrap();

    Ok(HttpResponse::Ok().json(web::Json(AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
    })))
}
