use crate::etc::{internal_context::InternalContextRuntime, reqctx::PropagationDraft};
use ::http::{HeaderMap, Method, Version};
use gate::graph::{HeaderValueNode, InternalContextNode, ResponseBodyNode};
use hyper::body::Bytes;
use std::sync::Arc;

pub(super) use gate::DynLoadBalancer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReplayEligibility {
    Idempotent,
    SingleDispatch,
}

impl ReplayEligibility {
    pub(super) fn for_method(method: &Method) -> Self {
        // RFC 9110 sections 9.2.1–9.2.2: safe methods, PUT, and DELETE are
        // idempotent. Headers alone cannot establish an operation contract.
        if matches!(
            *method,
            Method::GET
                | Method::HEAD
                | Method::OPTIONS
                | Method::TRACE
                | Method::PUT
                | Method::DELETE
        ) {
            Self::Idempotent
        } else {
            Self::SingleDispatch
        }
    }
}

#[derive(Debug, Default, Clone)]
pub(super) struct ResponseHeaderMutations {
    pub(super) add: Vec<HeaderValueNode>,
    pub(super) set: Vec<HeaderValueNode>,
    pub(super) remove: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct RequestState {
    pub(super) execution: super::lifecycle::Execution,
    pub(super) client_ip: String,
    pub(super) load_balancer_key: Option<String>,
    pub(super) original_path: String,
    pub(super) path: String,
    pub(super) query: String,
    pub(super) preserve_host: bool,
    pub(super) response_headers: ResponseHeaderMutations,
    pub(super) propagation_draft: Option<Arc<PropagationDraft>>,
    pub(super) internal_context_runtime: Option<Arc<InternalContextRuntime>>,
}

#[derive(Debug, Clone)]
pub(super) struct ReplayRequest {
    pub(super) method: Method,
    pub(super) version: Version,
    pub(super) headers: HeaderMap,
    pub(super) body: Bytes,
}

#[derive(Debug, Clone)]
pub(super) enum SelectedService {
    Upstream {
        service_name: String,
        upstream_base_url: String,
        internal_context: Option<InternalContextNode>,
    },
    DirectResponse {
        status: u16,
        headers: Vec<HeaderValueNode>,
        body: Option<ResponseBodyNode>,
    },
}

#[derive(Debug, Clone)]
pub(super) enum ExecutionPlan {
    Upstream {
        service_name: String,
        internal_context: Option<InternalContextNode>,
    },
    DirectResponse {
        status: u16,
        headers: Vec<HeaderValueNode>,
        body: Option<ResponseBodyNode>,
    },
    Failover {
        services: Vec<ExecutionPlan>,
        on_status: Vec<u16>,
    },
    Mirror {
        service: Box<ExecutionPlan>,
        mirrors: Vec<ExecutionPlan>,
    },
}

#[derive(Debug)]
pub(super) enum ServiceSelectionError {
    NoHealthyUpstream,
    Internal(String),
}
