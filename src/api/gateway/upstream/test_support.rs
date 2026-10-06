use http::HeaderMap;
use std::collections::HashMap;
use {
    crate::api::gateway::{execution::replay::ReplayRequest, request::RequestState},
    gate::DynLoadBalancer,
};

fn ctx() -> lb::RequestContext<'static> {
    lb::RequestContext {
        client_ip: "127.0.0.1",
        path: "/",
        method: "GET",
        key: None,
    }
}

pub(super) fn available(balancers: &HashMap<String, DynLoadBalancer>, service: &str) -> bool {
    balancers.get(service).unwrap().select(&ctx()).is_some()
}

pub(super) fn replay_state() -> RequestState {
    RequestState {
        execution: crate::api::gateway::lifecycle::Execution::default(),
        client_ip: "127.0.0.1".into(),
        load_balancer_key: None,
        original_path: "/orders".to_owned(),
        path: "/orders".to_owned(),
        query: String::new(),
        preserve_host: false,
        response_headers: Default::default(),
        propagation_draft: None,
        internal_context_runtime: None,
    }
}

pub(super) fn replay_request() -> ReplayRequest {
    ReplayRequest {
        method: ::http::Method::GET,
        version: ::http::Version::HTTP_11,
        headers: HeaderMap::new(),
        body: hyper::body::Bytes::new(),
    }
}
