use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

pub const AUDIT_EVENT_VERSION: i32 = 1;

const MAX_EVENT_BYTES: usize = 65_536;
const MAX_BEFORE_AFTER_BYTES: usize = 16_384;
const MAX_METADATA_BYTES: usize = 8_192;
const MAX_DYNAMIC_JSON_DEPTH: usize = 8;
const MAX_SNAPSHOT_STRING_BYTES: usize = 2_048;
const MAX_SNAPSHOT_ARRAY_ITEMS: usize = 64;
const MAX_TYPE_OR_ACTION_BYTES: usize = 128;
const MAX_CUSTOM_OPERATION_BYTES: usize = 64;
const MAX_IDENTIFIER_BYTES: usize = 255;
const MAX_EMAIL_BYTES: usize = 320;
const MAX_RESOURCE_NAME_BYTES: usize = 255;
const MAX_IP_ADDRESS_BYTES: usize = 64;
const MAX_USER_AGENT_BYTES: usize = 512;

const ALLOWED_ACTOR_TYPES: &[&str] = &[
    "admin",
    "admin_key",
    "anonymous",
    "api_key",
    "external_identity",
    "service",
    "system",
    "user",
];

const ALLOWED_RESOURCE_TYPES: &[&str] = &[
    "admin_key",
    "api_key",
    "credential",
    "oauth_client",
    "organization",
    "organization_membership",
    "service_account",
    "super_admin",
    "user",
];

const ALLOWED_ACTIONS: &[(&str, &str)] = &[
    ("admin_key.created", "create"),
    ("admin_key.deleted", "delete"),
    ("admin_key.permissions_updated", "update"),
    ("admin_key.revoked", "update"),
    ("api_key.created", "create"),
    ("api_key.deleted", "delete"),
    ("api_key.revoked", "update"),
    ("api_key.updated", "update"),
    ("credential.password_changed", "update"),
    ("credential.rehashed", "update"),
    ("oauth_client.created", "create"),
    ("oauth_client.deleted", "delete"),
    ("oauth_client.disabled", "update"),
    ("oauth_client.enabled", "update"),
    ("oauth_client.secret_rotated", "update"),
    ("oauth_client.updated", "update"),
    ("organization.created", "create"),
    ("organization.deleted", "delete"),
    ("organization.member_added", "create"),
    ("organization.member_removed", "delete"),
    ("organization.updated", "update"),
    ("service_account.created", "create"),
    ("service_account.deleted", "delete"),
    ("service_account.updated", "update"),
    ("super_admin.granted", "create"),
    ("user.created", "create"),
    ("user.deleted", "delete"),
    ("user.updated", "update"),
];

pub type AuditEventResult<T> = Result<T, AuditEventError>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AuditEventError {
    message: String,
}

impl AuditEventError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AuditEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AuditEventError {}

#[derive(Debug, Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AuditId(String);

impl AuditId {
    pub fn new() -> Self {
        Self(ulid::Ulid::generate().to_string())
    }

