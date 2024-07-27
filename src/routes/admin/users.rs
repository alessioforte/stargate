use crate::models::users::{Payload as UserPayload, User};
use crate::modules::hash::Hash;
use actix_web::{delete, get, post, web, HttpResponse, Responder};
use actix_web_grants::protect;

#[utoipa::path(
    context_path = "/admin/users",
    path = "/",
    responses(
        (status = 200, description = "OK", body = Health)
    )
)]
#[get("")]
#[protect("SUPER_ADMIN")]
pub async fn get_users() -> impl Responder {
    let users = User::get_all().await.unwrap();
    HttpResponse::Ok().json(web::Json(users))
}

#[utoipa::path(
    context_path = "/admin/users",
    path = "/",
    responses(
        (status = 200, description = "OK", body = Health)
    )
)]
#[post("")]
#[protect("SUPER_ADMIN")]
pub async fn create_user(user: web::Json<UserPayload>) -> impl Responder {
    let new_user = UserPayload {
        email: user.email.clone(),
        name: user.name.clone(),
        nickname: user.nickname.clone(),
        password: Hash::encode(&user.password).unwrap(),
        phone_number: user.phone_number.clone(),
        picture: user.picture.clone(),
    };
    let user = User::create(new_user).await.unwrap();
    HttpResponse::Ok().json(web::Json(user))
}

#[utoipa::path(
    context_path = "/admin/users",
    path = "/{id}",
    responses(
        (status = 200, description = "OK", body = Health)
    )
)]
#[delete("/{id}")]
#[protect("SUPER_ADMIN")]
pub async fn delete_user(params: web::Path<String>) -> impl Responder {
    let id = params.into_inner();
    let user = User::delete(id).await.unwrap();
    HttpResponse::Ok().json(web::Json(user))
}

pub fn routes() -> actix_web::Scope {
    web::scope("/users")
        .service(get_users)
        .service(create_user)
        .service(delete_user)
}
