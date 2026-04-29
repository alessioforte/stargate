use super::graph::{
    CompileError, CompiledConfig, HeaderValueNode, HttpGraph, MatchExprNode, MiddlewareNode,
    MirrorServiceNode, NamedValuePredicate, PathPredicate, PolicyNode, ResponseBodyNode,
    RouterNode, ServiceNode, SourceIpPredicate, TransportNode, UpstreamNode, UpstreamTargetNode,
    ValuePredicate, WeightedServiceNode,
};
use super::{Limit, LoadBalancer, MtlsConfig};
use indexmap::IndexMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

const SCHEMA: &str = "stargate/v2alpha1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_schema")]
    pub schema: String,
    #[serde(default)]
    pub limits: Vec<Limit>,
    #[serde(default)]
    pub mtls: Option<MtlsConfig>,
    #[serde(default)]
    pub http: HttpConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema: default_schema(),
            limits: Vec::new(),
            mtls: None,
            http: HttpConfig::default(),
        }
    }
}

impl Config {
    pub fn from_file(path: &str) -> Self {
        if !Path::new(path).exists() {
            tracing::info!("Creating v2alpha1 gate configuration yaml file");
            let config = Config::default();
            let config_str = serde_yaml_bw::to_string(&config).expect("Unable to serialize config");
            std::fs::write(path, config_str).expect("Unable to write config file");
            return config;
        }

        let file = std::fs::read_to_string(path).expect("Unable to read config file");
        serde_yaml_bw::from_str(&file).expect("Unable to parse config file")
    }

    pub fn to_file(&self, path: &str) {
        let config_str = serde_yaml_bw::to_string(self).expect("Unable to serialize config");
        std::fs::write(path, config_str).expect("Unable to write config file");
    }

