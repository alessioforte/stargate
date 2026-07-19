use std::fmt;

use chrono::{DateTime, Utc};

use super::{
    AuditActor, AuditId, AuditRequestContext, AuditScopeSelector, AuditService,
    RawAuditEventBuilder,
};

/// Failure to establish an audit actor or scope at a trusted application
/// boundary.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TrustedAuditContextError {
    message: String,
}

impl TrustedAuditContextError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TrustedAuditContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TrustedAuditContextError {}

/// Scope selected by trusted route, ownership, or background-job code.
///
/// This type deliberately has no `Default` implementation and does not
/// deserialize from request data. Organization scope can only be constructed
/// from a persisted organization id, with an optional request assertion used
/// solely as a consistency check.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TrustedAuditBoundary {
    scope: AuditScopeSelector,
}

impl TrustedAuditBoundary {
    pub fn application() -> Self {
        Self {
            scope: AuditScopeSelector::application(),
        }
    }

    pub fn control_plane() -> Self {
        Self {
            scope: AuditScopeSelector::control_plane(),
        }
    }

    pub fn organization_from_persisted(
        persisted_organization_id: Option<&str>,
        asserted_organization_id: Option<&str>,
    ) -> Result<Self, TrustedAuditContextError> {
        let persisted_organization_id = persisted_organization_id.ok_or_else(|| {
            TrustedAuditContextError::new(
                "persisted organization ownership is required for organization audit scope",
            )
        })?;

        AuditId::parse(persisted_organization_id.to_string()).map_err(|_| {
            TrustedAuditContextError::new(
                "persisted organization id must be an uppercase ULID for organization audit scope",
            )
        })?;

        if let Some(asserted_organization_id) = asserted_organization_id
            && asserted_organization_id != persisted_organization_id
        {
            return Err(TrustedAuditContextError::new(format!(
                "asserted organization id does not match persisted owner {persisted_organization_id}"
            )));
        }

        Ok(Self {
            scope: AuditScopeSelector::organization(persisted_organization_id),
        })
    }

    pub fn scope(&self) -> &AuditScopeSelector {
        &self.scope
    }
}

/// Actor derived from authenticated or otherwise verified application state.
///
/// There is no deserialization or general-purpose setter, so actor identity is
/// carried from code that already established the principal.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TrustedAuditActor {
    actor_type: &'static str,
    id: Option<String>,
}

impl TrustedAuditActor {
    fn identified(actor_type: &'static str, id: impl Into<String>) -> Self {
        Self {
            actor_type,
            id: Some(id.into()),
        }
    }

    pub fn admin(id: impl Into<String>) -> Self {
        Self::identified("admin", id)
    }

    pub fn admin_key(id: impl Into<String>) -> Self {
        Self::identified("admin_key", id)
    }

    pub fn user(id: impl Into<String>) -> Self {
        Self::identified("user", id)
    }

    pub fn api_key(id: impl Into<String>) -> Self {
        Self::identified("api_key", id)
    }

    pub fn external_identity(id: impl Into<String>) -> Self {
        Self::identified("external_identity", id)
    }

    pub fn service(id: impl Into<String>) -> Self {
        Self::identified("service", id)
    }

    pub fn system(id: Option<String>) -> Self {
        Self {
            actor_type: "system",
            id,
        }
    }

    pub fn anonymous() -> Self {
        Self {
            actor_type: "anonymous",
            id: None,
        }
    }

    pub fn actor_type(&self) -> &str {
        self.actor_type
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn raw_actor(&self) -> AuditActor {
        AuditActor {
            actor_type: self.actor_type.to_string(),
            id: self.id.clone(),
            email: None,
        }
    }
}

/// Principal accepted by the `/admin/*` middleware. Its conversion path
/// always constructs a control-plane context.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TrustedAdminActor {
    Admin { user_id: String },
    AdminKey { key_id: String },
}

