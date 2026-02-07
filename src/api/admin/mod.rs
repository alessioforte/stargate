pub mod admin_keys;
pub mod api_keys;
pub mod configurations;
pub mod service_accounts;
pub mod users;

use crate::etc::{ext::RequestExt, jwt::jwt_config};
use crate::fun::check_super_admin_by_claims;
use actix_web::{
    Error, body::BoxBody, body::EitherBody, dev::ServiceFactory, dev::ServiceRequest,
    dev::ServiceResponse, web,
};
use actix_web_grants::GrantsMiddleware;
use std::collections::HashSet;

const SUPER_ADMIN: &str = "super_admin";

async fn extract(req: &mut ServiceRequest) -> Result<HashSet<String>, Error> {
    if let Some(token) = req.request().get_token() {
        let jwt = jwt_config();
        if let Some(claims) = jwt.validate_token(&token).ok() {
            if check_super_admin_by_claims(claims) {
                return Ok(HashSet::from([SUPER_ADMIN.to_string()]));
            }
        }
    }

    if let Some(raw_key) = req.request().get_api_key() {
        let hash = pw::hash_api_key(&raw_key);
        if let Ok(Some(key)) = crate::db::get_admin_key_by_hash(&hash).await {
            if !key.revoked {
                return Ok(key.permissions.iter().map(|p| p.to_string()).collect());
            }
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
        .service(admin_keys::routes())
        .service(service_accounts::routes())
}