    pub fn compile(&self) -> Result<CompiledConfig, CompileError> {
        if self.schema != SCHEMA {
            return Err(CompileError::new("schema", format!("expected {}", SCHEMA)));
        }

        let limit_names = collect_limit_names(&self.limits)?;
        let upstreams = compile_upstreams(&self.http.upstreams)?;
        let middlewares = compile_middlewares(&self.http.middlewares)?;
        let policies = compile_policies(&self.http.policies, &limit_names)?;

        validate_service_refs(&self.http.services, &upstreams)?;
        let services = compile_services(&self.http.services)?;
        let routers = compile_routers(
            &self.http.routers,
            &self.http.services,
            &self.http.middlewares,
            &self.http.policies,
        )?;

        Ok(CompiledConfig {
            schema: self.schema.clone(),
            limits: self.limits.clone(),
            mtls: self.mtls.clone(),
            http: HttpGraph {
                upstreams,
                services,
                middlewares,
                policies,
                routers,
            },
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, utoipa::ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnvProfile {
    None,
    Basic,
    Geo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpConfig {
    #[serde(default)]
    pub upstreams: IndexMap<String, Upstream>,
    #[serde(default)]
    pub services: IndexMap<String, Service>,
    #[serde(default)]
    pub middlewares: IndexMap<String, Middleware>,
    #[serde(default)]
    pub policies: IndexMap<String, Policy>,
    #[serde(default)]
    pub routers: IndexMap<String, Router>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upstream {
    pub targets: Vec<UpstreamTarget>,
    #[serde(default)]
    pub load_balancer: LoadBalancer,
    #[serde(default)]
    pub transport: Option<Transport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamTarget {
    pub url: String,
    #[serde(default)]
    pub weight: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transport {
    #[serde(default)]
    pub connect_timeout: Option<String>,
    #[serde(default)]
    pub protocols: Vec<UpstreamProtocol>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamProtocol {
    Http1,
    Http2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Service {
    LoadBalancer {
        upstream: String,
    },
    Weighted {
        services: Vec<WeightedService>,
    },
    Mirror {
        service: String,
        mirrors: Vec<MirrorService>,
    },
    Failover {
        service: String,
        failovers: Vec<String>,
        #[serde(default)]
        on_status: Vec<u16>,
    },
    DirectResponse {
        status: u16,
        #[serde(default)]
        headers: Vec<HeaderValue>,
        #[serde(default)]
        body: Option<ResponseBody>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedService {
    pub name: String,
    pub weight: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorService {
    pub service: String,
    pub percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderValue {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseBody {
    Text { text: String },
    Json { json: serde_yaml_bw::Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Middleware {
    StripPrefix {
        prefixes: Vec<String>,
    },
    AddPrefix {
        prefix: String,
    },
    ReplacePathRegex {
        pattern: String,
        replacement: String,
    },
    PreserveHost,
    RequestHeaders {
        #[serde(default)]
        add: Vec<HeaderValue>,
        #[serde(default)]
        set: Vec<HeaderValue>,
        #[serde(default)]
        remove: Vec<String>,
    },
    ResponseHeaders {
        #[serde(default)]
        add: Vec<HeaderValue>,
        #[serde(default)]
        set: Vec<HeaderValue>,
        #[serde(default)]
        remove: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Policy {
    Auth {
        strategies: Vec<AuthStrategy>,
    },
    AccessControl {
        resource: String,
        #[serde(default)]
        env: Option<EnvProfile>,
    },
    RateLimit {
        limit: String,
    },
    Quota {
        limit: String,
        cost: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthStrategy {
    Jwt,
    ApiKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Router {
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(rename = "match")]
    pub matcher: MatchExpr,
    pub service: String,
    #[serde(default)]
    pub middlewares: Vec<String>,
    #[serde(default)]
    pub policies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchExpr {
    All(Vec<MatchExpr>),
    Any(Vec<MatchExpr>),
    Not(Box<MatchExpr>),
    Host(ValueMatch),
    Method(Vec<String>),
    Path(PathMatch),
    Header(NamedValueMatch),
    Query(NamedValueMatch),
    Cookie(NamedValueMatch),
    SourceIp(SourceIpMatch),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValueMatch {
    #[serde(default)]
    pub eq: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub contains: Option<String>,
    #[serde(default)]
    pub regex: Option<String>,
    #[serde(default)]
    pub present: Option<bool>,
    #[serde(default)]
    pub one_of: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathMatch {
    #[serde(default)]
    pub exact: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub regex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedValueMatch {
    pub name: String,
    #[serde(flatten)]
    pub value: ValueMatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceIpMatch {
    pub cidrs: Vec<String>,
}

fn default_schema() -> String {
    SCHEMA.to_string()
}

fn collect_limit_names(limits: &[Limit]) -> Result<HashSet<String>, CompileError> {
    let mut names = HashSet::new();
    for (idx, limit) in limits.iter().enumerate() {
        let path = format!("limits[{}].name", idx);
        if limit.name.trim().is_empty() {
            return Err(CompileError::new(path, "limit name cannot be empty"));
        }
        if !names.insert(limit.name.clone()) {
            return Err(CompileError::new(path, "duplicate limit name"));
        }
    }
    Ok(names)
}

fn compile_upstreams(
    upstreams: &IndexMap<String, Upstream>,
) -> Result<IndexMap<String, UpstreamNode>, CompileError> {
    let mut out = IndexMap::new();

    for (name, upstream) in upstreams {
        if upstream.targets.is_empty() {
            return Err(CompileError::new(
                format!("http.upstreams.{}.targets", name),
                "at least one target is required",
            ));
        }

        let mut targets = Vec::with_capacity(upstream.targets.len());
        for (idx, target) in upstream.targets.iter().enumerate() {
            let path = format!("http.upstreams.{}.targets[{}]", name, idx);
            if target.url.trim().is_empty() || !target.url.contains("://") {
                return Err(CompileError::new(
                    format!("{}.url", path),
                    "target url must include a scheme",
                ));
            }
            if let Some(weight) = target.weight {
                if weight == 0 {
                    return Err(CompileError::new(
                        format!("{}.weight", path),
                        "target weight must be greater than zero",
                    ));
                }
            }
            targets.push(UpstreamTargetNode {
                url: target.url.clone(),
                weight: target.weight,
            });
        }

        if let Some(transport) = &upstream.transport {
            let mut seen = HashSet::new();
            for (idx, protocol) in transport.protocols.iter().enumerate() {
                if !seen.insert(*protocol as u8) {
                    return Err(CompileError::new(
                        format!("http.upstreams.{}.transport.protocols[{}]", name, idx),
                        "duplicate protocol",
                    ));
                }
            }
        }

        out.insert(
            name.clone(),
            UpstreamNode {
                name: name.clone(),
                targets,
                load_balancer: upstream.load_balancer.clone(),
                transport: upstream.transport.as_ref().map(|transport| TransportNode {
                    connect_timeout: transport.connect_timeout.clone(),
                    protocols: transport.protocols.clone(),
                }),
            },
        );
    }

    Ok(out)
}

fn compile_middlewares(
    middlewares: &IndexMap<String, Middleware>,
) -> Result<IndexMap<String, MiddlewareNode>, CompileError> {
    let mut out = IndexMap::new();

    for (name, middleware) in middlewares {
        let node = match middleware {
            Middleware::StripPrefix { prefixes } => {
                if prefixes.is_empty() {
                    return Err(CompileError::new(
                        format!("http.middlewares.{}.prefixes", name),
                        "at least one prefix is required",
                    ));
                }
                MiddlewareNode::StripPrefix {
                    prefixes: prefixes.clone(),
                }
            }
            Middleware::AddPrefix { prefix } => {
                if prefix.is_empty() {
                    return Err(CompileError::new(
                        format!("http.middlewares.{}.prefix", name),
                        "prefix cannot be empty",
                    ));
                }
                MiddlewareNode::AddPrefix {
                    prefix: prefix.clone(),
                }
            }
            Middleware::ReplacePathRegex {
                pattern,
                replacement,
            } => MiddlewareNode::ReplacePathRegex {
                pattern: compile_regex(format!("http.middlewares.{}.pattern", name), pattern)?,
                replacement: replacement.clone(),
            },
            Middleware::PreserveHost => MiddlewareNode::PreserveHost,
            Middleware::RequestHeaders { add, set, remove } => MiddlewareNode::RequestHeaders {
                add: compile_headers(format!("http.middlewares.{}.add", name), add)?,
                set: compile_headers(format!("http.middlewares.{}.set", name), set)?,
                remove: remove
                    .iter()
                    .map(|value| value.to_ascii_lowercase())
                    .collect(),
            },
            Middleware::ResponseHeaders { add, set, remove } => MiddlewareNode::ResponseHeaders {
                add: compile_headers(format!("http.middlewares.{}.add", name), add)?,
                set: compile_headers(format!("http.middlewares.{}.set", name), set)?,
                remove: remove
                    .iter()
                    .map(|value| value.to_ascii_lowercase())
                    .collect(),
            },
        };

        out.insert(name.clone(), node);
    }

    Ok(out)
}

fn compile_policies(
    policies: &IndexMap<String, Policy>,
    limit_names: &HashSet<String>,
) -> Result<IndexMap<String, PolicyNode>, CompileError> {
    let mut out = IndexMap::new();

    for (name, policy) in policies {
        let node = match policy {
            Policy::Auth { strategies } => {
                if strategies.is_empty() {
                    return Err(CompileError::new(
                        format!("http.policies.{}.strategies", name),
                        "at least one auth strategy is required",
                    ));
                }
                PolicyNode::Auth {
                    strategies: strategies.clone(),
                }
            }
            Policy::AccessControl { resource, env } => {
                if resource.trim().is_empty() {
                    return Err(CompileError::new(
                        format!("http.policies.{}.resource", name),
                        "resource cannot be empty",
                    ));
                }
                PolicyNode::AccessControl {
                    resource: resource.clone(),
                    env: *env,
                }
            }
            Policy::RateLimit { limit } => {
                ensure_limit_exists(name, limit, limit_names, "rate limit")?;
                PolicyNode::RateLimit {
                    limit: limit.clone(),
                }
            }
            Policy::Quota { limit, cost } => {
                ensure_limit_exists(name, limit, limit_names, "quota")?;
                if *cost == 0 {
                    return Err(CompileError::new(
                        format!("http.policies.{}.cost", name),
                        "quota cost must be greater than zero",
                    ));
                }
                PolicyNode::Quota {
                    limit: limit.clone(),
                    cost: *cost,
                }
            }
        };

        out.insert(name.clone(), node);
    }

    Ok(out)
}

fn ensure_limit_exists(
    policy_name: &str,
    limit: &str,
    limit_names: &HashSet<String>,
    kind: &str,
) -> Result<(), CompileError> {
    if !limit_names.contains(limit) {
        return Err(CompileError::new(
            format!("http.policies.{}.limit", policy_name),
            format!("unknown {} '{}'", kind, limit),
        ));
    }
    Ok(())
}

fn validate_service_refs(
    services: &IndexMap<String, Service>,
    upstreams: &IndexMap<String, UpstreamNode>,
) -> Result<(), CompileError> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum VisitState {
        Visiting,
        Visited,
    }

    fn visit(
        name: &str,
        services: &IndexMap<String, Service>,
        upstreams: &IndexMap<String, UpstreamNode>,
        states: &mut HashMap<String, VisitState>,
        stack: &mut Vec<String>,
    ) -> Result<(), CompileError> {
        match states.get(name).copied() {
            Some(VisitState::Visited) => return Ok(()),
            Some(VisitState::Visiting) => {
                stack.push(name.to_string());
                return Err(CompileError::new(
                    format!("http.services.{}", name),
                    format!("service cycle detected: {}", stack.join(" -> ")),
                ));
            }
            None => {}
        }

        let service = services.get(name).ok_or_else(|| {
            CompileError::new(
                format!("http.services.{}", name),
                "referenced service does not exist",
            )
        })?;

        states.insert(name.to_string(), VisitState::Visiting);
        stack.push(name.to_string());

        match service {
            Service::LoadBalancer { upstream } => {
                if !upstreams.contains_key(upstream) {
                    return Err(CompileError::new(
                        format!("http.services.{}.upstream", name),
                        format!("unknown upstream '{}'", upstream),
                    ));
                }
            }
            Service::Weighted { services: refs } => {
                if refs.is_empty() {
                    return Err(CompileError::new(
                        format!("http.services.{}.services", name),
                        "weighted service requires at least one child service",
                    ));
                }
                for (idx, child) in refs.iter().enumerate() {
                    if child.weight == 0 {
                        return Err(CompileError::new(
                            format!("http.services.{}.services[{}].weight", name, idx),
                            "weight must be greater than zero",
                        ));
                    }
                    visit(&child.name, services, upstreams, states, stack)?;
                }
            }
            Service::Mirror {
                service: primary,
                mirrors,
            } => {
                if mirrors.is_empty() {
                    return Err(CompileError::new(
                        format!("http.services.{}.mirrors", name),
                        "mirror service requires at least one mirror",
                    ));
                }
                visit(primary, services, upstreams, states, stack)?;
                for (idx, mirror) in mirrors.iter().enumerate() {
                    if mirror.percent == 0 || mirror.percent > 100 {
                        return Err(CompileError::new(
                            format!("http.services.{}.mirrors[{}].percent", name, idx),
                            "mirror percent must be between 1 and 100",
                        ));
                    }
                    visit(&mirror.service, services, upstreams, states, stack)?;
                }
            }
            Service::Failover {
                service: primary,
                failovers,
                on_status,
            } => {
                if failovers.is_empty() {
                    return Err(CompileError::new(
                        format!("http.services.{}.failovers", name),
                        "failover service requires at least one fallback",
                    ));
                }
                if on_status
                    .iter()
                    .any(|status| *status < 100 || *status > 599)
                {
                    return Err(CompileError::new(
                        format!("http.services.{}.on_status", name),
                        "statuses must be valid HTTP status codes",
                    ));
                }
                visit(primary, services, upstreams, states, stack)?;
                for child in failovers {
                    visit(child, services, upstreams, states, stack)?;
                }
            }
            Service::DirectResponse { status, .. } => {
                if *status < 100 || *status > 599 {
                    return Err(CompileError::new(
                        format!("http.services.{}.status", name),
                        "status must be a valid HTTP status code",
                    ));
                }
            }
        }

        stack.pop();
        states.insert(name.to_string(), VisitState::Visited);
        Ok(())
    }

    let mut states = HashMap::new();
    let mut stack = Vec::new();

    for name in services.keys() {
        visit(name, services, upstreams, &mut states, &mut stack)?;
    }

    Ok(())
}

fn compile_services(
    services: &IndexMap<String, Service>,
) -> Result<IndexMap<String, ServiceNode>, CompileError> {
    let mut out = IndexMap::new();

    for (name, service) in services {
        let node = match service {
            Service::LoadBalancer { upstream } => ServiceNode::LoadBalancer {
                name: name.clone(),
                upstream: upstream.clone(),
            },
            Service::Weighted { services } => ServiceNode::Weighted {
                name: name.clone(),
                services: services
                    .iter()
                    .map(|service| WeightedServiceNode {
                        service: service.name.clone(),
                        weight: service.weight,
                    })
                    .collect(),
            },
            Service::Mirror { service, mirrors } => ServiceNode::Mirror {
                name: name.clone(),
                service: service.clone(),
                mirrors: mirrors
                    .iter()
                    .map(|mirror| MirrorServiceNode {
                        service: mirror.service.clone(),
                        percent: mirror.percent,
                    })
                    .collect(),
            },
            Service::Failover {
                service,
                failovers,
                on_status,
            } => ServiceNode::Failover {
                name: name.clone(),
                service: service.clone(),
                failovers: failovers.clone(),
                on_status: on_status.clone(),
            },
            Service::DirectResponse {
                status,
                headers,
                body,
            } => ServiceNode::DirectResponse {
                name: name.clone(),
                status: *status,
                headers: compile_headers(format!("http.services.{}.headers", name), headers)?,
                body: body.as_ref().map(compile_response_body),
            },
        };

        out.insert(name.clone(), node);
    }

    Ok(out)
}

fn compile_routers(
    routers: &IndexMap<String, Router>,
    services: &IndexMap<String, Service>,
    middlewares: &IndexMap<String, Middleware>,
    policies: &IndexMap<String, Policy>,
) -> Result<Vec<RouterNode>, CompileError> {
    let mut out = Vec::with_capacity(routers.len());

    for (order, (name, router)) in routers.iter().enumerate() {
        if !services.contains_key(&router.service) {
            return Err(CompileError::new(
                format!("http.routers.{}.service", name),
                format!("unknown service '{}'", router.service),
            ));
        }

        for (idx, middleware) in router.middlewares.iter().enumerate() {
            if !middlewares.contains_key(middleware) {
                return Err(CompileError::new(
                    format!("http.routers.{}.middlewares[{}]", name, idx),
                    format!("unknown middleware '{}'", middleware),
                ));
            }
        }

        for (idx, policy) in router.policies.iter().enumerate() {
            if !policies.contains_key(policy) {
                return Err(CompileError::new(
                    format!("http.routers.{}.policies[{}]", name, idx),
                    format!("unknown policy '{}'", policy),
                ));
            }
        }

        out.push(RouterNode {
            name: name.clone(),
            priority: router.priority.unwrap_or(0),
            order,
            matcher: compile_match_expr(&router.matcher, format!("http.routers.{}.match", name))?,
            service: router.service.clone(),
            middlewares: router.middlewares.clone(),
            policies: router.policies.clone(),
        });
    }

    out.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.order.cmp(&b.order))
    });

    Ok(out)
}

fn compile_match_expr(expr: &MatchExpr, path: String) -> Result<MatchExprNode, CompileError> {
    match expr {
        MatchExpr::All(values) => {
            if values.is_empty() {
                return Err(CompileError::new(
                    path,
                    "all requires at least one child matcher",
                ));
            }
            Ok(MatchExprNode::All(
                values
                    .iter()
                    .enumerate()
                    .map(|(idx, value)| compile_match_expr(value, format!("{}.all[{}]", path, idx)))
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        MatchExpr::Any(values) => {
            if values.is_empty() {
                return Err(CompileError::new(
                    path,
                    "any requires at least one child matcher",
                ));
            }
            Ok(MatchExprNode::Any(
                values
                    .iter()
                    .enumerate()
                    .map(|(idx, value)| compile_match_expr(value, format!("{}.any[{}]", path, idx)))
                    .collect::<Result<Vec<_>, _>>()?,
            ))
        }
        MatchExpr::Not(value) => Ok(MatchExprNode::Not(Box::new(compile_match_expr(
            value,
            format!("{}.not", path),
        )?))),
        MatchExpr::Host(value) => Ok(MatchExprNode::Host(compile_value_match(
            value,
            format!("{}.host", path),
        )?)),
        MatchExpr::Method(methods) => {
            if methods.is_empty() {
                return Err(CompileError::new(
                    path,
                    "method requires at least one value",
                ));
            }
            Ok(MatchExprNode::Method(
                methods
                    .iter()
                    .map(|value| value.to_ascii_uppercase())
                    .collect(),
            ))
        }
        MatchExpr::Path(value) => Ok(MatchExprNode::Path(compile_path_match(
            value,
            format!("{}.path", path),
        )?)),
        MatchExpr::Header(value) => Ok(MatchExprNode::Header(compile_named_value_match(
            value,
            format!("{}.header", path),
        )?)),
        MatchExpr::Query(value) => Ok(MatchExprNode::Query(compile_named_value_match(
            value,
            format!("{}.query", path),
        )?)),
        MatchExpr::Cookie(value) => Ok(MatchExprNode::Cookie(compile_named_value_match(
            value,
            format!("{}.cookie", path),
        )?)),
        MatchExpr::SourceIp(value) => {
            if value.cidrs.is_empty() {
                return Err(CompileError::new(
                    path,
                    "source_ip requires at least one cidr",
                ));
            }
            Ok(MatchExprNode::SourceIp(SourceIpPredicate {
                cidrs: value.cidrs.clone(),
            }))
        }
    }
}

fn compile_named_value_match(
    value: &NamedValueMatch,
    path: String,
) -> Result<NamedValuePredicate, CompileError> {
    if value.name.trim().is_empty() {
        return Err(CompileError::new(
            format!("{}.name", path),
            "name cannot be empty",
        ));
    }

    Ok(NamedValuePredicate {
        name: value.name.to_ascii_lowercase(),
        predicate: compile_value_match(&value.value, path)?,
    })
}

fn compile_value_match(value: &ValueMatch, path: String) -> Result<ValuePredicate, CompileError> {
    let mut matches = Vec::new();

    if let Some(v) = &value.eq {
        matches.push(("eq", ValuePredicate::Eq(v.clone())));
    }
    if let Some(v) = &value.prefix {
        matches.push(("prefix", ValuePredicate::Prefix(v.clone())));
    }
    if let Some(v) = &value.suffix {
        matches.push(("suffix", ValuePredicate::Suffix(v.clone())));
    }
    if let Some(v) = &value.contains {
        matches.push(("contains", ValuePredicate::Contains(v.clone())));
    }
    if let Some(v) = &value.regex {
        matches.push((
            "regex",
            ValuePredicate::Regex(compile_regex(format!("{}.regex", path), v)?),
        ));
    }
    if let Some(v) = value.present {
        matches.push(("present", ValuePredicate::Present(v)));
    }
    if let Some(v) = &value.one_of {
        if v.is_empty() {
            return Err(CompileError::new(
                format!("{}.one_of", path),
                "one_of requires at least one value",
            ));
        }
        matches.push(("one_of", ValuePredicate::OneOf(v.clone())));
    }

    if matches.len() != 1 {
        return Err(CompileError::new(
            path,
            "exactly one matcher operator must be set",
        ));
    }

    Ok(matches.pop().unwrap().1)
}

fn compile_path_match(value: &PathMatch, path: String) -> Result<PathPredicate, CompileError> {
    let mut matches = Vec::new();

    if let Some(v) = &value.exact {
        matches.push(PathPredicate::Exact(v.clone()));
    }
    if let Some(v) = &value.prefix {
        matches.push(PathPredicate::Prefix(v.clone()));
    }
    if let Some(v) = &value.template {
        matches.push(PathPredicate::Template(v.clone()));
    }
    if let Some(v) = &value.regex {
        matches.push(PathPredicate::Regex(compile_regex(
            format!("{}.regex", path),
            v,
        )?));
    }

    if matches.len() != 1 {
        return Err(CompileError::new(
            path,
            "exactly one path matcher must be set",
        ));
    }

    Ok(matches.pop().unwrap())
}

fn compile_headers(
    path: String,
    headers: &[HeaderValue],
) -> Result<Vec<HeaderValueNode>, CompileError> {
    headers
        .iter()
        .enumerate()
        .map(|(idx, header)| {
            if header.name.trim().is_empty() {
                return Err(CompileError::new(
                    format!("{}[{}].name", path, idx),
                    "header name cannot be empty",
                ));
            }
            Ok(HeaderValueNode {
                name: header.name.to_ascii_lowercase(),
                value: header.value.clone(),
            })
        })
        .collect()
}

fn compile_response_body(body: &ResponseBody) -> ResponseBodyNode {
    match body {
        ResponseBody::Text { text } => ResponseBodyNode::Text(text.clone()),
        ResponseBody::Json { json } => ResponseBodyNode::Json(json.clone()),
    }
}

fn compile_regex(path: String, pattern: &str) -> Result<Regex, CompileError> {
    Regex::new(pattern)
        .map_err(|error| CompileError::new(path, format!("invalid regex: {}", error)))
}

#[cfg(test)]
mod tests {
    use super::Config;
    use crate::graph::{MatchExprNode, MiddlewareNode, PathPredicate, ServiceNode, ValuePredicate};

    fn parse_config(yaml: &str) -> Config {
        serde_yaml_bw::from_str(yaml).expect("config should parse")
    }

    #[test]
    fn compiles_named_entities_and_sorts_routers() {
        let config = parse_config(
            r#"
schema: stargate/v2alpha1
limits:
  - name: default
    strategy: gcra
    params:
      max_burst: 3
      replenish_1_per: 500ms
  - name: daily
    strategy: quota_tracker
    params:
      limit: 1000
      period: day
http:
  upstreams:
    reports-v1:
      targets:
        - url: http://localhost:4042
      load_balancer:
        strategy: round_robin
    reports-v2:
      targets:
        - url: http://localhost:4043
  services:
    reports-v1:
      kind: load_balancer
      upstream: reports-v1
    reports-v2:
      kind: load_balancer
      upstream: reports-v2
    reports-canary:
      kind: weighted
      services:
        - name: reports-v1
          weight: 95
        - name: reports-v2
          weight: 5
    not-found:
      kind: direct_response
      status: 404
      body:
        json:
          code: ROUTE_NOT_FOUND
  middlewares:
    strip-api:
      kind: strip_prefix
      prefixes: [/api]
    legacy-rewrite:
      kind: replace_path_regex
      pattern: ^/api/legacy/(.*)$
      replacement: /v2/$1
  policies:
    auth-default:
      kind: auth
      strategies: [jwt, api_key]
    reports-read:
      kind: access_control
      resource: financial_reports:read
    rl-default:
      kind: rate_limit
      limit: default
    quota-default:
      kind: quota
      limit: daily
      cost: 10
  routers:
    fallback:
      priority: -1000
      match:
        path:
          prefix: /
      service: not-found
    reports-v2-by-header:
      priority: 1000
      match:
        all:
          - host:
              eq: api.example.com
          - path:
              prefix: /api/v1/reports
          - header:
              name: x-version
              eq: v2
      service: reports-v2
      middlewares: [strip-api]
      policies: [auth-default, reports-read, rl-default]
"#,
        );

        let compiled = config.compile().expect("config should compile");
        assert_eq!(compiled.http.routers.len(), 2);
        assert_eq!(compiled.http.routers[0].name, "reports-v2-by-header");
        assert_eq!(compiled.http.routers[1].name, "fallback");
        assert!(matches!(
            compiled.http.services["reports-canary"],
            ServiceNode::Weighted { .. }
        ));
        assert!(matches!(
            compiled.http.middlewares["legacy-rewrite"],
            MiddlewareNode::ReplacePathRegex { .. }
        ));
        match &compiled.http.routers[0].matcher {
            MatchExprNode::All(conditions) => {
                assert_eq!(conditions.len(), 3);
                assert!(matches!(
                    conditions[1],
                    MatchExprNode::Path(PathPredicate::Prefix(_))
                ));
                assert!(matches!(conditions[2], MatchExprNode::Header(_)));
            }
            other => panic!("unexpected matcher: {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_refs() {
        let config = parse_config(
            r#"
schema: stargate/v2alpha1
limits:
  - name: default
    strategy: gcra
    params:
      max_burst: 3
      replenish_1_per: 500ms
http:
  upstreams:
    reports:
      targets:
        - url: http://localhost:4042
  services:
    reports:
      kind: load_balancer
      upstream: reports
  routers:
    bad:
      match:
        path:
          prefix: /
      service: missing
"#,
        );

        let error = config.compile().expect_err("compile should fail");
        assert_eq!(error.path, "http.routers.bad.service");
    }

    #[test]
    fn rejects_service_cycles() {
        let config = parse_config(
            r#"
schema: stargate/v2alpha1
http:
  services:
    a:
      kind: weighted
      services:
        - name: b
          weight: 1
    b:
      kind: mirror
      service: a
      mirrors:
        - service: a
          percent: 10
"#,
        );

        let error = config.compile().expect_err("compile should fail");
        assert!(error.message.contains("service cycle detected"));
    }

    #[test]
    fn rejects_ambiguous_match_operator() {
        let config = parse_config(
            r#"
schema: stargate/v2alpha1
http:
  upstreams:
    reports:
      targets:
        - url: http://localhost:4042
  services:
    reports:
      kind: load_balancer
      upstream: reports
  routers:
    bad:
      match:
        host:
          eq: api.example.com
          prefix: api.
      service: reports
"#,
        );

        let error = config.compile().expect_err("compile should fail");
        assert_eq!(error.path, "http.routers.bad.match.host");
    }

    #[test]
    fn compiles_regex_matchers() {
        let config = parse_config(
            r#"
schema: stargate/v2alpha1
http:
  upstreams:
    reports:
      targets:
        - url: http://localhost:4042
  services:
    reports:
      kind: load_balancer
      upstream: reports
  routers:
    regex:
      match:
        path:
          regex: ^/api/reports/[0-9]+$
      service: reports
"#,
        );

        let compiled = config.compile().expect("config should compile");
        match &compiled.http.routers[0].matcher {
            MatchExprNode::Path(PathPredicate::Regex(regex)) => {
                assert!(regex.is_match("/api/reports/42"));
            }
            other => panic!("unexpected matcher: {other:?}"),
        }
    }

    #[test]
    fn compiles_value_predicates() {
        let config = parse_config(
            r#"
schema: stargate/v2alpha1
http:
  upstreams:
    reports:
      targets:
        - url: http://localhost:4042
  services:
    reports:
      kind: load_balancer
      upstream: reports
  routers:
    header:
      match:
        header:
          name: x-version
          one_of: [v1, v2]
      service: reports
"#,
        );

        let compiled = config.compile().expect("config should compile");
        match &compiled.http.routers[0].matcher {
            MatchExprNode::Header(value) => {
                assert_eq!(value.name, "x-version");
                assert!(matches!(value.predicate, ValuePredicate::OneOf(_)));
            }
            other => panic!("unexpected matcher: {other:?}"),
        }
    }
}
