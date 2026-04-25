use super::{http, retry_after_header_value, ws};
use crate::err::{ErrorResponse, HttpError};
use crate::etc::{ac::access_control, ext::RequestExt, gate::get_client, reqctx, sub::Subject};
use ::http::{
    HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode, Uri, Version,
    header::{CONTENT_LENGTH, CONTENT_TYPE},
};
use axum::{body::Body, response::Response};
use gate::{
    Gate,
    cfg::{service::EnvProfile, v2alpha1::AuthStrategy},
    graph::{
        HeaderValueNode, HttpGraph, MatchExprNode, MiddlewareNode, NamedValuePredicate,
        PathPredicate, PolicyNode, ResponseBodyNode, RouterNode, ServiceNode, SourceIpPredicate,
        ValuePredicate,
    },
};
use http_body_util::BodyExt;
use hyper::body::Bytes;
use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    net::IpAddr,
    sync::Arc,
};

type DynLoadBalancer = Arc<dyn lb::LoadBalancer + Send + Sync>;
const DEFAULT_REPLAY_BODY_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AuthKind {
    ApiKey,
    Jwt,
}

#[derive(Debug, Default, Clone)]
struct ResponseHeaderMutations {
    add: Vec<HeaderValueNode>,
    set: Vec<HeaderValueNode>,
    remove: Vec<String>,
}

#[derive(Debug, Clone)]
struct RequestState {
    path: String,
    query: String,
    preserve_host: bool,
    response_headers: ResponseHeaderMutations,
}

#[derive(Debug, Clone)]
struct ReplayRequest {
    method: Method,
    version: Version,
    headers: HeaderMap,
    body: Bytes,
}

#[derive(Debug, Clone)]
enum SelectedService {
    Upstream {
        service_name: String,
        upstream_base_url: String,
    },
    DirectResponse {
        status: u16,
        headers: Vec<HeaderValueNode>,
        body: Option<ResponseBodyNode>,
    },
}

#[derive(Debug, Default, Clone)]
struct ExecutionPlan {
    attempts: Vec<SelectedService>,
    failover_on_status: Vec<u16>,
    mirrors: Vec<ExecutionPlan>,
}

impl ExecutionPlan {
    fn requires_replay(&self) -> bool {
        !self.mirrors.is_empty() || (!self.failover_on_status.is_empty() && self.attempts.len() > 1)
    }

    fn needs_status_failover_replay(&self) -> bool {
        !self.failover_on_status.is_empty() && self.attempts.len() > 1
    }
}

#[derive(Debug)]
enum ServiceSelectionError {
    NoHealthyUpstream,
    Internal(String),
}

pub(super) async fn handle(
    mut req: Request<Body>,
    gate: Arc<Gate>,
    auth_kind: Option<AuthKind>,
    subject: Option<Subject>,
) -> Result<Response, ErrorResponse> {
    let graph = gate.http_graph.load_full();

    let router = graph
        .routers
        .iter()
        .find(|router| router_matches(router, &req))
        .ok_or_else(|| ErrorResponse::from(HttpError::NotFound("Route not found".to_string())))?;

    let mut state = RequestState {
        path: req.uri().path().to_string(),
        query: req.uri().query().unwrap_or("").to_string(),
        preserve_host: false,
        response_headers: ResponseHeaderMutations::default(),
    };

    apply_middlewares(&graph, router, &mut req, &mut state)?;

    let request_id = reqctx::request_id_from(req.extensions());
    let client_ip = req.get_client_ip();
    let method = req.method().clone();

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_str(&request_id).unwrap(),
    );

    apply_policies(
        &graph,
        router,
        &gate,
        &mut req,
        auth_kind,
        subject.as_ref(),
        &client_ip,
        &mut response_headers,
    )
    .await?;

    let ctx = lb::RequestContext {
        client_ip: &client_ip,
        path: &state.path,
        method: method.as_str(),
        key: subject.as_ref().map(|sub| sub.id.as_str()),
    };

    let balancers = gate.http_balancers.load_full();
    let plan = build_execution_plan(
        &graph,
        balancers.as_ref(),
        &router.service,
        &ctx,
        &request_id,
    )
    .map_err(selection_error_response)?;

    let mut response = if req.get_protocol() == "ws" {
        let selected = plan.attempts.first().ok_or_else(|| {
            ErrorResponse::from(HttpError::ServiceUnavailable(
                "No healthy upstream available".to_string(),
            ))
        })?;
        execute_selected_with_request(selected, req, &state).await?
    } else if plan.requires_replay() {
        let limit = replay_body_limit();
        if !plan.needs_status_failover_replay() && content_length_exceeds(req.headers(), limit) {
            tracing::warn!(
                limit,
                "Skipping mirror traffic because request body exceeds replay limit"
            );
            let selected = plan.attempts.first().ok_or_else(|| {
                ErrorResponse::from(HttpError::ServiceUnavailable(
                    "No healthy upstream available".to_string(),
                ))
            })?;
            execute_selected_with_request(selected, req, &state).await?
        } else {
            let replay = buffer_request(req, limit).await?;
            spawn_mirrors(plan.mirrors.clone(), replay.clone(), state.clone());
            execute_plan_from_replay(&plan, &replay, &state).await?
        }
    } else {
        let selected = plan.attempts.first().ok_or_else(|| {
            ErrorResponse::from(HttpError::ServiceUnavailable(
                "No healthy upstream available".to_string(),
            ))
        })?;
        execute_selected_with_request(selected, req, &state).await?
    };

    apply_response_header_mutations(response.headers_mut(), &state.response_headers);
    apply_gateway_headers(response.headers_mut(), &response_headers);
    Ok(response)
}

async fn buffer_request(req: Request<Body>, limit: usize) -> Result<ReplayRequest, ErrorResponse> {
    let (parts, mut body) = req.into_parts();
    let mut bytes = Vec::new();

    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|error| {
            tracing::error!(%error, "Failed to buffer request body for replay");
            ErrorResponse::from(HttpError::BadRequest(
                "Failed to buffer request body".to_string(),
            ))
        })?;

        let Ok(data) = frame.into_data() else {
            continue;
        };

        let next_len = bytes.len().saturating_add(data.len());
        if next_len > limit {
            return Err(ErrorResponse::from(HttpError::PayloadTooLarge(format!(
                "Replay body limit exceeded: {} bytes",
                limit
            ))));
        }
        bytes.extend_from_slice(&data);
    }

    Ok(ReplayRequest {
        method: parts.method,
        version: parts.version,
        headers: parts.headers,
        body: Bytes::from(bytes),
    })
}

