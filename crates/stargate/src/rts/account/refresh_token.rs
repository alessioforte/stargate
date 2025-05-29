use super::AuthResponse;
use super::RefreshTokenRequestBody;
use crate::err::{ErrorResponse, HttpError};
use actix_web::{put, web, HttpResponse};
use db::ent::token::Token;
use db::ent::user::User;
use jwt::{jwt_config, Claims};
use password::Hash;

#[utoipa::path(
    context_path = "/account",
    path = "/refresh-token",
    responses(
        (status = 200, description = "OK")
    )
)]
#[put("/refresh-token")]
pub async fn handler(
    body: web::Json<RefreshTokenRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();
    let refresh_token = body.refresh_token.clone();
    let jwt = jwt_config();
    let claims = match jwt.validate_token(&refresh_token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )))
        }
    };

    let token = Token::get(&claims.sub_id).await.unwrap();
    if token.is_none() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid Token".to_string(),
        )));
    }

    let token = token.unwrap();

    if Hash::verify(&refresh_token, &token.value).is_err() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Invalid Token".to_string(),
        )));
    }

    let user = User::get(&claims.sub_id).await.unwrap();

    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();

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

    Token::save(Token {
        id: user.id.clone(),
        value: refresh_token_hash.clone(),
    })
    .await
    .unwrap();

    Ok(HttpResponse::Ok().json(web::Json(AuthResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
    })))
}
