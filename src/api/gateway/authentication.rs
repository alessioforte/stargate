use crate::etc::auth::{identity::VerifiedIdentity, verification};
use axum::body::Body;
use http::Request;

pub(super) async fn authenticate(auth_req: Request<()>) -> VerifiedIdentity {
    #[cfg(test)]
    if let Some(authenticator) = auth_req.extensions().get::<TestAuthenticator>() {
        return (authenticator.verify)(&auth_req);
    }
    match verification::verify_api_key(&auth_req).await {
        Some(identity) => identity,
        None => verification::verify_bearer(&auth_req)
            .await
            .unwrap_or_else(VerifiedIdentity::anonymous),
    }
}

pub(super) fn auth_request(req: &Request<Body>) -> Request<()> {
    let mut auth_req = Request::new(());
    *auth_req.method_mut() = req.method().clone();
    *auth_req.uri_mut() = req.uri().clone();
    *auth_req.headers_mut() = req.headers().clone();
    #[cfg(test)]
    if let Some(authenticator) = req.extensions().get::<TestAuthenticator>() {
        auth_req.extensions_mut().insert(authenticator.clone());
    }
    auth_req
}

#[cfg(test)]
type IdentityVerifier = dyn Fn(&Request<()>) -> VerifiedIdentity + Send + Sync;

#[cfg(test)]
#[derive(Clone)]
pub(super) struct TestAuthenticator {
    pub verify: std::sync::Arc<IdentityVerifier>,
}