fn replay_body_limit() -> usize {
    std::env::var("GATEWAY_REPLAY_BODY_LIMIT")
        .ok()
        .and_then(|value| parse_byte_size(&value))
        .unwrap_or(DEFAULT_REPLAY_BODY_LIMIT)
}

fn content_length_exceeds(headers: &HeaderMap, limit: usize) -> bool {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|value| value > limit)
}

fn parse_byte_size(value: &str) -> Option<usize> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let split_at = value
        .find(|ch: char| !ch.is_ascii_digit() && ch != '_')
        .unwrap_or(value.len());
    let number = value[..split_at].replace('_', "");
    let unit = value[split_at..].trim().to_ascii_lowercase();
    let number = number.parse::<usize>().ok()?;
    let multiplier = match unit.as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        _ => return None,
    };

    number.checked_mul(multiplier)
}

impl ReplayRequest {
    fn build(&self, uri: &str) -> Result<Request<Body>, ErrorResponse> {
        let uri = uri.parse::<Uri>().map_err(|error| {
            tracing::error!(%uri, %error, "Invalid upstream URI");
            ErrorResponse::from(HttpError::BadGateway(
                "Failed to connect to backend service".to_string(),
            ))
        })?;

        let mut req = Request::new(Body::from(self.body.clone()));
        *req.method_mut() = self.method.clone();
        *req.version_mut() = self.version;
        *req.uri_mut() = uri;
        *req.headers_mut() = self.headers.clone();
        Ok(req)
    }
}

fn router_matches(router: &RouterNode, req: &Request<Body>) -> bool {
    matches_expr(&router.matcher, req)
}

fn matches_expr(expr: &MatchExprNode, req: &Request<Body>) -> bool {
    match expr {
        MatchExprNode::All(values) => values.iter().all(|value| matches_expr(value, req)),
        MatchExprNode::Any(values) => values.iter().any(|value| matches_expr(value, req)),
        MatchExprNode::Not(value) => !matches_expr(value, req),
        MatchExprNode::Host(predicate) => req
            .get_host()
            .map(|host| host_value_matches(predicate, &host))
            .unwrap_or_else(|| matches!(predicate, ValuePredicate::Present(false))),
        MatchExprNode::Method(methods) => methods
            .iter()
            .any(|method| method.eq_ignore_ascii_case(req.method().as_str())),
        MatchExprNode::Path(predicate) => path_matches(predicate, req.uri().path()),
        MatchExprNode::Header(predicate) => named_value_matches(
            predicate,
            req.headers()
                .get_all(predicate.name.as_str())
                .iter()
                .filter_map(|value| value.to_str().ok())
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ),
        MatchExprNode::Query(predicate) => {
            named_value_matches(predicate, req.get_query_values(&predicate.name))
        }
        MatchExprNode::Cookie(predicate) => named_value_matches(
            predicate,
            req.get_cookie_value(&predicate.name).into_iter().collect(),
        ),
        MatchExprNode::SourceIp(predicate) => req
            .get_client_ip_addr()
            .map(|ip| source_ip_matches(predicate, &ip))
            .unwrap_or(false),
    }
}

fn named_value_matches(predicate: &NamedValuePredicate, values: Vec<String>) -> bool {
    match &predicate.predicate {
        ValuePredicate::Present(expected) => {
            (*expected && !values.is_empty()) || (!expected && values.is_empty())
        }
        other => values.iter().any(|value| value_matches(other, value)),
    }
}

fn value_matches(predicate: &ValuePredicate, value: &str) -> bool {
    match predicate {
        ValuePredicate::Eq(expected) => value == expected,
        ValuePredicate::Prefix(expected) => value.starts_with(expected),
        ValuePredicate::Suffix(expected) => value.ends_with(expected),
        ValuePredicate::Contains(expected) => value.contains(expected),
        ValuePredicate::Regex(expected) => expected.is_match(value),
        ValuePredicate::Present(expected) => *expected,
        ValuePredicate::OneOf(expected) => expected.iter().any(|candidate| candidate == value),
    }
}

fn host_value_matches(predicate: &ValuePredicate, host: &str) -> bool {
    match predicate {
        ValuePredicate::Eq(expected) => host.eq_ignore_ascii_case(expected),
        ValuePredicate::Prefix(expected) => host
            .to_ascii_lowercase()
            .starts_with(&expected.to_ascii_lowercase()),
        ValuePredicate::Suffix(expected) => host
            .to_ascii_lowercase()
            .ends_with(&expected.to_ascii_lowercase()),
        ValuePredicate::Contains(expected) => host
            .to_ascii_lowercase()
            .contains(&expected.to_ascii_lowercase()),
        ValuePredicate::Regex(expected) => expected.is_match(host),
        ValuePredicate::Present(expected) => *expected,
        ValuePredicate::OneOf(expected) => expected
            .iter()
            .any(|candidate| host.eq_ignore_ascii_case(candidate)),
    }
}

fn path_matches(predicate: &PathPredicate, path: &str) -> bool {
    match predicate {
        PathPredicate::Exact(expected) => path == expected,
        PathPredicate::Prefix(expected) => path_prefix_matches(path, expected),
        PathPredicate::Template(expected) => template_matches(expected, path),
        PathPredicate::Regex(expected) => expected.is_match(path),
    }
}

