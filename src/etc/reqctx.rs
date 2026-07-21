use crate::etc::{
    ac::Env,
    geoip,
    guard::{AuthKind, VerifiedIdentity},
    sub::SubjectType,
};
use chrono::{DateTime, Utc};
use ctx::{
    Actor, ActorType, Authentication, AuthenticationKind, DispatchContext, DispatchKind,
    IssueRequest, MAX_METHOD_BYTES, MAX_NAME_BYTES, MAX_PATH_BYTES, MAX_REQUEST_ID_BYTES,
    MAX_ROLE_BYTES, MAX_USER_AGENT_BYTES, Organization, RequestContext as PropagatedRequestContext,
    RouteContext, StargateContext, VERSION, truncate_utf8,
};
use db::ent::{TrustedAuditContext, TrustedAuditRequest};
use gate::cfg::EnvProfile;
use http::header::{HeaderName, HeaderValue};
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use ulid::Ulid;

pub const INTERNAL_CONTEXT_HEADER: HeaderName = HeaderName::from_static("stargate-context");
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Clone)]
pub struct RequestContext {
    request_id: Box<str>,
    started_at: DateTime<Utc>,
    client_ip: Option<IpAddr>,
    user_agent: Option<Box<str>>,
    trace_id: Option<Box<str>>,
    geo: OnceLock<Arc<geoip::GeoInfo>>,
}

impl RequestContext {
    pub fn new(
        request_id: String,
        started_at: DateTime<Utc>,
        client_ip: Option<IpAddr>,
        user_agent: Option<String>,
        trace_id: Option<String>,
    ) -> Self {
        Self {
            request_id: request_id.into_boxed_str(),
            started_at,
            client_ip,
            user_agent: user_agent.map(String::into_boxed_str),
            trace_id: trace_id.map(String::into_boxed_str),
            geo: OnceLock::new(),
        }
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn client_ip(&self) -> Option<IpAddr> {
        self.client_ip
    }

    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }

    pub fn trace_id(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }

    pub fn audit_request(&self) -> TrustedAuditRequest {
        TrustedAuditRequest::from_http(
            self.request_id(),
            self.trace_id.as_deref().map(str::to_string),
            self.client_ip.map(|ip| ip.to_string()),
            self.user_agent.as_deref().map(str::to_string),
        )
    }

    pub fn env(&self, profile: EnvProfile) -> Env {
        match profile {
            EnvProfile::None => Env::default(),
            EnvProfile::Basic => self.basic_env(),
            EnvProfile::Geo => self.geo_env(),
        }
    }

    fn basic_env(&self) -> Env {
        Env {
            ip_address: self
                .client_ip
                .map(|ip| ip.to_string().into_boxed_str())
                .unwrap_or_else(|| "unknown".into()),
            user_agent: self.user_agent.clone().unwrap_or_else(|| "unknown".into()),
            country_code: Box::default(),
            country_name: Box::default(),
            city_name: Box::default(),
            date: self
                .started_at
                .format("%Y-%m-%d")
                .to_string()
                .into_boxed_str(),
            time: self.started_at.format("%H:%M").to_string().into_boxed_str(),
            day_of_week: self.started_at.format("%A").to_string().into_boxed_str(),
        }
    }

    fn geo_env(&self) -> Env {
        let mut env = self.basic_env();
        let geo = self.geo();
        env.country_code = geo.country_code.clone().unwrap_or_else(|| "unknown".into());
        env.country_name = geo.country_name.clone().unwrap_or_else(|| "unknown".into());
        env.city_name = geo.city_name.clone().unwrap_or_else(|| "unknown".into());
        env
    }

    fn geo(&self) -> &Arc<geoip::GeoInfo> {
        self.geo.get_or_init(|| {
            self.client_ip
                .and_then(geoip::lookup)
                .unwrap_or_else(geoip::GeoInfo::unknown)
        })
    }
}

/// Remove untrusted gateway-owned context and install the request id generated
/// for this ingress request before routing or configurable middleware runs.
pub fn sanitize_ingress_headers(
    headers: &mut http::HeaderMap,
    request_id: &str,
) -> Result<(), http::header::InvalidHeaderValue> {
    headers.remove(&INTERNAL_CONTEXT_HEADER);
    headers.remove(&REQUEST_ID_HEADER);
    headers.insert(&REQUEST_ID_HEADER, HeaderValue::from_str(request_id)?);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PropagationDraftError {
    #[error("verified identity is inconsistent")]
    Identity,
    #[error("trusted request context is invalid")]
    Request,
    #[error("compiled route context is invalid")]
    Route,
}

impl PropagationDraftError {
    pub const fn category(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Request => "request",
            Self::Route => "route",
        }
    }
}

