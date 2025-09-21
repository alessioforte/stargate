use crate::err::ErrorResponse;
use actix_web::{delete, get, patch, post, put, web, HttpResponse};
use actix_web_grants::protect;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, utoipa::ToSchema)]
struct User {
    email: String,
    first_name: String,
    last_name: String,
    password: String,
    nickname: String,
}

#[utoipa::path(
    context_path = "/admin/users",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[get("")]
#[protect("SUPER_ADMIN")]
pub async fn get_users() -> Result<HttpResponse, ErrorResponse> {
    // TODO: implement get users logic
    Ok(HttpResponse::Ok().json(web::Json("List of users")))
}

#[utoipa::path(
    context_path = "/admin/users",
    path = "/",
    responses(
        (status = 200, description = "OK")
    )
)]
#[post("")]
#[protect("SUPER_ADMIN")]
pub async fn create_user(_user: web::Json<User>) -> Result<HttpResponse, ErrorResponse> {
    // TODO: implement create user logic
    Ok(HttpResponse::Ok().json(web::Json("User created successfully")))
}

#[utoipa::path(
    context_path = "/admin/users",
    path = "/{id}",
    responses(
        (status = 200, description = "OK")
    )
)]
#[put("/{id}")]
#[protect("SUPER_ADMIN")]
pub async fn update_user(
    _params: web::Path<String>,
    _user: web::Json<User>,
) -> Result<HttpResponse, ErrorResponse> {
    // TODO: implement update user logic
    Ok(HttpResponse::Ok().json(web::Json("User updated successfully")))
}

#[utoipa::path(
    context_path = "/admin/users",
    path = "/{id}",
    responses(
        (status = 200, description = "OK")
    )
)]
#[patch("/{id}")]
#[protect("SUPER_ADMIN")]
pub async fn patch_user(
    _params: web::Path<String>,
    _user: web::Json<User>,
) -> Result<HttpResponse, ErrorResponse> {
    // TODO: implement patch user logic
    Ok(HttpResponse::Ok().json(web::Json("User patched successfully")))
}

#[utoipa::path(
    context_path = "/admin/users",
    path = "/{id}",
    responses(
        (status = 200, description = "OK")
    )
)]
#[delete("/{id}")]
#[protect("SUPER_ADMIN")]
pub async fn delete_user(_params: web::Path<String>) -> Result<HttpResponse, ErrorResponse> {
    // TODO: implement delete user logic
    Ok(HttpResponse::Ok().json(web::Json("User deleted successfully")))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/users")
        .service(get_users)
        .service(create_user)
        .service(update_user)
        .service(patch_user)
        .service(delete_user)
}