fn path_prefix_matches(path: &str, prefix: &str) -> bool {
    let prefix = normalize_path(prefix);
    if prefix == "/" {
        return path.starts_with('/');
    }

    path == prefix
        || path
            .strip_prefix(&prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn template_matches(template: &str, path: &str) -> bool {
    let template = normalize_path(template);
    let template_segments = path_segments(&template);
    let path_segments = path_segments(path);

    if template_segments.len() != path_segments.len() {
        return false;
    }

    template_segments
        .iter()
        .zip(path_segments.iter())
        .all(|(template_segment, path_segment)| {
            is_template_segment(template_segment) || template_segment == path_segment
        })
}

fn is_template_segment(segment: &str) -> bool {
    segment.starts_with('{') && segment.ends_with('}') && segment.len() > 2
}

fn path_segments(path: &str) -> Vec<&str> {
    if path == "/" {
        return Vec::new();
    }
    path.trim_start_matches('/').split('/').collect()
}

fn normalize_path(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() || path == "/" {
        return "/".to_string();
    }

    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };

    if path.len() > 1 {
        path.trim_end_matches('/').to_string()
    } else {
        path
    }
}

fn source_ip_matches(predicate: &SourceIpPredicate, ip: &IpAddr) -> bool {
    predicate.cidrs.iter().any(|cidr| cidr_contains(cidr, ip))
}

fn cidr_contains(raw: &str, ip: &IpAddr) -> bool {
    let raw = raw.trim();
    if raw.is_empty() {
        return false;
    }

    let Some((addr, prefix)) = raw.split_once('/') else {
        return raw
            .parse::<IpAddr>()
            .ok()
            .is_some_and(|candidate| candidate == *ip);
    };

    let Ok(prefix_len) = prefix.parse::<u8>() else {
        return false;
    };
    let Ok(network) = addr.parse::<IpAddr>() else {
        return false;
    };

    match (network, ip) {
        (IpAddr::V4(network), IpAddr::V4(candidate)) if prefix_len <= 32 => {
            let mask = if prefix_len == 0 {
                0
            } else {
                !0u32 << (32 - prefix_len)
            };
            (u32::from(*candidate) & mask) == (u32::from(network) & mask)
        }
        (IpAddr::V6(network), IpAddr::V6(candidate)) if prefix_len <= 128 => {
            let mask = if prefix_len == 0 {
                0
            } else {
                !0u128 << (128 - prefix_len)
            };
            (u128::from(*candidate) & mask) == (u128::from(network) & mask)
        }
        _ => false,
    }
}

fn apply_middlewares(
    graph: &HttpGraph,
    router: &RouterNode,
    req: &mut Request<Body>,
    state: &mut RequestState,
) -> Result<(), ErrorResponse> {
    for middleware_name in &router.middlewares {
        let middleware = graph.middlewares.get(middleware_name).ok_or_else(|| {
            ErrorResponse::from(HttpError::InternalServerError(format!(
                "Middleware '{}' not found",
                middleware_name
            )))
        })?;

        match middleware {
            MiddlewareNode::StripPrefix { prefixes } => {
                for prefix in prefixes {
                    if path_prefix_matches(&state.path, prefix) {
                        state.path = strip_prefix(&state.path, prefix);
                        break;
                    }
                }
            }
            MiddlewareNode::AddPrefix { prefix } => {
                state.path = add_prefix(&state.path, prefix);
            }
            MiddlewareNode::ReplacePathRegex {
                pattern,
                replacement,
            } => {
                let next = pattern
                    .replace_all(&state.path, replacement.as_str())
                    .to_string();
                state.path = normalize_path(&next);
            }
            MiddlewareNode::PreserveHost => {
                state.preserve_host = true;
            }
            MiddlewareNode::RequestHeaders { add, set, remove } => {
                apply_request_headers(req.headers_mut(), add, set, remove);
            }
            MiddlewareNode::ResponseHeaders { add, set, remove } => {
                state.response_headers.add.extend(add.iter().cloned());
                state.response_headers.set.extend(set.iter().cloned());
                state.response_headers.remove.extend(remove.iter().cloned());
            }
        }
    }

    Ok(())
}

fn strip_prefix(path: &str, prefix: &str) -> String {
    let prefix = normalize_path(prefix);
    if path == prefix {
        return "/".to_string();
    }

    if let Some(rest) = path.strip_prefix(&prefix) {
        if rest.starts_with('/') {
            return normalize_path(rest);
        }
    }

    path.to_string()
}

fn add_prefix(path: &str, prefix: &str) -> String {
    let path = normalize_path(path);
    let prefix = normalize_path(prefix);
    if path == "/" {
        prefix
    } else if prefix == "/" {
        path
    } else {
        format!("{}{}", prefix, path)
    }
}

fn apply_request_headers(
    headers: &mut HeaderMap,
    add: &[HeaderValueNode],
    set: &[HeaderValueNode],
    remove: &[String],
) {
    for name in remove {
        headers.remove(name);
    }
    for header in add {
        append_header(headers, header);
    }
    for header in set {
        insert_header(headers, header);
    }
}

fn apply_response_header_mutations(headers: &mut HeaderMap, mutations: &ResponseHeaderMutations) {
    for name in &mutations.remove {
        headers.remove(name);
    }
    for header in &mutations.add {
        append_header(headers, header);
    }
    for header in &mutations.set {
        insert_header(headers, header);
    }
}

fn append_header(headers: &mut HeaderMap, header: &HeaderValueNode) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(header.name.as_bytes()),
        HeaderValue::from_str(&header.value),
    ) {
        headers.append(name, value);
    }
}

fn insert_header(headers: &mut HeaderMap, header: &HeaderValueNode) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(header.name.as_bytes()),
        HeaderValue::from_str(&header.value),
    ) {
        headers.insert(name, value);
    }
}

