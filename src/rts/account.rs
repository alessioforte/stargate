use crate::act::get_token_from_request;
use crate::ent::resets::{PasswordReset, Payload as PasswordResetPayload};
use crate::ent::tokens::Token;
use crate::ent::users::{Profile, User};
use crate::err::{ErrorResponse, HttpError};
use crate::pks::auth::{create_tokens, validate_token, Claims};
use crate::pks::hash::Hash;
use crate::svc::smtp::{Smtp, Template};
use actix_session::Session;
use actix_web::{delete, get, post, put, web, HttpRequest, HttpResponse};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[utoipa::path(
    context_path = "/account",
    path = "/profile",
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("/profile")]
pub async fn profile(req: HttpRequest) -> Result<HttpResponse, ErrorResponse> {
    let token = get_token_from_request(&req);

    let claims = match validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )))
        }
    };

    let user = User::get(&claims.sub_id).await.unwrap();
    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();
    Ok(HttpResponse::Ok().json(web::Json(Profile {
        id: user.id.clone(),
        name: user.name.clone(),
        email: user.email.clone(),
        nickname: user.nickname.clone(),
        picture: user.picture.clone(),
        phone_number: user.phone_number.clone(),
    })))
}

// ----------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
}
#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct UserCredentials {
    username: String,
    password: String,
}

#[utoipa::path(
    context_path = "/account",
    path = "/login",

    responses(
        (status = 200, description = "OK", body = AuthResponse)
    )
)]
#[post("/login")]
pub async fn login(
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

    let (access_token, refresh_token) = create_tokens(Claims {
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
    })))
}

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct RefreshTokenRequestBody {
    refresh_token: String,
}

#[utoipa::path(
    context_path = "/account",
    path = "/refresh-token",
    responses(
        (status = 200, description = "OK")
    )
)]
#[put("/refresh-token")]
pub async fn refresh(
    body: web::Json<RefreshTokenRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();
    let refresh_token = body.refresh_token.clone();
    let claims = match validate_token(&refresh_token) {
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

    let (access_token, refresh_token) = create_tokens(Claims {
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
    })))
}

#[utoipa::path(
    context_path = "/account",
    path = "/logout",
    responses(
        (status = 200, description = "OK")
    )
)]
#[delete("/logout")]
pub async fn logout(req: HttpRequest) -> Result<HttpResponse, ErrorResponse> {
    let token = get_token_from_request(&req);

    let claims = match validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err(ErrorResponse::from(HttpError::Unauthorized(
                "Invalid Token".to_string(),
            )))
        }
    };

    let response = Token::delete(&claims.sub_id).await;
    if response.is_err() {
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not delete token".to_string(),
        )));
    }

    Ok(HttpResponse::Ok().json(web::Json("Logout User")))
}

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ForgotPasswordRequestBody {
    email: String,
}

#[utoipa::path(
    context_path = "/account",
    path = "/forgot-password",
    responses(
        (status = 200, description = "OK")
    )
)]
#[post("/forgot-password")]
pub async fn forgot_password(
    body: web::Json<ForgotPasswordRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();
    let user = User::get_by_email(&body.email).await.unwrap();
    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();
    // TODO: use JWT instead of UUID
    let uuid = Uuid::new_v4().to_string();
    let reset = PasswordReset::save(PasswordResetPayload {
        email: user.email.clone(),
        uuid: uuid.clone(),
        issued_at: Utc::now().timestamp(),
        expires_at: (Utc::now() + Duration::days(1)).timestamp(),
    })
    .await;

    if reset.is_err() {
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not create reset request".to_string(),
        )));
    }

    let sender = Smtp::new()
        .template(Template::ChangePasswordRequest)
        .to(user.email.clone())
        .name(Some(user.name.clone()))
        .token(uuid.clone())
        .build()
        .send();

    match sender {
        Ok(_) => Ok(HttpResponse::Ok().json(web::Json("Email sent"))),
        Err(e) => Err(ErrorResponse::from(HttpError::InternalServerError(
            e.to_string(),
        ))),
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
struct ChangePasswordRequestBody {
    token: String,
    password: String,
}

#[utoipa::path(
    context_path = "/account",
    path = "/reset-password",
    responses(
        (status = 200, description = "OK")
    )
)]
#[put("/reset-password")]
pub async fn change_password(
    body: web::Json<ChangePasswordRequestBody>,
) -> Result<HttpResponse, ErrorResponse> {
    let body = body.into_inner();
    let token = body.token.clone();
    let change_request = PasswordReset::get_by_uuid(&token).await.unwrap();
    if change_request.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "Change request not found".to_string(),
        )));
    }
    let change_request = change_request.unwrap();

    if change_request.expires_at < Utc::now().timestamp() {
        return Err(ErrorResponse::from(HttpError::Unauthorized(
            "Change request expired".to_string(),
        )));
    }

    let user = User::get_by_email(&change_request.email).await.unwrap();
    if user.is_none() {
        return Err(ErrorResponse::from(HttpError::DocumentNotFound(
            "User not found".to_string(),
        )));
    }

    let user = user.unwrap();
    let password = Hash::encode(&body.password).unwrap();
    let response = User::change_password(&user.id, &password).await;
    if response.is_err() {
        log::error!("Could not update password: {:?}", response.err());
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not update password".to_string(),
        )));
    }

    let response = PasswordReset::delete(&change_request.id).await;
    if response.is_err() {
        return Err(ErrorResponse::from(HttpError::InternalServerError(
            "Could not delete change request".to_string(),
        )));
    }

    // send email to notify user of password change
    Ok(HttpResponse::Ok().json(web::Json("Change Password")))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/account")
        .service(profile)
        .service(login)
        .service(refresh)
        .service(logout)
        .service(forgot_password)
        .service(change_password)
}
