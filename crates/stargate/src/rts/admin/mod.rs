pub mod configurations;
pub mod docs;
pub mod users;

use crate::etc::jwt::jwt_config;
use crate::{act::SUPER_ADMIN_NICKNAME, etc::ext::RequestExt};
use actix_web::{
    body::BoxBody, body::EitherBody, dev::ServiceFactory, dev::ServiceRequest,
    dev::ServiceResponse, web, Error,
};
use actix_web_grants::GrantsMiddleware;
use std::collections::HashSet;
use utoipa::OpenApi;

const SUPER_ADMIN: &str = "SUPER_ADMIN";

async fn extract(req: &mut ServiceRequest) -> Result<HashSet<String>, Error> {
    let token = req.request().get_token();
    let jwt = jwt_config();
    let claims = match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Ok(HashSet::new());
        }
    };

    if claims.nickname == Some(SUPER_ADMIN_NICKNAME.to_string()) {
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
        crate::rts::admin::configurations::get_configurations,
        crate::rts::admin::configurations::update_configurations,
        crate::rts::admin::users::get_users,
        crate::rts::admin::users::create_user,
        crate::rts::admin::users::delete_user,
    ),
    info(description = "Stargate APIs Admin documentation")
)]
pub struct ApiDoc;