/// Immutable, unsigned facts captured after routing and policy success. It has
/// no audience or dispatch data and therefore cannot itself be emitted.
#[derive(Clone, PartialEq, Eq)]
pub struct PropagationDraft {
    subject: Option<String>,
    actor: Actor,
    authentication: Authentication,
    organization: Option<Organization>,
    request: PropagatedRequestContext,
    route: RouteContext,
}

impl PropagationDraft {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        identity: &VerifiedIdentity,
        request_context: &RequestContext,
        method: &str,
        path: &str,
        original_path: &str,
        router: &str,
        service: &str,
        policy_revision: Option<String>,
    ) -> Result<Self, PropagationDraftError> {
        let (subject, actor, authentication, organization) = identity_facts(identity)?;
        validate_request_facts(request_context, method, path, original_path)?;
        validate_route_facts(router, service, policy_revision.as_deref())?;

        Ok(Self {
            subject,
            actor,
            authentication,
            organization,
            request: PropagatedRequestContext {
                id: request_context.request_id().to_owned(),
                trace_id: request_context.trace_id().map(str::to_owned),
                method: method.to_owned(),
                path: path.to_owned(),
                original_path: original_path.to_owned(),
                client_ip: request_context.client_ip().map(|ip| ip.to_string()),
                user_agent: request_context
                    .user_agent()
                    .map(|value| truncate_utf8(value, MAX_USER_AGENT_BYTES).to_owned()),
            },
            route: RouteContext {
                router: router.to_owned(),
                service: service.to_owned(),
                policy_revision,
            },
        })
    }

    pub fn actor_label(&self) -> &'static str {
        match self.actor.actor_type {
            ActorType::User => "user",
            ActorType::ApiKey => "api_key",
            ActorType::Anonymous => "anonymous",
        }
    }

    pub fn request_id(&self) -> &str {
        &self.request.id
    }

    pub fn trace_id(&self) -> Option<&str> {
        self.request.trace_id.as_deref()
    }

    /// Bind this immutable request snapshot to one selected leaf and network
    /// attempt. The caller supplies the final encoded path after the target
    /// base URL is resolved, so the token describes the exact request on the
    /// wire even when a target URL contributes a path prefix.
    pub fn issue_request(
        &self,
        audience: &str,
        kind: DispatchKind,
        attempt: u16,
        method: &str,
        encoded_path: &str,
    ) -> Result<IssueRequest, PropagationBindingError> {
        if self.request.method != method {
            return Err(PropagationBindingError);
        }

        let mut request = self.request.clone();
        request.path = encoded_path.to_owned();

        Ok(IssueRequest {
            audience: audience.to_owned(),
            subject: self.subject.clone(),
            context: StargateContext {
                v: VERSION,
                actor: self.actor.clone(),
                authentication: self.authentication.clone(),
                organization: self.organization.clone(),
                request,
                route: self.route.clone(),
                dispatch: DispatchContext { kind, attempt },
            },
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the final upstream request does not match the propagation draft")]
pub struct PropagationBindingError;

impl std::fmt::Debug for PropagationDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PropagationDraft([redacted])")
    }
}

fn identity_facts(
    identity: &VerifiedIdentity,
) -> Result<(Option<String>, Actor, Authentication, Option<Organization>), PropagationDraftError> {
    let Some(subject) = identity.subject() else {
        if identity.auth_kind() != AuthKind::Anonymous {
            return Err(PropagationDraftError::Identity);
        }
        return Ok((
            None,
            Actor {
                actor_type: ActorType::Anonymous,
            },
            Authentication {
                kind: AuthenticationKind::None,
                sid: None,
                auth_time: None,
            },
            None,
        ));
    };

    if !valid_ulid(&subject.id) || (subject.org_id.is_none() && subject.org_role.is_some()) {
        return Err(PropagationDraftError::Identity);
    }
    let organization = match subject.org_id.as_deref() {
        Some(id) if valid_ulid(id) => {
            if let Some(role) = subject.org_role.as_deref()
                && (role.trim().is_empty() || role.len() > MAX_ROLE_BYTES)
            {
                return Err(PropagationDraftError::Identity);
            }
            Some(Organization {
                id: id.to_owned(),
                role: subject.org_role.clone(),
            })
        }
        Some(_) => return Err(PropagationDraftError::Identity),
        None => None,
    };

    match (identity.auth_kind(), &subject.sub_type) {
        (AuthKind::Jwt, SubjectType::User) => {
            let session_id = identity
                .session_id()
                .filter(|value| valid_ulid(value))
                .ok_or(PropagationDraftError::Identity)?;
            if organization
                .as_ref()
                .is_some_and(|organization| organization.role.is_none())
            {
                return Err(PropagationDraftError::Identity);
            }
            Ok((
                Some(subject.id.clone()),
                Actor {
                    actor_type: ActorType::User,
                },
                Authentication {
                    kind: AuthenticationKind::Jwt,
                    sid: Some(session_id.to_owned()),
                    auth_time: identity.auth_time(),
                },
                organization,
            ))
        }
        (AuthKind::ApiKey, SubjectType::ApiKey) => Ok((
            Some(subject.id.clone()),
            Actor {
                actor_type: ActorType::ApiKey,
            },
            Authentication {
                kind: AuthenticationKind::ApiKey,
                sid: None,
                auth_time: None,
            },
            organization,
        )),
        _ => Err(PropagationDraftError::Identity),
    }
}

fn validate_request_facts(
    request_context: &RequestContext,
    method: &str,
    path: &str,
    original_path: &str,
) -> Result<(), PropagationDraftError> {
    if !valid_ulid(request_context.request_id())
        || request_context.request_id().len() > MAX_REQUEST_ID_BYTES
        || method.is_empty()
        || method.len() > MAX_METHOD_BYTES
        || !method.is_ascii()
        || method
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || !valid_path(path)
        || !valid_path(original_path)
        || request_context.trace_id().is_some_and(|trace_id| {
            trace_id.len() != 32
                || !trace_id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    {
        return Err(PropagationDraftError::Request);
    }
    Ok(())
}

fn validate_route_facts(
    router: &str,
    service: &str,
    policy_revision: Option<&str>,
) -> Result<(), PropagationDraftError> {
    if invalid_name(router) || invalid_name(service) || policy_revision.is_some_and(invalid_name) {
        return Err(PropagationDraftError::Route);
    }
    Ok(())
}

fn invalid_name(value: &str) -> bool {
    value.trim().is_empty() || value.len() > MAX_NAME_BYTES
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.starts_with('/')
        && path.len() <= MAX_PATH_BYTES
        && !path.contains(['?', '#'])
}

fn valid_ulid(value: &str) -> bool {
    value.len() == 26 && value.bytes().all(|byte| {
        byte.is_ascii_digit()
            || matches!(byte, b'A'..=b'H' | b'J'..=b'K' | b'M'..=b'N' | b'P'..=b'T' | b'V'..=b'Z')
    }) && Ulid::from_string(value).is_ok()
}

pub fn request_id_from(extensions: &http::Extensions) -> String {
    extensions
        .get::<RequestContext>()
        .map(|ctx| ctx.request_id().to_string())
        .unwrap_or_else(|| Ulid::new().to_string())
}

pub fn audit_request_from(extensions: &http::Extensions) -> TrustedAuditRequest {
    extensions
        .get::<RequestContext>()
        .map(RequestContext::audit_request)
        .unwrap_or_else(|| {
            TrustedAuditRequest::from_http(request_id_from(extensions), None, None, None)
        })
}

/// Consume a context established by a trusted route or authentication
/// boundary. This never invents a scope.
pub fn take_trusted_audit_context_from(
    extensions: &mut http::Extensions,
) -> Option<TrustedAuditContext> {
    extensions.remove::<TrustedAuditContext>()
}

pub fn build_env_from(extensions: &mut http::Extensions, profile: EnvProfile) -> Env {
    if let Some(env) = extensions.remove::<Env>() {
        return env;
    }
    extensions
        .get::<RequestContext>()
        .map(|ctx| ctx.env(profile))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etc::{
        guard::VerifiedIdentity,
        sub::{Subject, SubjectType},
    };
    use chrono::Utc;
    use http::{HeaderMap, HeaderValue};
    use serde_json::json;

    fn request_context(user_agent: Option<String>) -> RequestContext {
        RequestContext::new(
            "01JZ000000000000000000000R".to_owned(),
            Utc::now(),
            Some("203.0.113.10".parse().unwrap()),
            user_agent,
            Some("4bf92f3577b34da6a3ce929d0e0e4736".to_owned()),
        )
    }

    #[test]
    fn user_draft_maps_only_contract_identity_and_request_facts() {
        let mut subject = Subject::new(
            "01JZ000000000000000000000A".to_owned(),
            SubjectType::User,
            Some(json!({
                "secret": "must-not-propagate",
                "email": "private@example.test",
                "quota": "large"
            })),
        );
        subject.org_id = Some("01JZ000000000000000000000Z".to_owned());
        subject.org_role = Some("admin".to_owned());
        let identity = VerifiedIdentity::test_jwt(
            subject,
            "01JZ000000000000000000000B".to_owned(),
            Some(1_784_473_000),
        );

        let draft = PropagationDraft::build(
            &identity,
            &request_context(Some("example-client/1.0".to_owned())),
            "POST",
            "/v1/orders",
            "/api/orders",
            "orders-write",
            "orders",
            Some("sha256:revision".to_owned()),
        )
        .unwrap();

        assert_eq!(draft.subject.as_deref(), Some("01JZ000000000000000000000A"));
        assert_eq!(draft.actor.actor_type, ActorType::User);
        assert_eq!(draft.authentication.kind, AuthenticationKind::Jwt);
        assert_eq!(
            draft.authentication.sid.as_deref(),
            Some("01JZ000000000000000000000B")
        );
        assert_eq!(draft.authentication.auth_time, Some(1_784_473_000));
        assert_eq!(
            draft.organization.as_ref().unwrap().id,
            "01JZ000000000000000000000Z"
        );
        assert_eq!(
            draft.organization.as_ref().unwrap().role.as_deref(),
            Some("admin")
        );
        assert_eq!(draft.request.id, "01JZ000000000000000000000R");
        assert_eq!(draft.request.client_ip.as_deref(), Some("203.0.113.10"));
        assert_eq!(
            draft.request.trace_id.as_deref(),
            Some("4bf92f3577b34da6a3ce929d0e0e4736")
        );
        assert_eq!(draft.request.path, "/v1/orders");
        assert_eq!(draft.request.original_path, "/api/orders");

        let visible = serde_json::to_string(&(
            draft.subject.as_deref(),
            &draft.actor,
            &draft.authentication,
            draft.organization.as_ref(),
            &draft.request,
            &draft.route,
        ))
        .unwrap();
        assert!(!visible.contains("must-not-propagate"));
        assert!(!visible.contains("private@example.test"));
        assert!(!visible.contains("quota"));
        assert_eq!(format!("{draft:?}"), "PropagationDraft([redacted])");
    }

    #[test]
    fn issuance_binds_leaf_dispatch_and_the_exact_final_path() {
        let draft = PropagationDraft::build(
            &VerifiedIdentity::anonymous(),
            &request_context(None),
            "POST",
            "/orders",
            "/api/orders",
            "orders-write",
            "orders",
            None,
        )
        .unwrap();

        let issued = draft
            .issue_request(
                "urn:stargate:service:orders",
                DispatchKind::Primary,
                1,
                "POST",
                "/internal/v1/orders",
            )
            .unwrap();

        assert_eq!(issued.audience, "urn:stargate:service:orders");
        assert_eq!(issued.context.request.path, "/internal/v1/orders");
        assert_eq!(issued.context.request.original_path, "/api/orders");
        assert_eq!(issued.context.dispatch.kind, DispatchKind::Primary);
        assert_eq!(issued.context.dispatch.attempt, 1);
        assert!(issued.subject.is_none());
        assert!(
            draft
                .issue_request(
                    "urn:stargate:service:orders",
                    DispatchKind::Primary,
                    1,
                    "GET",
                    "/internal/v1/orders",
                )
                .is_err()
        );
    }

    #[test]
    fn api_key_and_anonymous_shapes_are_explicit() {
        let mut subject = Subject::new(
            "01JZ000000000000000000000K".to_owned(),
            SubjectType::ApiKey,
            None,
        );
        subject.org_id = Some("01JZ000000000000000000000Z".to_owned());
        let api_key = VerifiedIdentity::test_api_key(subject);
        let api_key_draft = PropagationDraft::build(
            &api_key,
            &request_context(None),
            "GET",
            "/orders",
            "/orders",
            "orders-read",
            "orders",
            None,
        )
        .unwrap();
        assert_eq!(api_key_draft.actor.actor_type, ActorType::ApiKey);
        assert_eq!(
            api_key_draft.authentication.kind,
            AuthenticationKind::ApiKey
        );
        assert!(api_key_draft.authentication.sid.is_none());

        let anonymous = PropagationDraft::build(
            &VerifiedIdentity::anonymous(),
            &request_context(None),
            "GET",
            "/public",
            "/public",
            "public",
            "public-service",
            None,
        )
        .unwrap();
        assert_eq!(anonymous.actor.actor_type, ActorType::Anonymous);
        assert_eq!(anonymous.authentication.kind, AuthenticationKind::None);
        assert!(anonymous.subject.is_none());
        assert!(anonymous.organization.is_none());
    }

    #[test]
    fn ingress_sanitizer_removes_all_spoofed_context_and_replaces_request_id() {
        let mut headers = HeaderMap::new();
        headers.append(
            INTERNAL_CONTEXT_HEADER.clone(),
            HeaderValue::from_static("attacker-one"),
        );
        headers.append(
            INTERNAL_CONTEXT_HEADER.clone(),
            HeaderValue::from_static("attacker-two"),
        );
        headers.append(
            REQUEST_ID_HEADER.clone(),
            HeaderValue::from_static("attacker-request-one"),
        );
        headers.append(
            REQUEST_ID_HEADER.clone(),
            HeaderValue::from_static("attacker-request-two"),
        );

        sanitize_ingress_headers(&mut headers, "01JZ000000000000000000000R").unwrap();

        assert!(
            headers
                .get_all(&INTERNAL_CONTEXT_HEADER)
                .iter()
                .next()
                .is_none()
        );
        assert_eq!(
            headers
                .get_all(&REQUEST_ID_HEADER)
                .iter()
                .collect::<Vec<_>>(),
            vec!["01JZ000000000000000000000R"]
        );
    }

    #[test]
    fn request_payload_query_cookie_and_raw_headers_cannot_supply_draft_facts() {
        let attacker_body = json!({
            "actor": "user",
            "organization": "attacker-org",
            "role": "owner",
            "request_id": "attacker-request",
            "client_ip": "198.51.100.99",
            "trace_id": "ffffffffffffffffffffffffffffffff"
        });
        let attacker_query = "actor=user&organization=attacker-org&request_id=attacker-request";
        let attacker_cookie = "actor=user; organization=attacker-org; role=owner";
        let attacker_header = "attacker-signed-context";

        let draft = PropagationDraft::build(
            &VerifiedIdentity::anonymous(),
            &request_context(None),
            "GET",
            "/public",
            "/public",
            "public",
            "public-service",
            Some("sha256:trusted".to_owned()),
        )
        .unwrap();
        let visible = serde_json::to_string(&(
            draft.subject.as_deref(),
            &draft.actor,
            &draft.authentication,
            draft.organization.as_ref(),
            &draft.request,
            &draft.route,
        ))
        .unwrap();

        for untrusted in [
            attacker_body.to_string(),
            attacker_query.to_owned(),
            attacker_cookie.to_owned(),
            attacker_header.to_owned(),
            "attacker-org".to_owned(),
            "attacker-request".to_owned(),
            "198.51.100.99".to_owned(),
            "ffffffffffffffffffffffffffffffff".to_owned(),
        ] {
            assert!(!visible.contains(&untrusted));
        }
        assert_eq!(draft.actor.actor_type, ActorType::Anonymous);
        assert_eq!(draft.request.id, "01JZ000000000000000000000R");
        assert_eq!(draft.request.client_ip.as_deref(), Some("203.0.113.10"));
    }

    #[test]
    fn draft_user_agent_is_truncated_on_a_utf8_boundary() {
        let context = request_context(Some(format!("{}💫", "a".repeat(511))));
        let draft = PropagationDraft::build(
            &VerifiedIdentity::anonymous(),
            &context,
            "GET",
            "/public",
            "/public",
            "public",
            "public-service",
            None,
        )
        .unwrap();
        let user_agent = draft.request.user_agent.as_deref().unwrap();

        assert_eq!(user_agent.len(), 511);
        assert!(user_agent.is_char_boundary(user_agent.len()));
        assert!(!user_agent.contains('💫'));
        assert!(context.user_agent().unwrap().contains('💫'));
    }
}
