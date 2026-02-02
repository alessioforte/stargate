pub mod api_keys;
pub mod configurations;
pub mod users;

use crate::etc::{ext::RequestExt, jwt::jwt_config};
use crate::fun::check_super_admin_by_claims;
use actix_web::{
    Error, body::BoxBody, body::EitherBody, dev::ServiceFactory, dev::ServiceRequest,
    dev::ServiceResponse, web,
};
use actix_web_grants::GrantsMiddleware;
use std::collections::HashSet;

const SUPER_ADMIN: &str = "SUPER_ADMIN";

async fn extract(req: &mut ServiceRequest) -> Result<HashSet<String>, Error> {
    // TODO: add admin api keys support ?
    let token = match req.request().get_token() {
        Some(t) => t,
        None => {
            return Ok(HashSet::new());
        }
    };

    let jwt = jwt_config();
    let claims = match jwt.validate_token(&token) {
        Ok(claims) => claims,
        Err(_) => {
            return Ok(HashSet::new());
        }
    };

    if check_super_admin_by_claims(claims) {
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
        .service(api_keys::routes())
}
