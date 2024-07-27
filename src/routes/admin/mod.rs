pub mod configurations;
pub mod docs;
pub mod users;

use crate::data::AppData;
use crate::modules::auth::validate_token;
use actix_web::{
    body::BoxBody, body::EitherBody, dev::ServiceFactory, dev::ServiceRequest,
    dev::ServiceResponse, http::header::Header, web, Error,
};
use actix_web_grants::GrantsMiddleware;
use actix_web_httpauth::headers::authorization::{Authorization, Bearer};
use std::collections::HashSet;
use utoipa::OpenApi;

const SUPER_ADMIN: &str = "SUPER_ADMIN";

async fn extract(req: &mut ServiceRequest) -> Result<HashSet<String>, Error> {
    let auth = Authorization::<Bearer>::parse(&req);
    let token = match auth {
        Ok(auth) => auth.into_scheme().token().to_string(),
        Err(_) => "".to_string(),
    };
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
        // TODO: Add middleware to extract claims
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
