use actix_web::http::header::Header;
use actix_web::{delete, post, put, web, HttpRequest, HttpResponse, Responder};
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::resets::{NewPasswordReset, PasswordReset};
use crate::models::tokens::Token;
use crate::models::users::User;
use crate::services::smtp::send_email;
use crate::utils::auth::{create_token, validate_token};
use crate::utils::hash::Hash;

// ----------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct UserCredentials {
    email: String,
    password: String,
}

// TODO: verify access from another device and notify user
#[post("/login")]
pub async fn login(credentials: web::Json<UserCredentials>) -> impl Responder {
    let user = User::get_by_email(credentials.email.clone()).await.unwrap();
    if user.is_none() {
        return HttpResponse::NotFound().json(web::Json("User not found"));
    }
    let user = user.unwrap();

    let is_valid: bool = match Hash::verify(&credentials.password, &user.password) {
        Ok(_) => true,
        Err(_) => false,
    };

    if !is_valid {
        return HttpResponse::Unauthorized().json(web::Json("Invalid password"));
    }

    let access_token = create_token(&user, 60).unwrap(); // expires in 60 minutes
    let refresh_token = create_token(&user, 1440).unwrap(); // expires in 24 hours

    let refresh_token_hash = Hash::encode(&refresh_token).unwrap();

    let upsert_token = Token::upsert(Token {
        id: user.id.clone(),
        value: refresh_token_hash.clone(),
    })
    .await;
    if upsert_token.is_err() {
        log::error!("Could not create token: {:?}", upsert_token.err());
        return HttpResponse::InternalServerError().json(web::Json("Could not create token"));
    }

    HttpResponse::Ok().json(web::Json(AuthResponse {
        access_token,
        refresh_token,
    }))
}

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize)]
struct RefreshTokenRequestBody {
    refresh_token: String,
}
#[put("/refresh-token")]
pub async fn refresh(body: web::Json<RefreshTokenRequestBody>) -> impl Responder {
    let body = body.into_inner();
    let refresh_token = body.refresh_token.clone();
    let claims = match validate_token(&refresh_token) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(web::Json("Invalid Token")),
    };

    let token = Token::get(claims.sub_id.clone()).await.unwrap();
    if token.is_none() {
        return HttpResponse::NotFound().json(web::Json("Token not found"));
    }

    let token = token.unwrap();
    let is_valid: bool = match Hash::verify(&refresh_token, &token.value) {
        Ok(_) => true,
        Err(_) => false,
    };

    if !is_valid {
        return HttpResponse::Unauthorized().json(web::Json("Invalid Token"));
    }

    let user = User::get(claims.sub_id.clone()).await.unwrap();

    if user.is_none() {
        return HttpResponse::NotFound().json(web::Json("User not found"));
    }

    let user = user.unwrap();

    let access_token = create_token(&user, 60).unwrap(); // expires in 60 minutes
    let refresh_token = create_token(&user, 1440).unwrap(); // expires in 24 hours
    let refresh_token_hash = Hash::encode(&refresh_token).unwrap();

    Token::upsert(Token {
        id: user.id.clone(),
        value: refresh_token_hash.clone(),
    })
    .await
    .unwrap();

    HttpResponse::Ok().json(web::Json(AuthResponse {
        access_token,
        refresh_token,
    }))
}

// ----------------------------------------------------------------------------
#[delete("/logout")]
pub async fn logout(req: HttpRequest) -> impl Responder {
    let auth = Authorization::<Bearer>::parse(&req);
    let token = match auth {
        Ok(auth) => auth.into_scheme().token().to_string(),
        Err(_) => "".to_string(),
    };

    let claims = match validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(web::Json("Invalid Token")),
    };

    let response = Token::delete(claims.sub_id.clone()).await;
    if response.is_err() {
        return HttpResponse::NotFound().json(web::Json("Token not found"));
    }

    HttpResponse::Ok().json(web::Json("Logout User"))
}

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize)]
struct ForgotPasswordRequestBody {
    email: String,
}
// forgot password request
#[post("/forgot-password")]
pub async fn forgot_password(body: web::Json<ForgotPasswordRequestBody>) -> impl Responder {
    let body = body.into_inner();
    let email = body.email.clone();
    let user = User::get_by_email(email.clone()).await.unwrap();
    if user.is_none() {
        return HttpResponse::NotFound().json(web::Json("User not found"));
    }

    let user = user.unwrap();
    // TODO: use JWT instead of UUID
    let uuid = Uuid::new_v4().to_string();
    let reset = PasswordReset::create(NewPasswordReset {
        email: user.email.clone(),
        uuid: uuid.clone(),
        issued_at: Utc::now().timestamp(),
        expires_at: (Utc::now() + Duration::days(1)).timestamp(),
    })
    .await;

    if reset.is_err() {
        return HttpResponse::InternalServerError().json(web::Json("Could not create reset"));
    }

    // send email
    match send_email(user.email.clone(), uuid.clone()) {
        Ok(_) => HttpResponse::Ok().json(web::Json("Reset request sent")),
        Err(_) => HttpResponse::InternalServerError().json(web::Json("Could not send email")),
    }
}

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize)]
struct ChangePasswordRequestBody {
    token: String,
    password: String,
}
#[put("/reset-password")]
pub async fn change_password(body: web::Json<ChangePasswordRequestBody>) -> impl Responder {
    let body = body.into_inner();
    let token = body.token.clone();
    let change_request = PasswordReset::get_by_uuid(token.clone()).await.unwrap();
    if change_request.is_none() {
        return HttpResponse::NotFound().json(web::Json("Change request not found"));
    }
    let change_request = change_request.unwrap();

    if change_request.expires_at < Utc::now().timestamp() {
        return HttpResponse::BadRequest().json(web::Json("Change request expired"));
    }

    let user = User::get_by_email(change_request.email.clone())
        .await
        .unwrap();
    if user.is_none() {
        return HttpResponse::NotFound().json(web::Json("User not found"));
    }

    let user = user.unwrap();
    let password = Hash::encode(&body.password).unwrap();
    let response = User::change_password(user.id.clone(), password.clone()).await;
    if response.is_err() {
        log::error!("Could not update password: {:?}", response.err());
        return HttpResponse::InternalServerError().json(web::Json("Could not update password"));
    }

    let response = PasswordReset::delete(change_request.id.clone()).await;
    if response.is_err() {
        return HttpResponse::InternalServerError().json(web::Json("Could not delete reset"));
    }

    // send email to notify user of password change
    HttpResponse::Ok().json(web::Json("Change Password"))
}

// ----------------------------------------------------------------------------
pub fn routes() -> actix_web::Scope {
    web::scope("/account")
        .service(login)
        .service(refresh)
        .service(logout)
        .service(forgot_password)
        .service(change_password)
}