impl TrustedAdminActor {
    pub fn admin(user_id: impl Into<String>) -> Self {
        Self::Admin {
            user_id: user_id.into(),
        }
    }

    pub fn admin_key(key_id: impl Into<String>) -> Self {
        Self::AdminKey {
            key_id: key_id.into(),
        }
    }

    pub fn audit_actor(&self) -> TrustedAuditActor {
        self.clone().into_audit_actor()
    }

    fn into_audit_actor(self) -> TrustedAuditActor {
        match self {
            Self::Admin { user_id } => TrustedAuditActor::admin(user_id),
            Self::AdminKey { key_id } => TrustedAuditActor::admin_key(key_id),
        }
    }
}

/// Principal accepted by background audit construction. This prevents a job
/// from accidentally using an HTTP-only actor such as `anonymous` or `admin`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TrustedBackgroundActor {
    System { job_id: Option<String> },
    Service { service_id: String },
}

impl TrustedBackgroundActor {
    pub fn system(job_id: Option<String>) -> Self {
        Self::System { job_id }
    }

    pub fn service(service_id: impl Into<String>) -> Self {
        Self::Service {
            service_id: service_id.into(),
        }
    }

    fn into_audit_actor(self) -> TrustedAuditActor {
        match self {
            Self::System { job_id } => TrustedAuditActor::system(job_id),
            Self::Service { service_id } => TrustedAuditActor::service(service_id),
        }
    }
}

/// Request facts collected by Stargate infrastructure. These values are
/// diagnostic only and cannot select the actor or scope.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TrustedAuditRequest {
    request: AuditRequestContext,
}

impl TrustedAuditRequest {
    pub fn from_http(
        request_id: impl Into<String>,
        trace_id: Option<String>,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> Self {
        Self {
            request: AuditRequestContext {
                request_id: request_id.into(),
                trace_id,
                ip_address,
                user_agent,
            },
        }
    }

    pub fn raw_request(&self) -> &AuditRequestContext {
        &self.request
    }
}

/// Actor, scope, and request facts established before an audit producer runs.
///
/// Public construction paths encode the security boundary: admin requests are
/// always control-plane, ordinary HTTP requests select application or a
/// persisted organization, and background work must provide both a boundary
/// and an allowed background actor.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TrustedAuditContext {
    boundary: TrustedAuditBoundary,
    actor: TrustedAuditActor,
    request: Option<TrustedAuditRequest>,
}

impl TrustedAuditContext {
    pub fn admin_control_plane(actor: TrustedAdminActor, request: TrustedAuditRequest) -> Self {
        Self {
            boundary: TrustedAuditBoundary::control_plane(),
            actor: actor.into_audit_actor(),
            request: Some(request),
        }
    }

    pub fn application(actor: TrustedAuditActor, request: TrustedAuditRequest) -> Self {
        Self {
            boundary: TrustedAuditBoundary::application(),
            actor,
            request: Some(request),
        }
    }

    pub fn organization(
        actor: TrustedAuditActor,
        request: TrustedAuditRequest,
        persisted_organization_id: Option<&str>,
        asserted_organization_id: Option<&str>,
    ) -> Result<Self, TrustedAuditContextError> {
        Ok(Self {
            boundary: TrustedAuditBoundary::organization_from_persisted(
                persisted_organization_id,
                asserted_organization_id,
            )?,
            actor,
            request: Some(request),
        })
    }

    pub fn background(boundary: TrustedAuditBoundary, actor: TrustedBackgroundActor) -> Self {
        Self {
            boundary,
            actor: actor.into_audit_actor(),
            request: None,
        }
    }

    pub fn boundary(&self) -> &TrustedAuditBoundary {
        &self.boundary
    }

    pub fn actor(&self) -> &TrustedAuditActor {
        &self.actor
    }

    pub fn request(&self) -> Option<&TrustedAuditRequest> {
        self.request.as_ref()
    }

