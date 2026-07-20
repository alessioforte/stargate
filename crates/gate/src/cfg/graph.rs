use crate::cfg::{
    Limit, LoadBalancer, MtlsConfig,
    {AuthStrategy, EnvProfile, LimitScope, OnMissingOrg, UpstreamProtocol},
};
use indexmap::IndexMap;
use regex::Regex;
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub struct CompiledConfig {
    pub schema: String,
    pub limits: Vec<Limit>,
    pub mtls: Option<MtlsConfig>,
    pub http: HttpGraph,
}

#[derive(Debug, Clone, Default)]
pub struct HttpGraph {
    pub upstreams: IndexMap<String, UpstreamNode>,
    pub services: IndexMap<String, ServiceNode>,
    pub middlewares: IndexMap<String, MiddlewareNode>,
    pub policies: IndexMap<String, PolicyNode>,
    pub routers: Vec<RouterNode>,
}

#[derive(Debug, Clone)]
pub struct UpstreamNode {
    pub name: String,
    pub targets: Vec<UpstreamTargetNode>,
    pub load_balancer: LoadBalancer,
    pub transport: Option<TransportNode>,
    pub internal_context: Option<InternalContextNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalContextNode {
    pub audience: String,
}

#[derive(Debug, Clone)]
pub struct UpstreamTargetNode {
    pub url: String,
    pub weight: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct TransportNode {
    pub connect_timeout: Option<String>,
    pub protocols: Vec<UpstreamProtocol>,
}

#[derive(Debug, Clone)]
pub enum ServiceNode {
    LoadBalancer {
        name: String,
        upstream: String,
    },
    Weighted {
        name: String,
        services: Vec<WeightedServiceNode>,
    },
    Mirror {
        name: String,
        service: String,
        mirrors: Vec<MirrorServiceNode>,
    },
    Failover {
        name: String,
        service: String,
        failovers: Vec<String>,
        on_status: Vec<u16>,
    },
    DirectResponse {
        name: String,
        status: u16,
        headers: Vec<HeaderValueNode>,
        body: Option<ResponseBodyNode>,
    },
}

#[derive(Debug, Clone)]
pub struct WeightedServiceNode {
    pub service: String,
    pub weight: u16,
}

#[derive(Debug, Clone)]
pub struct MirrorServiceNode {
    pub service: String,
    pub percent: u8,
}

#[derive(Debug, Clone)]
pub struct HeaderValueNode {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum ResponseBodyNode {
    Text(String),
    Json(serde_yaml_bw::Value),
}

#[derive(Debug, Clone)]
pub enum MiddlewareNode {
    StripPrefix {
        prefixes: Vec<String>,
    },
    AddPrefix {
        prefix: String,
    },
    ReplacePathRegex {
        pattern: Regex,
        replacement: String,
    },
    PreserveHost,
    RequestHeaders {
        add: Vec<HeaderValueNode>,
        set: Vec<HeaderValueNode>,
        remove: Vec<String>,
    },
    ResponseHeaders {
        add: Vec<HeaderValueNode>,
        set: Vec<HeaderValueNode>,
        remove: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum PolicyNode {
    Auth {
        strategies: Vec<AuthStrategy>,
    },
    AccessControl {
        resource: String,
        env: Option<EnvProfile>,
    },
    RateLimit {
        limit: String,
        scope: LimitScope,
        on_missing: OnMissingOrg,
    },
    Quota {
        limit: String,
        scope: LimitScope,
        on_missing: OnMissingOrg,
    },
}

#[derive(Debug, Clone)]
pub struct RouterNode {
    pub name: String,
    pub priority: i32,
    pub order: usize,
    pub matcher: MatchExprNode,
    pub service: String,
    pub middlewares: Vec<String>,
    pub policies: Vec<String>,
    /// Quota units one request on this route consumes, charged to every
    /// quota bucket that applies (policy-selected and subject `attrs.quota`).
    pub quota_cost: u64,
}

#[derive(Debug, Clone)]
pub enum MatchExprNode {
    All(Vec<MatchExprNode>),
    Any(Vec<MatchExprNode>),
    Not(Box<MatchExprNode>),
    Host(ValuePredicate),
    Method(Vec<String>),
    Path(PathPredicate),
    Header(NamedValuePredicate),
    Query(NamedValuePredicate),
    Cookie(NamedValuePredicate),
    SourceIp(SourceIpPredicate),
}

#[derive(Debug, Clone)]
pub enum ValuePredicate {
    Eq(String),
    Prefix(String),
    Suffix(String),
    Contains(String),
    Regex(Regex),
    Present(bool),
    OneOf(Vec<String>),
}

#[derive(Debug, Clone)]
pub enum PathPredicate {
    Exact(String),
    Prefix(String),
    Template(String),
    Regex(Regex),
}

#[derive(Debug, Clone)]
pub struct NamedValuePredicate {
    pub name: String,
    pub predicate: ValuePredicate,
}

#[derive(Debug, Clone)]
pub struct SourceIpPredicate {
    pub cidrs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub path: String,
    pub message: String,
}

impl CompileError {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl Display for CompileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl Error for CompileError {}
