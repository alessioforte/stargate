use crate::models::users::{NewUser, User};
use crate::modules::hash::Hash;
use actix_web::{delete, get, post, web, HttpResponse, Responder};
use actix_web_grants::protect;

#[get("")]
#[protect("SUPER_ADMIN")]
pub async fn get_users() -> impl Responder {
    let users = User::get_all().await.unwrap();
    HttpResponse::Ok().json(web::Json(users))
}

#[post("")]
#[protect("SUPER_ADMIN")]
pub async fn create_user(user: web::Json<NewUser>) -> impl Responder {
    let new_user = NewUser {
        email: user.email.clone(),
        name: user.name.clone(),
        nickname: user.nickname.clone(),
        password: Hash::encode(&user.password).unwrap(),
        phone_number: user.phone_number.clone(),
        picture: user.picture.clone(),
    };
    println!("new user{:?}", new_user);
    let user = User::create(new_user).await.unwrap();
    HttpResponse::Ok().json(web::Json(user))
}

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
