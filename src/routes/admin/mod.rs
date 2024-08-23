pub mod configurations;
pub mod docs;
pub mod users;

use crate::actions::get_token_from_request;
use crate::etc::AppData;
use crate::modules::auth::validate_token;
use actix_web::{
    body::BoxBody, body::EitherBody, dev::ServiceFactory, dev::ServiceRequest,
    dev::ServiceResponse, web, Error,
};
use actix_web_grants::GrantsMiddleware;
use std::collections::HashSet;
use utoipa::OpenApi;

const SUPER_ADMIN: &str = "SUPER_ADMIN";

async fn extract(req: &mut ServiceRequest) -> Result<HashSet<String>, Error> {
    let token = get_token_from_request(req.request());
    let data = req.app_data::<AppData>().unwrap();
    let claims = match validate_token(&token, &data.jwt_secret) {
        Ok(claims) => claims,
        Err(_) => {
            return Ok(HashSet::new());
        }
    };

    if claims.nickname == Some("admin".to_string()) {
        return Ok(HashSet::from([SUPER_ADMIN.to_string()]));
    }

    Ok(HashSet::new())
}

pub fn routes() -> actix_web::Scope<
    impl ServiceFactory<
        ServiceRequest,
        Config = (),
        Response = ServiceResponse<EitherBody<BoxBody>>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    web::scope("/admin")
        .wrap(GrantsMiddleware::with_extractor(extract))
        .service(users::routes())
        .service(configurations::routes())
        .service(docs::routes())
}

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::routes::admin::configurations::get_configurations,
        crate::routes::admin::configurations::update_configurations,
        crate::routes::admin::users::get_users,
        crate::routes::admin::users::create_user,
        crate::routes::admin::users::delete_user,
    ),
    info(description = "Stargate Admin documentation.")
)]
pub struct ApiDoc;
