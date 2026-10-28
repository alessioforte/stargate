use crate::{conf::hosts::IamHosts, web::request::RequestExt};
use axum::{Router, body::Body, extract::Request, response::Response};
use std::{
    convert::Infallible,
    task::{Context, Poll},
};
use tower::Service;

/// Selects the IAM router before path matching; both routers are always ready.
#[derive(Clone)]
pub(super) struct HostDispatch {
    hosts: IamHosts,
    iam: Router,
    gateway: Router,
}

impl HostDispatch {
    pub(super) fn new(hosts: IamHosts, iam: Router) -> Self {
        Self {
            hosts,
            iam,
            gateway: Router::new().fallback_service(tower::service_fn(crate::gate::service)),
        }
    }
}

impl Service<Request<Body>> for HostDispatch {
    type Response = Response;
    type Error = Infallible;
    type Future = <Router as Service<Request<Body>>>::Future;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        if self.hosts.allows(request.get_host().as_deref()) {
            self.iam.call(request)
        } else {
            self.gateway.call(request)
        }
    }
}

#[cfg(all(test, feature = "memory"))]
mod tests;
