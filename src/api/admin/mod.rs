pub mod admin_keys;
pub mod api_keys;
pub mod configurations;
pub mod health;
pub mod organizations;
pub mod service_accounts;
pub mod users;

use crate::etc::{self, ext::RequestExt, jwt::jwt_config, sub::Subject};
use crate::fun::check_super_admin_by_claims;
use actix_web::HttpMessage;
use actix_web::{
    Error, body::BoxBody, body::EitherBody, dev::ServiceFactory, dev::ServiceRequest,
    dev::ServiceResponse, web,
};
use actix_web_grants::GrantsMiddleware;
use db::ent::AuditContext;
use std::collections::HashSet;
use store::Store;

const SUPER_ADMIN: &str = "super_admin";

async fn extract(req: &mut ServiceRequest) -> Result<HashSet<String>, Error> {
    let mut ctx = req
        .extensions_mut()
        .remove::<AuditContext>()
        .unwrap_or_else(AuditContext::anonymous);
    if let Some(token) = req.request().get_token() {
        let jwt = jwt_config();
        if let Some(claims) = jwt.validate_token(&token).ok() {
            // Validate session store — reject revoked/logged-out tokens
            let sid = claims.sid.clone().unwrap_or_default();
            let store = etc::store::use_store();
            let session = store.get::<Subject>(&sid).await.unwrap_or(None);
            if session.is_some() && check_super_admin_by_claims(&claims) {
                ctx = ctx.with_actor(db::ent::ActorType::Admin, claims.sub_id);
                req.extensions_mut().insert(ctx);
                return Ok(HashSet::from([SUPER_ADMIN.to_string()]));
            }
        }
    }

    if let Some(raw_key) = req.request().get_api_key() {
        let hash = pw::hash_api_key(&raw_key);
        if let Ok(Some(key)) = crate::db::get_admin_key_by_hash(&hash).await {
            if !key.revoked {
                ctx = ctx.with_actor(db::ent::ActorType::AdminKey, Some(key.id));
                req.extensions_mut().insert(ctx);
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
        .service(health::get)
        .service(users::routes())
        .service(configurations::routes())
        .service(api_keys::routes())
        .service(admin_keys::routes())
        .service(organizations::routes())
        .service(service_accounts::routes())
}