async fn apply_policies(
    graph: &HttpGraph,
    router: &RouterNode,
    gate: &Arc<Gate>,
    req: &mut Request<Body>,
    auth_kind: Option<AuthKind>,
    subject: Option<&Subject>,
    client_ip: &str,
    response_headers: &mut HeaderMap,
) -> Result<(), ErrorResponse> {
    let mut rate_limit_override = None;
    let mut quota_override = None;

    for policy_name in &router.policies {
        let policy = graph.policies.get(policy_name).ok_or_else(|| {
            ErrorResponse::from(HttpError::InternalServerError(format!(
                "Policy '{}' not found",
                policy_name
            )))
        })?;

        match policy {
            PolicyNode::Auth { strategies } => {
                let Some(kind) = auth_kind else {
                    return Err(ErrorResponse::from(HttpError::Unauthorized(
                        "Unauthorized".to_string(),
                    )));
                };
                if !auth_strategy_allowed(strategies, kind) {
                    return Err(ErrorResponse::from(HttpError::Unauthorized(
                        "Unauthorized".to_string(),
                    )));
                }
            }
            PolicyNode::AccessControl { resource, env } => {
                let Some(subject) = subject else {
                    return Err(ErrorResponse::from(HttpError::Unauthorized(
                        "Unauthorized".to_string(),
                    )));
                };

                let profile = env.unwrap_or(EnvProfile::Geo);
                let pe = gate.policy_engine.load();
                let env = reqctx::build_env_from(req.extensions_mut(), profile);
                let allowed = access_control(&pe, subject, &env, resource);

                if !allowed {
                    return Err(ErrorResponse::from(HttpError::Forbidden(
                        "Forbidden".to_string(),
                    )));
                }
            }
            PolicyNode::RateLimit { limit } => {
                if rate_limit_override.replace(limit.clone()).is_some() {
                    return Err(ErrorResponse::from(HttpError::InternalServerError(
                        "Multiple rate_limit policies are not supported".to_string(),
                    )));
                }
            }
            PolicyNode::Quota { limit, cost } => {
                if quota_override.replace((limit.clone(), *cost)).is_some() {
                    return Err(ErrorResponse::from(HttpError::InternalServerError(
                        "Multiple quota policies are not supported".to_string(),
                    )));
                }
            }
        }
    }

    apply_limits(
        gate,
        subject,
        client_ip,
        response_headers,
        rate_limit_override,
        quota_override,
    )
    .await
}

fn auth_strategy_allowed(strategies: &[AuthStrategy], kind: AuthKind) -> bool {
    match kind {
        AuthKind::ApiKey => strategies.contains(&AuthStrategy::ApiKey),
        AuthKind::Jwt => strategies.contains(&AuthStrategy::Jwt),
    }
}

async fn apply_limits(
    gate: &Arc<Gate>,
    subject: Option<&Subject>,
    client_ip: &str,
    headers: &mut HeaderMap,
    rate_limit_override: Option<String>,
    quota_override: Option<(String, u64)>,
) -> Result<(), ErrorResponse> {
    let limiter = gate.limiter.load();
    let mut sub_key = client_ip.to_string();
    let mut limit_name = rate_limit_override
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let mut quota_name = quota_override.as_ref().map(|(name, _)| name.clone());
    let mut quota_cost = quota_override.as_ref().map(|(_, cost)| *cost).unwrap_or(1);

    if let Some(subject) = subject {
        if rate_limit_override.is_none() {
            if let Some(rate_limit) = subject
                .get_attr("rate_limit")
                .and_then(|value| value.as_str())
            {
                limit_name = rate_limit.to_string();
            }
        }

        if quota_override.is_none() {
            if let Some(quota) = subject.get_attr("quota").and_then(|value| value.as_str()) {
                quota_name = Some(quota.to_string());
                quota_cost = 1;
            }
        }

        sub_key = subject.id.clone();
    }

    let mut key = String::with_capacity(4 + sub_key.len());
    key.push_str("lim:");
    key.push_str(&sub_key);

    let decision = limiter
        .check(&limit_name, &key, None)
        .await
        .map_err(|error| {
            tracing::error!("Rate limiter error: {}", error);
            ErrorResponse::from(HttpError::InternalServerError(
                "Rate limiter error".to_string(),
            ))
        })?;

    let limit = decision.limit.to_string();
    let remaining = decision.remaining.to_string();

    if !decision.is_allowed() {
        let retry_after = decision
            .retry_after
            .unwrap_or(std::time::Duration::from_secs(60));
        let retry_after = retry_after_header_value(retry_after);
        let mut response =
            ErrorResponse::from(HttpError::TooManyRequests("Too Many Requests".to_string()));
        response
            .insert_header("retry-after", &retry_after)
            .insert_header("x-ratelimit-limit", &limit)
            .insert_header("x-ratelimit-remaining", &remaining);
        return Err(response);
    }

    headers.insert(
        HeaderName::from_static("x-ratelimit-limit"),
        HeaderValue::from_str(&limit).unwrap(),
    );
    headers.insert(
        HeaderName::from_static("x-ratelimit-remaining"),
        HeaderValue::from_str(&remaining).unwrap(),
    );

    if let Some(quota_name) = quota_name {
        let mut quota_key = String::with_capacity(6 + sub_key.len());
        quota_key.push_str("quota:");
        quota_key.push_str(&sub_key);

        let decision = limiter
            .check(&quota_name, &quota_key, Some(quota_cost))
            .await
            .map_err(|error| {
                tracing::error!("Quota limiter error: {}", error);
                ErrorResponse::from(HttpError::InternalServerError(
                    "Quota limiter error".to_string(),
                ))
            })?;

        let limit = decision.limit.to_string();
        let remaining = decision.remaining.to_string();

        if !decision.is_allowed() {
            let retry_after = decision
                .retry_after
                .unwrap_or(std::time::Duration::from_secs(60));
            let retry_after = retry_after_header_value(retry_after);
            let mut response = ErrorResponse::from(HttpError::TooManyRequests(
                "Quota limit exceeded".to_string(),
            ));
            response
                .insert_header("retry-after", &retry_after)
                .insert_header("x-quota-limit", &limit)
                .insert_header("x-quota-remaining", &remaining);
            return Err(response);
        }

        headers.insert(
            HeaderName::from_static("x-quota-limit"),
            HeaderValue::from_str(&limit).unwrap(),
        );
        headers.insert(
            HeaderName::from_static("x-quota-remaining"),
            HeaderValue::from_str(&remaining).unwrap(),
        );
    }

    Ok(())
}

