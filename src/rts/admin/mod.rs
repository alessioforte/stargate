pub mod configurations;
pub mod docs;
pub mod users;

use crate::act::{get_token_from_request, SUPER_ADMIN_NICKNAME};
use crate::pks::jwt;
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
    let jwt = jwt::jwt_config();
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
