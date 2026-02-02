use crate::etc::{ext::RequestExt, guard, sub::SubjectType};
use actix_web::{
    Error, HttpMessage,
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    middleware::Next,
};
use db::ent::{ActorType, AuditContext};
use ulid::Ulid;

pub async fn middleware(
    sr: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let req = sr.request();
    let mut sub = match guard::verify_api_key(&req).await {
        Some(s) => Some(s),
        None => None,
    };

    if sub.is_none() {
        sub = match guard::verify_jwt(&req).await {
            Some(s) => Some(s),
            None => None,
        };
    }

    let request_id = Ulid::new().to_string();
    let ip_address = match req.get_client_ip() {
        Some(ip) => ip.to_string(),
        None => "unknown".to_string(),
    };
    let user_agent = req
        .get_user_agent()
        .unwrap_or_else(|| "unknown".to_string());

    let ctx = match sub.clone() {
        Some(s) => {
            let actor_type = match s.sub_type {
                SubjectType::User => ActorType::User,
                SubjectType::ApiKey => ActorType::ApiKey,
            };
            AuditContext::new(actor_type, Some(s.id))
                .with_request_context(request_id, ip_address, user_agent)
        }
        None => AuditContext::anonymous().with_request_context(request_id, ip_address, user_agent),
    };

    if let Some(s) = sub {
        req.extensions_mut().insert(s);
    }
    req.extensions_mut().insert(ctx);

    next.call(sr).await
}
