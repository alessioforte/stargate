use crate::etc::{internal_context::InternalContextRuntime, reqctx::PropagationDraft};
use ::http::{HeaderMap, Method, StatusCode, Version};
use gate::graph::{HeaderValueNode, InternalContextNode, ResponseBodyNode};
use hyper::body::Bytes;
use std::sync::Arc;

pub(super) use gate::DynLoadBalancer;

#[derive(Debug, Default, Clone)]
pub(super) struct ResponseHeaderMutations {
    pub(super) add: Vec<HeaderValueNode>,
    pub(super) set: Vec<HeaderValueNode>,
    pub(super) remove: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct RequestState {
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

#[derive(Debug, Default, Clone)]
pub(super) struct ExecutionPlan {
    pub(super) attempts: Vec<SelectedService>,
    pub(super) failover_on_status: Vec<u16>,
    pub(super) mirrors: Vec<ExecutionPlan>,
}

impl ExecutionPlan {
    pub(super) fn requires_replay(&self) -> bool {
        !self.mirrors.is_empty() || self.attempts.len() > 1
    }

    pub(super) fn needs_failover_replay(&self) -> bool {
        self.attempts.len() > 1
    }

    pub(super) fn should_failover_response(&self, status: StatusCode) -> bool {
        self.failover_on_status.contains(&status.as_u16())
    }
}

#[derive(Debug)]
pub(super) enum ServiceSelectionError {
    NoHealthyUpstream,
    Internal(String),
}