    /// Seed a raw-event builder exclusively from the trusted actor, scope, and
    /// request facts. Producers still provide their semantic action, resource,
    /// safe snapshots, and metadata.
    pub fn raw_event_builder(&self, occurred_at: DateTime<Utc>) -> RawAuditEventBuilder {
        let mut builder = RawAuditEventBuilder::new()
            .event_id(AuditId::new())
            .occurred_at(occurred_at)
            .scope(self.boundary.scope.clone())
            .actor(self.actor.raw_actor())
            .service(AuditService::stargate());
        if let Some(request) = &self.request {
            builder = builder.request(request.request.clone());
        }
        builder
    }
}

#[cfg(test)]
mod trusted_context_tests {
    use super::*;

    const ORGANIZATION_ID: &str = "01JZ0000000000000000000001";
    const OTHER_ORGANIZATION_ID: &str = "01JZ000000000000000000000X";

    fn request() -> TrustedAuditRequest {
        TrustedAuditRequest::from_http(
            "01JZ000000000000000000000R",
            Some("4bf92f3577b34da6a3ce929d0e0e4736".to_string()),
            Some("192.0.2.10".to_string()),
            Some("test-agent".to_string()),
        )
    }

    #[test]
    fn admin_constructor_is_always_control_plane() {
        let context = TrustedAuditContext::admin_control_plane(
            TrustedAdminActor::admin("01JZ000000000000000000000A"),
            request(),
        );

        assert_eq!(
            context.boundary().scope(),
            &AuditScopeSelector::ControlPlane
        );
        assert_eq!(context.actor().actor_type(), "admin");
        assert_eq!(context.actor().id(), Some("01JZ000000000000000000000A"));
        assert_eq!(
            context
                .request()
                .expect("HTTP context must have request facts")
                .raw_request()
                .ip_address
                .as_deref(),
            Some("192.0.2.10")
        );
    }

    #[test]
    fn organization_context_requires_persisted_ownership() {
        let error = TrustedAuditContext::organization(
            TrustedAuditActor::user("01JZ000000000000000000000U"),
            request(),
            None,
            None,
        )
        .expect_err("missing persisted ownership must fail");

        assert!(error.to_string().contains("persisted organization"));
    }

    #[test]
    fn organization_context_rejects_request_owner_mismatch() {
        let error = TrustedAuditContext::organization(
            TrustedAuditActor::user("01JZ000000000000000000000U"),
            request(),
            Some(ORGANIZATION_ID),
            Some(OTHER_ORGANIZATION_ID),
        )
        .expect_err("a request assertion cannot replace persisted ownership");

        assert!(error.to_string().contains("does not match persisted owner"));
    }

    #[test]
    fn organization_context_uses_exact_persisted_owner() {
        let context = TrustedAuditContext::organization(
            TrustedAuditActor::service("oauth-client"),
            request(),
            Some(ORGANIZATION_ID),
            None,
        )
        .expect("persisted ownership should establish scope");

        assert_eq!(
            context.boundary().scope(),
            &AuditScopeSelector::Organization {
                organization_id: ORGANIZATION_ID.to_string(),
            }
        );
    }

    #[test]
    fn background_context_requires_explicit_boundary_and_actor_class() {
        let application = TrustedAuditContext::background(
            TrustedAuditBoundary::application(),
            TrustedBackgroundActor::system(Some("credential-rehash".to_string())),
        );
        let organization = TrustedAuditContext::background(
            TrustedAuditBoundary::organization_from_persisted(Some(ORGANIZATION_ID), None)
                .expect("persisted owner"),
            TrustedBackgroundActor::service("provisioner"),
        );

        assert_eq!(
            application.boundary().scope(),
            &AuditScopeSelector::Application
        );
        assert_eq!(application.actor().actor_type(), "system");
        assert!(application.request().is_none());
        assert_eq!(organization.actor().actor_type(), "service");
        assert!(matches!(
            organization.boundary().scope(),
            AuditScopeSelector::Organization { organization_id }
                if organization_id == ORGANIZATION_ID
        ));
    }
}