async fn execute_selected_with_request(
    selected: &SelectedService,
    req: Request<Body>,
    state: &RequestState,
) -> Result<Response, ErrorResponse> {
    match selected {
        SelectedService::DirectResponse {
            status,
            headers,
            body,
        } => build_direct_response(*status, headers, body.as_ref()),
        SelectedService::Upstream {
            service_name,
            upstream_base_url,
        } => {
            let uri = upstream_uri(upstream_base_url, state);
            if req.get_protocol() == "ws" {
                return ws::handler(req, &uri, state.preserve_host).await;
            }

            let client = get_client(service_name).ok_or_else(|| {
                ErrorResponse::from(HttpError::InternalServerError(
                    "HTTP client not found".to_string(),
                ))
            })?;

            let empty_headers = HeaderMap::new();
            http::handler(req, &empty_headers, &uri, &client, state.preserve_host).await
        }
    }
}

async fn execute_selected_from_replay(
    selected: &SelectedService,
    replay: &ReplayRequest,
    state: &RequestState,
) -> Result<Response, ErrorResponse> {
    match selected {
        SelectedService::DirectResponse {
            status,
            headers,
            body,
        } => build_direct_response(*status, headers, body.as_ref()),
        SelectedService::Upstream {
            service_name,
            upstream_base_url,
        } => {
            let uri = upstream_uri(upstream_base_url, state);
            let req = replay.build(&uri)?;
            let client = get_client(service_name).ok_or_else(|| {
                ErrorResponse::from(HttpError::InternalServerError(
                    "HTTP client not found".to_string(),
                ))
            })?;

            let empty_headers = HeaderMap::new();
            http::handler(req, &empty_headers, &uri, &client, state.preserve_host).await
        }
    }
}

async fn execute_plan_from_replay(
    plan: &ExecutionPlan,
    replay: &ReplayRequest,
    state: &RequestState,
) -> Result<Response, ErrorResponse> {
    let last_idx = plan.attempts.len().saturating_sub(1);
    for (idx, selected) in plan.attempts.iter().enumerate() {
        let response = execute_selected_from_replay(selected, replay, state).await?;
        if idx < last_idx && plan.should_failover_response(response.status()) {
            let status = response.status();
            tracing::warn!(
                status = status.as_u16(),
                attempt = idx,
                "Failing over response by status"
            );
            if let Err(error) = response.into_body().collect().await {
                tracing::warn!(%status, %error, "Failover response drain failed");
            }
            continue;
        }
        return Ok(response);
    }

    Err(ErrorResponse::from(HttpError::ServiceUnavailable(
        "No healthy upstream available".to_string(),
    )))
}

impl ExecutionPlan {
    fn should_failover_response(&self, status: StatusCode) -> bool {
        self.failover_on_status.contains(&status.as_u16())
    }
}

fn spawn_mirrors(mirrors: Vec<ExecutionPlan>, replay: ReplayRequest, state: RequestState) {
    for mirror in mirrors {
        let replay = replay.clone();
        let state = state.clone();
        tokio::spawn(async move {
            match execute_plan_from_replay(&mirror, &replay, &state).await {
                Ok(response) => {
                    let status = response.status();
                    if let Err(error) = response.into_body().collect().await {
                        tracing::warn!(%status, %error, "Mirror response drain failed");
                    }
                }
                Err(error) => tracing::warn!(%error, "Mirror request failed"),
            }
        });
    }
}

fn upstream_uri(base_url: &str, state: &RequestState) -> String {
    let mut uri = format!("{}{}", base_url, state.path);
    if !state.query.is_empty() {
        uri.push('?');
        uri.push_str(&state.query);
    }
    uri
}

fn build_execution_plan(
    graph: &HttpGraph,
    balancers: &HashMap<String, DynLoadBalancer>,
    service_name: &str,
    ctx: &lb::RequestContext<'_>,
    seed: &str,
) -> Result<ExecutionPlan, ServiceSelectionError> {
    let service = graph.services.get(service_name).ok_or_else(|| {
        ServiceSelectionError::Internal(format!("Service '{}' not found", service_name))
    })?;

    match service {
        ServiceNode::LoadBalancer { name, .. } => {
            let lb = balancers.get(name).ok_or_else(|| {
                ServiceSelectionError::Internal(format!("Load balancer '{}' not found", name))
            })?;

            let upstream = lb
                .select(ctx)
                .ok_or(ServiceSelectionError::NoHealthyUpstream)?;

            Ok(ExecutionPlan {
                attempts: vec![SelectedService::Upstream {
                    service_name: name.clone(),
                    upstream_base_url: upstream.base_url.clone(),
                }],
                ..ExecutionPlan::default()
            })
        }
        ServiceNode::Weighted { services, .. } => {
            let child = choose_weighted_service(services, seed).ok_or_else(|| {
                ServiceSelectionError::Internal(format!(
                    "Weighted service '{}' has no targets",
                    service_name
                ))
            })?;
            build_execution_plan(graph, balancers, child, ctx, seed)
        }
        ServiceNode::Mirror {
            service, mirrors, ..
        } => {
            let mut plan = build_execution_plan(graph, balancers, service, ctx, seed)?;
            for mirror in mirrors {
                if !mirror_selected(mirror.percent, seed, &mirror.service) {
                    continue;
                }
                match build_execution_plan(graph, balancers, &mirror.service, ctx, seed) {
                    Ok(mirror_plan) => plan.mirrors.push(mirror_plan),
                    Err(error) => tracing::warn!(
                        service = mirror.service.as_str(),
                        error = %selection_error_message(&error),
                        "Mirror target skipped"
                    ),
                }
            }
            Ok(plan)
        }
        ServiceNode::Failover {
            service,
            failovers,
            on_status,
            ..
        } => {
            let mut plan = ExecutionPlan::default();
            append_available_plan(&mut plan, graph, balancers, service, ctx, seed)?;
            for failover in failovers {
                append_available_plan(&mut plan, graph, balancers, failover, ctx, seed)?;
            }

            if plan.attempts.is_empty() {
                return Err(ServiceSelectionError::NoHealthyUpstream);
            }

            plan.failover_on_status.extend(on_status.iter().copied());
            Ok(plan)
        }
        ServiceNode::DirectResponse {
            status,
            headers,
            body,
            ..
        } => Ok(ExecutionPlan {
            attempts: vec![SelectedService::DirectResponse {
                status: *status,
                headers: headers.clone(),
                body: body.clone(),
            }],
            ..ExecutionPlan::default()
        }),
    }
}