    pub fn parse(value: impl Into<String>) -> AuditEventResult<Self> {
        let value = value.into();
        validate_ulid(&value, "audit id")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for AuditId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AuditId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for AuditId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl FromStr for AuditId {
    type Err = AuditEventError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for AuditId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuditScopeSelector {
    Application,
    Organization { organization_id: String },
    ControlPlane,
}

impl AuditScopeSelector {
    pub fn application() -> Self {
        Self::Application
    }

    pub fn organization(organization_id: impl Into<String>) -> Self {
        Self::Organization {
            organization_id: organization_id.into(),
        }
    }

    pub fn control_plane() -> Self {
        Self::ControlPlane
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditActor {
    #[serde(rename = "type")]
    pub actor_type: String,
    pub id: Option<String>,
    pub email: Option<String>,
}

impl AuditActor {
    pub fn new(actor_type: impl Into<String>) -> Self {
        Self {
            actor_type: actor_type.into(),
            id: None,
            email: None,
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditService {
    pub name: String,
}

impl AuditService {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn stargate() -> Self {
        Self::new("stargate")
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditResource {
    #[serde(rename = "type")]
    pub resource_type: String,
    pub id: String,
    pub name: Option<String>,
}

impl AuditResource {
    pub fn new(resource_type: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            id: id.into(),
            name: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AuditOperation {
    Create,
    Update,
    Delete,
    Restore,
    Archive,
    Custom(String),
}

impl AuditOperation {
    fn as_str(&self) -> &str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Restore => "restore",
            Self::Archive => "archive",
            Self::Custom(value) => value,
        }
    }
}

impl Serialize for AuditOperation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AuditOperation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let operation = if value.eq_ignore_ascii_case("create") {
            Self::Create
        } else if value.eq_ignore_ascii_case("update") {
            Self::Update
        } else if value.eq_ignore_ascii_case("delete") {
            Self::Delete
        } else if value.eq_ignore_ascii_case("restore") {
            Self::Restore
        } else if value.eq_ignore_ascii_case("archive") {
            Self::Archive
        } else {
            Self::Custom(value)
        };
        Ok(operation)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuditRequestContext {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

impl AuditRequestContext {
    pub fn new(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            trace_id: None,
            ip_address: None,
            user_agent: None,
        }
    }

    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    pub fn with_ip_address(mut self, ip_address: impl Into<String>) -> Self {
        self.ip_address = Some(ip_address.into());
        self
    }

    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawAuditEvent {
    pub event_id: AuditId,
    pub event_version: i32,
    pub occurred_at: DateTime<Utc>,
    pub scope: AuditScopeSelector,
    pub operation_id: Option<AuditId>,
    pub actor: AuditActor,
    pub service: AuditService,
    pub resource: AuditResource,
    pub action: String,
    pub operation: AuditOperation,
    pub before: Option<Value>,
    pub after: Option<Value>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    pub request: Option<AuditRequestContext>,
}

impl RawAuditEvent {
    pub fn validate_and_serialize(mut self) -> AuditEventResult<ValidatedAuditEvent> {
        self.normalize_and_validate()?;

        let payload = serde_json::to_string(&self).map_err(|error| {
            AuditEventError::new(format!("failed to serialize raw audit event: {error}"))
        })?;
        if payload.len() > MAX_EVENT_BYTES {
            return Err(AuditEventError::new(format!(
                "raw audit event exceeds {MAX_EVENT_BYTES} serialized bytes"
            )));
        }

        Ok(ValidatedAuditEvent {
            event: self,
            payload,
        })
    }

    fn normalize_and_validate(&mut self) -> AuditEventResult<()> {
        if self.event_version != AUDIT_EVENT_VERSION {
            return Err(AuditEventError::new(format!(
                "event_version must be {AUDIT_EVENT_VERSION}, got {}",
                self.event_version
            )));
        }

        normalize_scope(&mut self.scope)?;
        normalize_actor(&mut self.actor)?;
        normalize_required(
            &mut self.service.name,
            "service.name",
            MAX_TYPE_OR_ACTION_BYTES,
        )?;
        normalize_resource(&mut self.resource)?;
        normalize_required(&mut self.action, "action", MAX_TYPE_OR_ACTION_BYTES)?;
        normalize_operation(&mut self.operation)?;
        validate_action_and_operation(&self.action, &self.operation)?;

        normalize_nullable_json(&mut self.before);
        normalize_nullable_json(&mut self.after);
        validate_snapshot(self.before.as_ref(), "before")?;
        validate_snapshot(self.after.as_ref(), "after")?;
        validate_metadata(&self.metadata)?;

        if let Some(request) = self.request.as_mut() {
            normalize_request(request)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedAuditEvent {
    event: RawAuditEvent,
    payload: String,
}

impl ValidatedAuditEvent {
    pub fn event(&self) -> &RawAuditEvent {
        &self.event
    }

    pub fn event_id(&self) -> &AuditId {
        &self.event.event_id
    }

    pub fn operation_id(&self) -> Option<&AuditId> {
        self.event.operation_id.as_ref()
    }

    pub fn payload(&self) -> &str {
        &self.payload
    }

    pub fn into_parts(self) -> (RawAuditEvent, String) {
        (self.event, self.payload)
    }
}

#[derive(Debug)]
pub struct RawAuditEventBuilder {
    event_id: Option<AuditId>,
    event_version: i32,
    occurred_at: Option<DateTime<Utc>>,
    scope: Option<AuditScopeSelector>,
    operation_id: Option<AuditId>,
    actor: Option<AuditActor>,
    service: Option<AuditService>,
    resource: Option<AuditResource>,
    action: Option<String>,
    operation: Option<AuditOperation>,
    before: Option<Value>,
    after: Option<Value>,
    metadata: Option<Value>,
    request: Option<AuditRequestContext>,
}

impl Default for RawAuditEventBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl RawAuditEventBuilder {
    pub fn new() -> Self {
        Self {
            event_id: None,
            event_version: AUDIT_EVENT_VERSION,
            occurred_at: None,
            scope: None,
            operation_id: None,
            actor: None,
            service: None,
            resource: None,
            action: None,
            operation: None,
            before: None,
            after: None,
            metadata: None,
            request: None,
        }
    }

    pub fn event_id(mut self, event_id: AuditId) -> Self {
        self.event_id = Some(event_id);
        self
    }

    pub fn event_version(mut self, event_version: i32) -> Self {
        self.event_version = event_version;
        self
    }

    pub fn occurred_at(mut self, occurred_at: DateTime<Utc>) -> Self {
        self.occurred_at = Some(occurred_at);
        self
    }

    pub fn scope(mut self, scope: AuditScopeSelector) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn operation_id(mut self, operation_id: AuditId) -> Self {
        self.operation_id = Some(operation_id);
        self
    }

    pub fn actor(mut self, actor: AuditActor) -> Self {
        self.actor = Some(actor);
        self
    }

    pub fn service(mut self, service: AuditService) -> Self {
        self.service = Some(service);
        self
    }

    pub fn resource(mut self, resource: AuditResource) -> Self {
        self.resource = Some(resource);
        self
    }

    pub fn action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }

    pub fn operation(mut self, operation: AuditOperation) -> Self {
        self.operation = Some(operation);
        self
    }

    pub fn before(mut self, before: Value) -> Self {
        self.before = Some(before);
        self
    }

    pub fn after(mut self, after: Value) -> Self {
        self.after = Some(after);
        self
    }

    pub fn metadata(mut self, metadata: Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub fn request(mut self, request: AuditRequestContext) -> Self {
        self.request = Some(request);
        self
    }

    pub fn build(self) -> AuditEventResult<ValidatedAuditEvent> {
        let metadata = match self.metadata.unwrap_or_else(|| Value::Object(Map::new())) {
            Value::Object(metadata) => metadata,
            _ => {
                return Err(AuditEventError::new(
                    "metadata must be a JSON object when present",
                ));
            }
        };

        RawAuditEvent {
            event_id: required(self.event_id, "event_id")?,
            event_version: self.event_version,
            occurred_at: required(self.occurred_at, "occurred_at")?,
            scope: required(self.scope, "scope")?,
            operation_id: self.operation_id,
            actor: required(self.actor, "actor")?,
            service: required(self.service, "service")?,
            resource: required(self.resource, "resource")?,
            action: required(self.action, "action")?,
            operation: required(self.operation, "operation")?,
            before: self.before,
            after: self.after,
            metadata,
            request: self.request,
        }
        .validate_and_serialize()
    }
}

fn required<T>(value: Option<T>, field: &str) -> AuditEventResult<T> {
    value.ok_or_else(|| AuditEventError::new(format!("{field} is required")))
}

fn validate_ulid(value: &str, field: &str) -> AuditEventResult<()> {
    let has_valid_bytes = value.len() == 26
        && value.bytes().all(|byte| {
            byte.is_ascii_digit()
                || matches!(
                    byte,
                    b'A'..=b'H' | b'J'..=b'K' | b'M'..=b'N' | b'P'..=b'T' | b'V'..=b'Z'
                )
        });
    if !has_valid_bytes || ulid::Ulid::from_string(value).is_err() {
        return Err(AuditEventError::new(format!(
            "{field} must be an uppercase 26-character Crockford Base32 ULID"
        )));
    }
    Ok(())
}

fn normalize_scope(scope: &mut AuditScopeSelector) -> AuditEventResult<()> {
    if let AuditScopeSelector::Organization { organization_id } = scope {
        normalize_required(
            organization_id,
            "scope.organization_id",
            MAX_IDENTIFIER_BYTES,
        )?;
        validate_ulid(organization_id, "scope.organization_id")?;
    }
    Ok(())
}

fn normalize_actor(actor: &mut AuditActor) -> AuditEventResult<()> {
    normalize_required(
        &mut actor.actor_type,
        "actor.type",
        MAX_TYPE_OR_ACTION_BYTES,
    )?;
    validate_allowed(&actor.actor_type, "actor.type", ALLOWED_ACTOR_TYPES)?;
    normalize_optional(&mut actor.id, "actor.id", MAX_IDENTIFIER_BYTES)?;
    normalize_optional(&mut actor.email, "actor.email", MAX_EMAIL_BYTES)?;
    if actor.email.is_some() {
        return Err(AuditEventError::new(
            "actor.email must be null in Stargate event version 1",
        ));
    }
    Ok(())
}

fn normalize_resource(resource: &mut AuditResource) -> AuditEventResult<()> {
    normalize_required(
        &mut resource.resource_type,
        "resource.type",
        MAX_TYPE_OR_ACTION_BYTES,
    )?;
    validate_allowed(
        &resource.resource_type,
        "resource.type",
        ALLOWED_RESOURCE_TYPES,
    )?;
    normalize_required(&mut resource.id, "resource.id", MAX_IDENTIFIER_BYTES)?;
    normalize_optional(&mut resource.name, "resource.name", MAX_RESOURCE_NAME_BYTES)
}

fn normalize_operation(operation: &mut AuditOperation) -> AuditEventResult<()> {
    if let AuditOperation::Custom(value) = operation {
        normalize_required(value, "operation", MAX_CUSTOM_OPERATION_BYTES)?;
        if value.to_lowercase() != *value {
            return Err(AuditEventError::new("custom operation must be lowercase"));
        }
    }
    Ok(())
}

fn validate_action_and_operation(action: &str, operation: &AuditOperation) -> AuditEventResult<()> {
    let Some((_, expected_operation)) = ALLOWED_ACTIONS
        .iter()
        .find(|(allowed_action, _)| *allowed_action == action)
    else {
        return Err(AuditEventError::new(format!(
            "action '{action}' is not approved for Stargate event version 1"
        )));
    };

    if operation.as_str() != *expected_operation {
        return Err(AuditEventError::new(format!(
            "action '{action}' requires operation '{expected_operation}', got '{}'",
            operation.as_str()
        )));
    }
    Ok(())
}

fn normalize_request(request: &mut AuditRequestContext) -> AuditEventResult<()> {
    normalize_required(
        &mut request.request_id,
        "request.request_id",
        MAX_IDENTIFIER_BYTES,
    )?;
    normalize_optional(
        &mut request.trace_id,
        "request.trace_id",
        MAX_IDENTIFIER_BYTES,
    )?;
    normalize_optional(
        &mut request.ip_address,
        "request.ip_address",
        MAX_IP_ADDRESS_BYTES,
    )?;
    normalize_optional(
        &mut request.user_agent,
        "request.user_agent",
        MAX_USER_AGENT_BYTES,
    )
}

fn normalize_required(value: &mut String, field: &str, maximum: usize) -> AuditEventResult<()> {
    *value = value.trim().to_owned();
    if value.is_empty() {
        return Err(AuditEventError::new(format!("{field} must not be blank")));
    }
    validate_text(value, field, maximum)
}

fn normalize_optional(
    value: &mut Option<String>,
    field: &str,
    maximum: usize,
) -> AuditEventResult<()> {
    let Some(current) = value.take() else {
        return Ok(());
    };
    let current = current.trim().to_owned();
    if current.is_empty() {
        return Ok(());
    }
    validate_text(&current, field, maximum)?;
    *value = Some(current);
    Ok(())
}

fn validate_text(value: &str, field: &str, maximum: usize) -> AuditEventResult<()> {
    if value.len() > maximum {
        return Err(AuditEventError::new(format!(
            "{field} exceeds {maximum} UTF-8 bytes"
        )));
    }
    if value.chars().any(|character| character.is_ascii_control()) {
        return Err(AuditEventError::new(format!(
            "{field} must not contain ASCII control characters"
        )));
    }
    Ok(())
}

fn validate_allowed(value: &str, field: &str, allowed: &[&str]) -> AuditEventResult<()> {
    if !allowed.contains(&value) {
        return Err(AuditEventError::new(format!(
            "{field} '{value}' is not approved for Stargate event version 1"
        )));
    }
    Ok(())
}

fn normalize_nullable_json(value: &mut Option<Value>) {
    if matches!(value, Some(Value::Null)) {
        *value = None;
    }
}

fn validate_snapshot(value: Option<&Value>, field: &str) -> AuditEventResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if !value.is_object() {
        return Err(AuditEventError::new(format!(
            "{field} must be a JSON object or null"
        )));
    }
    validate_dynamic_json(value, field, 1, true)?;
    validate_serialized_size(value, field, MAX_BEFORE_AFTER_BYTES)
}

fn validate_metadata(metadata: &Map<String, Value>) -> AuditEventResult<()> {
    let value = Value::Object(metadata.clone());
    validate_dynamic_json(&value, "metadata", 1, false)?;
    validate_serialized_size(&value, "metadata", MAX_METADATA_BYTES)
}

fn validate_dynamic_json(
    value: &Value,
    field: &str,
    depth: usize,
    enforce_snapshot_value_limits: bool,
) -> AuditEventResult<()> {
    match value {
        Value::Object(object) => {
            validate_json_depth(field, depth)?;
            for child in object.values() {
                validate_dynamic_json(child, field, depth + 1, enforce_snapshot_value_limits)?;
            }
        }
        Value::Array(array) => {
            validate_json_depth(field, depth)?;
            if enforce_snapshot_value_limits && array.len() > MAX_SNAPSHOT_ARRAY_ITEMS {
                return Err(AuditEventError::new(format!(
                    "{field} contains an array with more than {MAX_SNAPSHOT_ARRAY_ITEMS} elements"
                )));
            }
            for child in array {
                validate_dynamic_json(child, field, depth + 1, enforce_snapshot_value_limits)?;
            }
        }
        Value::String(string)
            if enforce_snapshot_value_limits && string.len() > MAX_SNAPSHOT_STRING_BYTES =>
        {
            return Err(AuditEventError::new(format!(
                "{field} contains a string exceeding {MAX_SNAPSHOT_STRING_BYTES} UTF-8 bytes"
            )));
        }
        _ => {}
    }
    Ok(())
}

fn validate_json_depth(field: &str, depth: usize) -> AuditEventResult<()> {
    if depth > MAX_DYNAMIC_JSON_DEPTH {
        return Err(AuditEventError::new(format!(
            "{field} exceeds {MAX_DYNAMIC_JSON_DEPTH} JSON nesting levels"
        )));
    }
    Ok(())
}

fn validate_serialized_size(value: &Value, field: &str, maximum: usize) -> AuditEventResult<()> {
    let size = serde_json::to_vec(value)
        .map_err(|error| AuditEventError::new(format!("failed to serialize {field}: {error}")))?
        .len();
    if size > maximum {
        return Err(AuditEventError::new(format!(
            "{field} exceeds {maximum} serialized bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;

    const FIXTURES: &[&str] = &[
        include_str!("../../tests/fixtures/audit/application.json"),
        include_str!("../../tests/fixtures/audit/organization.json"),
        include_str!("../../tests/fixtures/audit/control_plane.json"),
        include_str!("../../tests/fixtures/audit/correlated_target.json"),
        include_str!("../../tests/fixtures/audit/correlated_control_plane.json"),
    ];

    fn audit_id(value: &str) -> AuditId {
        AuditId::parse(value).expect("test ULID must be valid")
    }

    fn valid_builder() -> RawAuditEventBuilder {
        RawAuditEventBuilder::new()
            .event_id(audit_id("01JZ0000000000000000000001"))
            .occurred_at(
                Utc.with_ymd_and_hms(2026, 7, 18, 10, 0, 0)
                    .single()
                    .expect("test timestamp must be valid"),
            )
            .scope(AuditScopeSelector::application())
            .actor(AuditActor::new("user").with_id("user-123"))
            .service(AuditService::stargate())
            .resource(AuditResource::new("user", "user-123"))
            .action("user.updated")
            .operation(AuditOperation::Update)
    }

    #[test]
    fn audit_id_generation_and_validation_use_canonical_ulids() {
        let generated = AuditId::new();
        assert_eq!(generated.as_str().len(), 26);
        assert!(
            generated
                .as_str()
                .bytes()
                .all(|byte| !byte.is_ascii_lowercase())
        );

        assert!(AuditId::parse("01JZ0000000000000000000001").is_ok());
        assert!(AuditId::parse("01jz0000000000000000000001").is_err());
        assert!(AuditId::parse("01JZ000000000000000000000I").is_err());
        assert!(AuditId::parse("01JZ000000000000000000001").is_err());
    }

    #[test]
    fn audit_golden_fixtures_match_the_exact_serialized_payload() {
        for fixture in FIXTURES {
            let raw: RawAuditEvent =
                serde_json::from_str(fixture).expect("fixture must deserialize");
            let validated = raw.validate_and_serialize().expect("fixture must validate");
            assert_eq!(validated.payload(), fixture.trim());
        }
    }

    #[test]
    fn audit_correlated_fixtures_share_only_the_operation_id() {
        let target: RawAuditEvent =
            serde_json::from_str(FIXTURES[3]).expect("target fixture must deserialize");
        let control_plane: RawAuditEvent =
            serde_json::from_str(FIXTURES[4]).expect("control-plane fixture must deserialize");

        assert_ne!(target.event_id, control_plane.event_id);
        assert_eq!(target.operation_id, control_plane.operation_id);
        assert!(matches!(
            target.scope,
            AuditScopeSelector::Organization { .. }
        ));
        assert_eq!(control_plane.scope, AuditScopeSelector::ControlPlane);
    }

    #[test]
    fn audit_builder_requires_every_non_optional_field() {
        let error = RawAuditEventBuilder::new()
            .build()
            .expect_err("missing event id must fail");
        assert_eq!(error.to_string(), "event_id is required");

        let error = RawAuditEventBuilder::new()
            .event_id(audit_id("01JZ0000000000000000000001"))
            .build()
            .expect_err("missing occurrence time must fail");
        assert_eq!(error.to_string(), "occurred_at is required");

        let event = valid_builder()
            .build()
            .expect("complete event must succeed");
        assert_eq!(event.event().event_version, AUDIT_EVENT_VERSION);
    }

    #[test]
    fn audit_builder_rejects_missing_scope() {
        let builder = RawAuditEventBuilder::new()
            .event_id(audit_id("01JZ0000000000000000000001"))
            .occurred_at(
                Utc.with_ymd_and_hms(2026, 7, 18, 10, 0, 0)
                    .single()
                    .expect("test timestamp must be valid"),
            )
            .actor(AuditActor::new("user").with_id("user-123"))
            .service(AuditService::stargate())
            .resource(AuditResource::new("user", "user-123"))
            .action("user.updated")
            .operation(AuditOperation::Update);
        assert_eq!(
            builder
                .build()
                .expect_err("missing scope must fail")
                .to_string(),
            "scope is required"
        );
    }

    #[test]
    fn audit_builder_rejects_unsupported_version() {
        let error = valid_builder()
            .event_version(2)
            .build()
            .expect_err("unsupported version must fail");
        assert!(error.to_string().contains("event_version must be 1"));
    }

    #[test]
    fn audit_builder_rejects_invalid_organization_scope() {
        let error = valid_builder()
            .scope(AuditScopeSelector::organization("org-123"))
            .build()
            .expect_err("invalid organization id must fail");
        assert!(error.to_string().contains("scope.organization_id"));
    }

    #[test]
    fn audit_builder_rejects_blank_and_control_character_fields() {
        let blank = valid_builder()
            .actor(AuditActor::new("  "))
            .build()
            .expect_err("blank actor type must fail");
        assert!(blank.to_string().contains("actor.type must not be blank"));

        let control = valid_builder()
            .resource(AuditResource::new("user", "user\n123"))
            .build()
            .expect_err("control character must fail");
        assert!(
            control
                .to_string()
                .contains("resource.id must not contain ASCII control characters")
        );
    }

    #[test]
    fn audit_builder_normalizes_optional_blanks_to_explicit_null() {
        let event = valid_builder()
            .actor(AuditActor {
                actor_type: " user ".to_owned(),
                id: Some(" user-123 ".to_owned()),
                email: Some("  ".to_owned()),
            })
            .resource(AuditResource::new(" user ", " user-123 ").with_name("  "))
            .request(AuditRequestContext {
                request_id: " request-123 ".to_owned(),
                trace_id: Some("  ".to_owned()),
                ip_address: None,
                user_agent: None,
            })
            .build()
            .expect("approved normalization must succeed");

        assert_eq!(event.event().actor.id.as_deref(), Some("user-123"));
        assert_eq!(event.event().actor.email, None);
        assert_eq!(event.event().resource.name, None);
        assert_eq!(
            event
                .event()
                .request
                .as_ref()
                .expect("request must remain present")
                .trace_id,
            None
        );
        assert!(event.payload().contains("\"operation_id\":null"));
        assert!(event.payload().contains("\"email\":null"));
        assert!(event.payload().contains("\"name\":null"));
        assert!(event.payload().contains("\"before\":null,\"after\":null"));
    }

    #[test]
    fn audit_builder_rejects_actor_email_in_version_one() {
        let error = valid_builder()
            .actor(AuditActor {
                actor_type: "user".to_owned(),
                id: Some("user-123".to_owned()),
                email: Some("user@example.com".to_owned()),
            })
            .build()
            .expect_err("actor email must fail");
        assert!(error.to_string().contains("actor.email must be null"));
    }

    #[test]
    fn audit_builder_rejects_unapproved_actor_resource_and_action() {
        let actor = valid_builder()
            .actor(AuditActor::new("root"))
            .build()
            .expect_err("unknown actor type must fail");
        assert!(actor.to_string().contains("actor.type 'root'"));

        let resource = valid_builder()
            .resource(AuditResource::new("database", "db-1"))
            .build()
            .expect_err("unknown resource type must fail");
        assert!(resource.to_string().contains("resource.type 'database'"));

        let action = valid_builder()
            .action("user.promoted")
            .build()
            .expect_err("unknown action must fail");
        assert!(action.to_string().contains("action 'user.promoted'"));
    }

    #[test]
    fn audit_builder_rejects_action_operation_mismatch() {
        let error = valid_builder()
            .operation(AuditOperation::Delete)
            .build()
            .expect_err("action-operation mismatch must fail");
        assert!(
            error
                .to_string()
                .contains("action 'user.updated' requires operation 'update'")
        );
    }

    #[test]
    fn audit_operations_deserialize_known_values_case_insensitively() {
        let operation: AuditOperation =
            serde_json::from_str("\"CREATE\"").expect("known operation must deserialize");
        assert_eq!(operation, AuditOperation::Create);
        assert_eq!(
            serde_json::to_string(&operation).expect("operation must serialize"),
            "\"create\""
        );

        let custom: AuditOperation =
            serde_json::from_str("\"import\"").expect("custom operation must deserialize");
        assert_eq!(custom, AuditOperation::Custom("import".to_owned()));
    }

    #[test]
    fn audit_builder_rejects_uppercase_custom_operation() {
        let error = valid_builder()
            .operation(AuditOperation::Custom("IMPORT".to_owned()))
            .build()
            .expect_err("uppercase custom operation must fail");
        assert!(
            error
                .to_string()
                .contains("custom operation must be lowercase")
        );
    }

    #[test]
    fn audit_builder_rejects_non_object_metadata() {
        let error = valid_builder()
            .metadata(json!(["not", "an", "object"]))
            .build()
            .expect_err("non-object metadata must fail");
        assert_eq!(
            error.to_string(),
            "metadata must be a JSON object when present"
        );
    }

    #[test]
    fn audit_deserialization_rejects_non_object_metadata() {
        let mut malformed: Value =
            serde_json::from_str(FIXTURES[0]).expect("fixture must be valid JSON");
        malformed["metadata"] = json!([]);
        let error = serde_json::from_value::<RawAuditEvent>(malformed)
            .expect_err("non-object metadata must not deserialize");
        assert!(error.to_string().contains("invalid type"));
    }

    #[test]
    fn audit_builder_rejects_non_object_snapshots() {
        let error = valid_builder()
            .before(json!(["invalid"]))
            .build()
            .expect_err("array snapshot must fail");
        assert_eq!(error.to_string(), "before must be a JSON object or null");
    }

    #[test]
    fn audit_builder_normalizes_explicit_null_snapshots() {
        let event = valid_builder()
            .before(Value::Null)
            .after(Value::Null)
            .build()
            .expect("explicit null snapshots must normalize");
        assert_eq!(event.event().before, None);
        assert_eq!(event.event().after, None);
        assert!(event.payload().contains("\"before\":null,\"after\":null"));
    }

    #[test]
    fn audit_builder_rejects_oversized_snapshot_values_and_arrays() {
        let long_string = valid_builder()
            .before(json!({ "value": "x".repeat(MAX_SNAPSHOT_STRING_BYTES + 1) }))
            .build()
            .expect_err("oversized snapshot string must fail");
        assert!(
            long_string
                .to_string()
                .contains("contains a string exceeding")
        );

        let long_array = valid_builder()
            .after(json!({ "values": vec![0; MAX_SNAPSHOT_ARRAY_ITEMS + 1] }))
            .build()
            .expect_err("oversized snapshot array must fail");
        assert!(
            long_array
                .to_string()
                .contains("contains an array with more than")
        );
    }

    #[test]
    fn audit_builder_rejects_oversized_components() {
        let mut before = Map::new();
        for index in 0..9 {
            before.insert(index.to_string(), Value::String("x".repeat(2_000)));
        }
        let before_error = valid_builder()
            .before(Value::Object(before))
            .build()
            .expect_err("oversized before object must fail");
        assert!(
            before_error
                .to_string()
                .contains("before exceeds 16384 serialized bytes")
        );

        let metadata_error = valid_builder()
            .metadata(json!({ "value": "x".repeat(MAX_METADATA_BYTES) }))
            .build()
            .expect_err("oversized metadata must fail");
        assert!(
            metadata_error
                .to_string()
                .contains("metadata exceeds 8192 serialized bytes")
        );
    }

    #[test]
    fn audit_builder_rejects_oversized_text_fields() {
        let resource_id = valid_builder()
            .resource(AuditResource::new(
                "user",
                "x".repeat(MAX_IDENTIFIER_BYTES + 1),
            ))
            .build()
            .expect_err("oversized resource id must fail");
        assert!(
            resource_id
                .to_string()
                .contains("resource.id exceeds 255 UTF-8 bytes")
        );

        let user_agent = valid_builder()
            .request(
                AuditRequestContext::new("request-1")
                    .with_user_agent("x".repeat(MAX_USER_AGENT_BYTES + 1)),
            )
            .build()
            .expect_err("oversized user agent must fail");
        assert!(
            user_agent
                .to_string()
                .contains("request.user_agent exceeds 512 UTF-8 bytes")
        );
    }

    #[test]
    fn audit_builder_rejects_excessive_json_nesting() {
        let mut nested = json!(true);
        for _ in 0..=MAX_DYNAMIC_JSON_DEPTH {
            nested = json!({ "nested": nested });
        }
        let error = valid_builder()
            .before(nested)
            .build()
            .expect_err("excessive nesting must fail");
        assert!(error.to_string().contains("exceeds 8 JSON nesting levels"));
    }

    #[test]
    fn audit_deserialization_rejects_malformed_event_id() {
        let malformed = FIXTURES[0].replacen(
            "01JZ0000000000000000000001",
            "01jz0000000000000000000001",
            1,
        );
        let error = serde_json::from_str::<RawAuditEvent>(&malformed)
            .expect_err("lowercase event id must not deserialize");
        assert!(error.to_string().contains("uppercase 26-character"));
    }

    #[test]
    fn audit_deserialization_rejects_invalid_timestamp() {
        let malformed = FIXTURES[0].replacen("2026-07-18T10:00:00Z", "not-an-rfc3339-timestamp", 1);
        let error = serde_json::from_str::<RawAuditEvent>(&malformed)
            .expect_err("invalid timestamp must not deserialize");
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn audit_deserialization_rejects_malformed_operation_id() {
        let malformed = FIXTURES[3].replacen(
            "01JZ000000000000000000000X",
            "01jz000000000000000000000x",
            1,
        );
        let error = serde_json::from_str::<RawAuditEvent>(&malformed)
            .expect_err("lowercase operation id must not deserialize");
        assert!(error.to_string().contains("uppercase 26-character"));
    }

    #[test]
    fn audit_missing_metadata_normalizes_to_an_explicit_empty_object() {
        let mut fixture: Value =
            serde_json::from_str(FIXTURES[0]).expect("fixture must be valid JSON");
        fixture
            .as_object_mut()
            .expect("fixture root must be an object")
            .remove("metadata");
        let raw: RawAuditEvent =
            serde_json::from_value(fixture).expect("missing metadata must deserialize");
        let event = raw
            .validate_and_serialize()
            .expect("missing metadata must normalize");
        assert!(event.payload().contains("\"metadata\":{}"));
    }

    #[test]
    fn audit_validated_payload_exposes_stable_identity_and_parts() {
        let event = valid_builder()
            .operation_id(audit_id("01JZ000000000000000000000X"))
            .build()
            .expect("valid event must build");
        assert_eq!(event.event_id().as_str(), "01JZ0000000000000000000001");
        assert_eq!(
            event.operation_id().map(AuditId::as_str),
            Some("01JZ000000000000000000000X")
        );

        let payload = event.payload().to_owned();
        let (raw, stored_payload) = event.into_parts();
        assert_eq!(raw.event_id.as_str(), "01JZ0000000000000000000001");
        assert_eq!(stored_payload, payload);
    }
}
