use crate::err::ErrorResponse;
use actix_web::{delete, get, post, web, HttpResponse};
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
    Ok(HttpResponse::Ok().json(web::Json("User created successfully")))
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
    Ok(HttpResponse::Ok().json(web::Json("User deleted successfully")))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/users")
        .service(get_users)
        .service(create_user)
        .service(delete_user)
}