fn append_available_plan(
    target: &mut ExecutionPlan,
    graph: &HttpGraph,
    balancers: &HashMap<String, DynLoadBalancer>,
    service_name: &str,
    ctx: &lb::RequestContext<'_>,
    seed: &str,
) -> Result<(), ServiceSelectionError> {
    match build_execution_plan(graph, balancers, service_name, ctx, seed) {
        Ok(plan) => {
            target.attempts.extend(plan.attempts);
            target.failover_on_status.extend(plan.failover_on_status);
            target.mirrors.extend(plan.mirrors);
            Ok(())
        }
        Err(ServiceSelectionError::NoHealthyUpstream) => Ok(()),
        Err(error) => Err(error),
    }
}

fn mirror_selected(percent: u8, seed: &str, service: &str) -> bool {
    if percent >= 100 {
        return true;
    }

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    service.hash(&mut hasher);
    hasher.finish() % 100 < u64::from(percent)
}

fn selection_error_message(error: &ServiceSelectionError) -> String {
    match error {
        ServiceSelectionError::NoHealthyUpstream => "no healthy upstream".to_string(),
        ServiceSelectionError::Internal(message) => message.clone(),
    }
}

fn choose_weighted_service<'a>(
    services: &'a [gate::graph::WeightedServiceNode],
    seed: &str,
) -> Option<&'a str> {
    let total: u64 = services
        .iter()
        .map(|service| u64::from(service.weight))
        .sum();
    if total == 0 {
        return None;
    }

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let bucket = hasher.finish() % total;

    let mut seen = 0u64;
    for service in services {
        seen += u64::from(service.weight);
        if bucket < seen {
            return Some(service.service.as_str());
        }
    }

    services.last().map(|service| service.service.as_str())
}

fn selection_error_response(error: ServiceSelectionError) -> ErrorResponse {
    match error {
        ServiceSelectionError::NoHealthyUpstream => ErrorResponse::from(
            HttpError::ServiceUnavailable("No healthy upstream available".to_string()),
        ),
        ServiceSelectionError::Internal(message) => {
            ErrorResponse::from(HttpError::InternalServerError(message))
        }
    }
}

fn build_direct_response(
    status: u16,
    headers: &[HeaderValueNode],
    body: Option<&ResponseBodyNode>,
) -> Result<Response, ErrorResponse> {
    let status = StatusCode::from_u16(status).map_err(|_| {
        ErrorResponse::from(HttpError::InternalServerError(
            "Invalid direct response status".to_string(),
        ))
    })?;

    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;

    for header in headers {
        insert_header(response.headers_mut(), header);
    }

    if let Some(body) = body {
        match body {
            ResponseBodyNode::Text(text) => {
                if !response.headers().contains_key(CONTENT_TYPE) {
                    response.headers_mut().insert(
                        CONTENT_TYPE,
                        HeaderValue::from_static("text/plain; charset=utf-8"),
                    );
                }
                *response.body_mut() = Body::from(text.clone());
            }
            ResponseBodyNode::Json(value) => {
                if !response.headers().contains_key(CONTENT_TYPE) {
                    response
                        .headers_mut()
                        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                }
                let payload = serde_json::to_vec(value).map_err(ErrorResponse::internal)?;
                *response.body_mut() = Body::from(payload);
            }
        }
    }

    Ok(response)
}

fn apply_gateway_headers(headers: &mut HeaderMap, additions: &HeaderMap) {
    for (name, value) in additions {
        headers.insert(name, value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AuthKind, RequestState, ResponseHeaderMutations, add_prefix, apply_middlewares,
        buffer_request, build_execution_plan, choose_weighted_service, execute_plan_from_replay,
        matches_expr, mirror_selected, parse_byte_size, path_prefix_matches, strip_prefix,
    };
    use axum::body::Body;
    use axum::{Router, extract::State, response::Response, routing::any};
    use gate::Gate;
    use gate::cfg::RuntimeConfig;
    use gate::graph::{
        HeaderValueNode, HttpGraph, MatchExprNode, MiddlewareNode, MirrorServiceNode,
        NamedValuePredicate, PathPredicate, ResponseBodyNode, RouterNode, ServiceNode,
        ValuePredicate, WeightedServiceNode,
    };
    use http::{HeaderMap, Method, Request, StatusCode, Version};
    use http_body_util::BodyExt;
    use hyper::body::Bytes;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::{Mutex, mpsc};
    use tokio::time::{Duration, timeout};

    static GATEWAY_TEST_LOCK: Mutex<()> = Mutex::const_new(());

    #[derive(Debug)]
    struct CapturedRequest {
        path: String,
        query: Option<String>,
        body: Bytes,
        headers: HeaderMap,
    }

    #[derive(Clone)]
    struct CaptureState {
        status: StatusCode,
        body: &'static str,
        tx: mpsc::UnboundedSender<CapturedRequest>,
    }

    async fn capture_handler(State(state): State<CaptureState>, req: Request<Body>) -> Response {
        let path = req.uri().path().to_string();
        let query = req.uri().query().map(ToString::to_string);
        let headers = req.headers().clone();
        let body = req.into_body().collect().await.unwrap().to_bytes();
        let _ = state.tx.send(CapturedRequest {
            path,
            query,
            body,
            headers,
        });

        Response::builder()
            .status(state.status)
            .body(Body::from(state.body))
            .unwrap()
    }

    async fn spawn_capture_server(
        status: StatusCode,
        body: &'static str,
    ) -> (String, mpsc::UnboundedReceiver<CapturedRequest>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        let state = CaptureState { status, body, tx };
        let app = Router::new()
            .fallback(any(capture_handler))
            .with_state(state);
        tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, app).await {
                tracing::error!(%error, "test capture server failed");
            }
        });

        (format!("http://{}", addr), rx)
    }

    fn build_test_gate(config: RuntimeConfig) -> Arc<Gate> {
        crate::etc::gate::set_config_for_test(config.clone());
        Arc::new(Gate::new(Arc::new(lim::State::new())).build(&config, "/tmp/stargate-policies"))
    }

    fn default_limit_yaml() -> &'static str {
        r#"
