use super::RequestContext;
use crate::etc::auth::{
    identity::{AuthKind, VerifiedIdentity},
    subject::SubjectType,
};
use ctx::{
    Actor, ActorType, Authentication, AuthenticationKind, DispatchContext, DispatchKind,
    IssueRequest, MAX_METHOD_BYTES, MAX_NAME_BYTES, MAX_PATH_BYTES, MAX_REQUEST_ID_BYTES,
    MAX_ROLE_BYTES, MAX_USER_AGENT_BYTES, Organization, RequestContext as PropagatedRequestContext,
    RouteContext, StargateContext, VERSION, truncate_utf8,
};
use ulid::Ulid;

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
    pub fn build(
        identity: &VerifiedIdentity,
        request_context: &RequestContext,
        method: &str,
        path: &str,
        original_path: &str,
        route: RouteContext,
    ) -> Result<Self, PropagationDraftError> {
        let (subject, actor, authentication, organization) = identity_facts(identity)?;
        validate_request_facts(request_context, method, path, original_path)?;
        validate_route_facts(
            &route.router,
            &route.service,
            route.policy_revision.as_deref(),
        )?;

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
            route,
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
        (AuthKind::Jwt | AuthKind::OAuth, SubjectType::User) => {
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

#[cfg(test)]
mod tests;
