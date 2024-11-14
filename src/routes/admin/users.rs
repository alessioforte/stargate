use crate::errors::{ErrorResponse, HttpError};
use crate::models::users::{Payload as UserPayload, User};
use crate::modules::hash::Hash;
use actix_web::{delete, get, post, web, HttpResponse};
use actix_web_grants::protect;

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
    let users = User::list().await.unwrap();
    Ok(HttpResponse::Ok().json(web::Json(users)))
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
pub async fn create_user(user: web::Json<UserPayload>) -> Result<HttpResponse, ErrorResponse> {
    let password = match &user.password {
        Some(password) => Some(Hash::encode(password).unwrap()),
        None => return Err(HttpError::BadRequest("Password is required".to_string()).into()),
    };
    let user = User::new()
        .name(user.name.clone())
        .email(user.email.clone())
        .nickname(user.nickname.clone())
        .password(password)
        .phone_number(user.phone_number.clone())
        .picture(user.picture.clone())
        .save()
        .await
        .unwrap();

    // TODO: Send email to user

    Ok(HttpResponse::Ok().json(web::Json(user)))
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
pub async fn delete_user(params: web::Path<String>) -> Result<HttpResponse, ErrorResponse> {
    let id = params.into_inner();
    let user = User::delete(&id).await.unwrap();
    Ok(HttpResponse::Ok().json(web::Json(user)))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/users")
        .service(get_users)
        .service(create_user)
        .service(delete_user)
}