limits:
  - name: default
    strategy: gcra
    params:
      max_burst: 1000000
      replenish_1_per: 1us
"#
    }

    async fn recv_request(rx: &mut mpsc::UnboundedReceiver<CapturedRequest>) -> CapturedRequest {
        timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap()
    }

    #[test]
    fn matchers_cover_host_query_cookie_and_not() {
        let req = Request::builder()
            .uri("http://api.example.com/api/reports?preview=true")
            .header("host", "api.example.com")
            .header("cookie", "canary=v2")
            .body(Body::empty())
            .unwrap();

        let expr = MatchExprNode::All(vec![
            MatchExprNode::Host(ValuePredicate::Eq("api.example.com".to_string())),
            MatchExprNode::Path(PathPredicate::Prefix("/api".to_string())),
            MatchExprNode::Query(NamedValuePredicate {
                name: "preview".to_string(),
                predicate: ValuePredicate::Eq("true".to_string()),
            }),
            MatchExprNode::Cookie(NamedValuePredicate {
                name: "canary".to_string(),
                predicate: ValuePredicate::OneOf(vec!["v2".to_string(), "v3".to_string()]),
            }),
            MatchExprNode::Not(Box::new(MatchExprNode::Header(NamedValuePredicate {
                name: "x-blocked".to_string(),
                predicate: ValuePredicate::Present(true),
            }))),
        ]);

        assert!(matches_expr(&expr, &req));
        assert!(matches!(AuthKind::Jwt, AuthKind::Jwt));
    }

    #[test]
    fn middlewares_rewrite_path_and_collect_response_headers() {
        let mut graph = HttpGraph::default();
        graph.middlewares.insert(
            "rewrite".to_string(),
            MiddlewareNode::StripPrefix {
                prefixes: vec!["/api".to_string()],
            },
        );
        graph.middlewares.insert(
            "prefix".to_string(),
            MiddlewareNode::AddPrefix {
                prefix: "/v2".to_string(),
            },
        );
        graph.middlewares.insert(
            "response".to_string(),
            MiddlewareNode::ResponseHeaders {
                add: vec![HeaderValueNode {
                    name: "x-added".to_string(),
                    value: "1".to_string(),
                }],
                set: Vec::new(),
                remove: vec!["x-remove".to_string()],
            },
        );

        let router = RouterNode {
            name: "reports".to_string(),
            priority: 0,
            order: 0,
            matcher: MatchExprNode::Path(PathPredicate::Prefix("/api".to_string())),
            service: "reports".to_string(),
            middlewares: vec![
                "rewrite".to_string(),
                "prefix".to_string(),
                "response".to_string(),
            ],
            policies: Vec::new(),
        };

        let mut req = Request::builder()
            .uri("http://example.com/api/reports")
            .header("host", "example.com")
            .body(Body::empty())
            .unwrap();
        let mut state = RequestState {
            path: "/api/reports".to_string(),
            query: String::new(),
            preserve_host: false,
            response_headers: ResponseHeaderMutations::default(),
        };

        apply_middlewares(&graph, &router, &mut req, &mut state).unwrap();

        assert_eq!(state.path, "/v2/reports");
        assert_eq!(state.response_headers.remove, vec!["x-remove".to_string()]);
        assert_eq!(state.response_headers.add.len(), 1);
    }

    #[test]
    fn weighted_selection_is_deterministic() {
        let services = vec![
            WeightedServiceNode {
                service: "v1".to_string(),
                weight: 95,
            },
            WeightedServiceNode {
                service: "v2".to_string(),
                weight: 5,
            },
        ];

        let first = choose_weighted_service(&services, "req-1");
        let second = choose_weighted_service(&services, "req-1");

        assert_eq!(first, second);
    }

    #[test]
    fn path_helpers_normalize_expected_shape() {
        assert_eq!(strip_prefix("/api/reports", "/api"), "/reports");
        assert_eq!(strip_prefix("/api", "/api"), "/");
        assert_eq!(add_prefix("/reports", "/v2"), "/v2/reports");
        assert!(path_prefix_matches("/missing", "/"));
    }

    #[test]
    fn byte_size_parser_accepts_binary_units() {
        assert_eq!(parse_byte_size("512"), Some(512));
        assert_eq!(parse_byte_size("2MiB"), Some(2 * 1024 * 1024));
        assert_eq!(parse_byte_size("1_gb"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_byte_size("bad"), None);
    }

    #[tokio::test]
    async fn replay_buffer_rejects_body_over_limit() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("http://example.com/upload")
            .body(Body::from(Bytes::from_static(b"abcdef")))
            .unwrap();

        let error = buffer_request(req, 3).await.unwrap_err();
        assert_eq!(error.code, StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn mirror_plan_includes_percent_selected_targets() {
        let mut graph = HttpGraph::default();
        graph.services.insert(
            "primary".to_string(),
            ServiceNode::DirectResponse {
                name: "primary".to_string(),
                status: 204,
                headers: Vec::new(),
                body: None,
            },
        );
        graph.services.insert(
            "shadow".to_string(),
            ServiceNode::DirectResponse {
                name: "shadow".to_string(),
                status: 204,
                headers: Vec::new(),
                body: None,
            },
        );
        graph.services.insert(
            "mirror".to_string(),
            ServiceNode::Mirror {
                name: "mirror".to_string(),
                service: "primary".to_string(),
                mirrors: vec![MirrorServiceNode {
                    service: "shadow".to_string(),
                    percent: 100,
                }],
            },
        );

        let balancers = HashMap::new();
        let ctx = lb::RequestContext {
            client_ip: "127.0.0.1",
            path: "/",
            method: "GET",
            key: None,
        };
        let plan = build_execution_plan(&graph, &balancers, "mirror", &ctx, "req-1").unwrap();

        assert_eq!(plan.attempts.len(), 1);
        assert_eq!(plan.mirrors.len(), 1);
        assert!(plan.requires_replay());
        assert!(mirror_selected(100, "req-1", "shadow"));
        assert!(!mirror_selected(0, "req-1", "shadow"));
    }

    #[tokio::test]
    async fn status_failover_returns_next_attempt_response() {
        let plan = super::ExecutionPlan {
            attempts: vec![
                super::SelectedService::DirectResponse {
                    status: 503,
                    headers: Vec::new(),
                    body: Some(ResponseBodyNode::Text("primary".to_string())),
                },
                super::SelectedService::DirectResponse {
                    status: 200,
                    headers: Vec::new(),
                    body: Some(ResponseBodyNode::Text("backup".to_string())),
                },
            ],
            failover_on_status: vec![503],
            mirrors: Vec::new(),
        };
        let replay = super::ReplayRequest {
            method: Method::GET,
            version: Version::HTTP_11,
            headers: HeaderMap::new(),
            body: Bytes::new(),
        };
        let state = RequestState {
            path: "/".to_string(),
            query: String::new(),
            preserve_host: false,
            response_headers: ResponseHeaderMutations::default(),
        };

        let response = execute_plan_from_replay(&plan, &replay, &state)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, Bytes::from_static(b"backup"));
    }

    #[tokio::test]
    async fn v2_routes_to_real_upstreams_by_header_query_regex_and_direct_fallback() {
        let _guard = GATEWAY_TEST_LOCK.lock().await;
        let (v2_url, mut v2_rx) = spawn_capture_server(StatusCode::OK, "v2").await;
        let (staging_url, mut staging_rx) = spawn_capture_server(StatusCode::OK, "staging").await;
        let yaml = format!(
            r#"
schema: stargate/v2alpha1
{}
http:
  upstreams:
    reports-v2:
      targets:
        - url: {}
    staging:
      targets:
        - url: {}
  services:
    reports-v2:
      kind: load_balancer
      upstream: reports-v2
    staging:
      kind: load_balancer
      upstream: staging
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
  routers:
    header-v2:
      priority: 1000
      match:
        all:
          - path:
              prefix: /api/reports
          - header:
              name: x-version
              eq: v2
      service: reports-v2
      middlewares: [strip-api]
    preview:
      priority: 900
      match:
        all:
          - path:
              prefix: /api/reports
          - query:
              name: preview
              eq: "true"
      service: staging
      middlewares: [strip-api]
    regex-legacy:
      priority: 800
      match:
        path:
          regex: ^/api/legacy/.+$
      service: reports-v2
      middlewares: [legacy-rewrite]
    fallback:
      priority: -1000
      match:
        path:
          prefix: /
      service: not-found
"#,
            default_limit_yaml(),
            v2_url,
            staging_url,
        );
        let config = RuntimeConfig::from_yaml_str(&yaml).unwrap();
        let gate = build_test_gate(config);

        let response = super::handle(
            Request::builder()
                .uri("/api/reports/42")
                .header("x-version", "v2")
                .body(Body::empty())
                .unwrap(),
            Arc::clone(&gate),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let captured = recv_request(&mut v2_rx).await;
        assert_eq!(captured.path, "/reports/42");
        assert!(captured.query.is_none());
        assert_eq!(captured.headers.get("x-version").unwrap(), "v2");

        let response = super::handle(
            Request::builder()
                .uri("/api/reports?preview=true")
                .body(Body::empty())
                .unwrap(),
            Arc::clone(&gate),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let captured = recv_request(&mut staging_rx).await;
        assert_eq!(captured.path, "/reports");
        assert_eq!(captured.query.as_deref(), Some("preview=true"));

        let response = super::handle(
            Request::builder()
                .uri("/api/legacy/foo")
                .body(Body::empty())
                .unwrap(),
            Arc::clone(&gate),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let captured = recv_request(&mut v2_rx).await;
        assert_eq!(captured.path, "/v2/foo");

        let response = super::handle(
            Request::builder()
                .uri("/missing")
                .body(Body::empty())
                .unwrap(),
            gate,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "ROUTE_NOT_FOUND");
    }

    #[tokio::test]
    async fn v2_mirrors_and_status_failover_replay_body_to_real_upstreams() {
        let _guard = GATEWAY_TEST_LOCK.lock().await;
        let (primary_url, mut primary_rx) =
            spawn_capture_server(StatusCode::SERVICE_UNAVAILABLE, "primary").await;
        let (backup_url, mut backup_rx) = spawn_capture_server(StatusCode::OK, "backup").await;
        let (shadow_url, mut shadow_rx) = spawn_capture_server(StatusCode::NO_CONTENT, "").await;
        let yaml = format!(
            r#"
schema: stargate/v2alpha1
{}
http:
  upstreams:
    primary:
      targets:
        - url: {}
    backup:
      targets:
        - url: {}
    shadow:
      targets:
        - url: {}
  services:
    primary:
      kind: load_balancer
      upstream: primary
    backup:
      kind: load_balancer
      upstream: backup
    shadow:
      kind: load_balancer
      upstream: shadow
    failover:
      kind: failover
      service: primary
      failovers: [backup]
      on_status: [503]
    mirrored:
      kind: mirror
      service: failover
      mirrors:
        - service: shadow
          percent: 100
  routers:
    upload:
      match:
        path:
          exact: /api/upload
      service: mirrored
"#,
            default_limit_yaml(),
            primary_url,
            backup_url,
            shadow_url,
        );
        let config = RuntimeConfig::from_yaml_str(&yaml).unwrap();
        let gate = build_test_gate(config);

        let response = super::handle(
            Request::builder()
                .method(Method::POST)
                .uri("/api/upload")
                .body(Body::from(Bytes::from_static(b"payload")))
                .unwrap(),
            gate,
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body, Bytes::from_static(b"backup"));

        let primary = recv_request(&mut primary_rx).await;
        let backup = recv_request(&mut backup_rx).await;
        let shadow = recv_request(&mut shadow_rx).await;

        assert_eq!(primary.path, "/api/upload");
        assert_eq!(backup.path, "/api/upload");
        assert_eq!(shadow.path, "/api/upload");
        assert_eq!(primary.body, Bytes::from_static(b"payload"));
        assert_eq!(backup.body, Bytes::from_static(b"payload"));
        assert_eq!(shadow.body, Bytes::from_static(b"payload"));
    }
}
