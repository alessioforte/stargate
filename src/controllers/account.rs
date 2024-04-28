use actix_web::{post, put, web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::models::tokens::Token;
use crate::models::users::User;
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
    let refresh_token_hash = Hash::encoded(&refresh_token).unwrap();

    Token::upsert(Token {
        id: user.id.clone(),
        value: refresh_token_hash.clone(),
        expires_at: 60,
        issued_at: 0,
    })
    .await
    .unwrap();

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
#[put("/refresh")]
pub async fn refresh(body: web::Json<RefreshTokenRequestBody>) -> impl Responder {
    let body = body.into_inner();
    let refresh_token = body.refresh_token.clone();
    let claims = match validate_token(&refresh_token) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(web::Json("Invalid Token")),
    };

    println!("Claims: {:?}", claims);
    let token = Token::get(claims.sub_id.clone()).await.unwrap();
    println!("Token here: {:?}", token);
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
    let refresh_token_hash = Hash::encoded(&refresh_token).unwrap();

    Token::upsert(Token {
        id: user.id.clone(),
        value: refresh_token_hash.clone(),
        expires_at: 60,
        issued_at: 0,
    })
    .await
    .unwrap();

    HttpResponse::Ok().json(web::Json(AuthResponse {
        access_token,
        refresh_token,
    }))
}

// ----------------------------------------------------------------------------
#[post("/logout")]
pub async fn logout() -> impl Responder {
    HttpResponse::Ok().json(web::Json("Logout User"))
}

// ----------------------------------------------------------------------------
// forgot password request
#[post("/forgot")]
pub async fn forgot_password() -> impl Responder {
    HttpResponse::Ok().json(web::Json("Forgot Password"))
}

// ----------------------------------------------------------------------------
// change password request
#[put("/change")]
pub async fn change_password() -> impl Responder {
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
