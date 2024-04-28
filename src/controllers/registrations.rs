use actix_web::{get, post, put, web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::utils::hash::Hash;
use crate::{
    models::registrations::{NewRegistration, Registration},
    models::users::{NewUser, User},
};

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize)]
struct RegistrationRequestBody {
    email: String,
}
#[post("")]
pub async fn registration_request(body: web::Json<RegistrationRequestBody>) -> impl Responder {
    let body = body.into_inner();

    let user = User::get_by_email(body.email.clone()).await.unwrap();
    if user.is_some() {
        return HttpResponse::Conflict().json(web::Json("Email already in use"));
    }
    let registration_request = Registration::get_by_email(body.email.clone())
        .await
        .unwrap();
    if registration_request.is_some() {
        return HttpResponse::Conflict().json(web::Json("Registration already requested"));
    }

    let uuid = Uuid::new_v4().to_string();
    let registration = NewRegistration {
        email: body.email.clone(),
        uuid: uuid.clone(),
    };

    Registration::create(registration).await.unwrap();

    HttpResponse::Ok().json(web::Json(uuid))

    // let is_success = send_email(body.email, token.clone());
    // if is_success {
    //     HttpResponse::Ok().json(web::Json(token))
    // } else {
    //     HttpResponse::InternalServerError().json(web::Json("Could not send email"))
    // }
}

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize)]
struct RegistrationConfirmParams {
    token: String,
}
#[get("")]
pub async fn registration_confirm(query: web::Query<RegistrationConfirmParams>) -> impl Responder {
    let query = query.into_inner();
    let token = query.token.clone();
    let registration = Registration::get_by_id(token.clone()).await.unwrap();
    HttpResponse::Ok().json(web::Json(registration))
}

// ----------------------------------------------------------------------------
#[derive(Debug, Serialize, Deserialize)]
struct RegistrationCompleteRequestBody {
    token: String,
    name: String,
    nickname: Option<String>,
    password: String,
}
#[put("")]
pub async fn registration_complete(
    body: web::Json<RegistrationCompleteRequestBody>,
) -> impl Responder {
    let body = body.into_inner();
    println!("Body: {:?}", body);

    // get registration by token
    let registration = Registration::get_by_id(body.token.clone()).await.unwrap();
    if registration.is_none() {
        return HttpResponse::NotFound().json(web::Json("Registration not found"));
    }
    // create user
    let registration = registration.unwrap();
    let user = NewUser {
        email: registration.email.clone(),
        name: body.name.clone(),
        nickname: body.nickname.clone(),
        password: Hash::encoded(&body.password).unwrap(),
        picture: None,
        phone_number: None,
    };

    User::create(user).await.unwrap();

    // delete registration
    println!("Registration: {:?}", registration);
    Registration::delete(registration.id.clone()).await.unwrap();

    HttpResponse::Ok().json(web::Json("Complete Registration"))
}

// ----------------------------------------------------------------------------
pub fn routes() -> actix_web::Scope {
    web::scope("/registrations")
        .service(registration_request)
        .service(registration_confirm)
        .service(registration_complete)
}
