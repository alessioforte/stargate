pub mod api_keys;
pub mod configurations;
pub mod docs;
pub mod users;

use crate::etc::{consts::STARGATE_ADMIN, ext::RequestExt, jwt::jwt_config};
use actix_web::{
    Error, body::BoxBody, body::EitherBody, dev::ServiceFactory, dev::ServiceRequest,
    dev::ServiceResponse, web,
};
use actix_web_grants::GrantsMiddleware;
use std::collections::HashSet;
use utoipa::OpenApi;

const SUPER_ADMIN: &str = "SUPER_ADMIN";

async fn extract(req: &mut ServiceRequest) -> Result<HashSet<String>, Error> {
    // TODO: add admin api keys support ?

    let token = req.request().get_token();
    let jwt = jwt_config();
    let claims = match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Ok(HashSet::new());
        }
    };

    if let Some(role) = claims.role {
        if role == STARGATE_ADMIN {
            return Ok(HashSet::from([SUPER_ADMIN.to_string()]));
        }
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
        .service(api_keys::routes())
        .service(docs::routes())
}

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::rts::admin::docs::get_api_doc,
        crate::rts::admin::configurations::get_configurations,
        crate::rts::admin::configurations::update_configurations,
        crate::rts::admin::users::get_users,
        crate::rts::admin::users::create_user,
        crate::rts::admin::users::delete_user,
        crate::rts::admin::users::update_user,
        crate::rts::admin::users::patch_user,
        crate::rts::admin::api_keys::get_api_keys,
        crate::rts::admin::api_keys::create_api_key,
        crate::rts::admin::api_keys::delete_api_key,
    ),
    info(description = "Stargate APIs Admin documentation")
)]
pub struct ApiDoc;
