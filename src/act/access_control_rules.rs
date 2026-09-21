use crate::{
    err::{ErrorCode, ErrorResponse},
    etc::{
        ac::{Env, create_context},
        sub::{Subject, SubjectType},
    },
};
use ace::{Policy, PolicyAction, PolicyEngine, ResourceAction};
use gate::PolicySnapshot;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccessControlRule {
    pub id: Option<String>,
    pub description: Option<String>,
    pub effect: String,
    pub subject: String,
    pub resource: String,
    pub action: Option<String>,
    pub condition: Option<String>,
    pub statement: String,
    pub line: usize,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PolicyDiagnostic {
    pub line: usize,
    pub message: String,
    pub statement: String,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccessControlRulesResponse {
    pub revision: String,
    pub content: String,
    pub valid: bool,
    pub diagnostics: Vec<PolicyDiagnostic>,
    pub rules: Vec<AccessControlRule>,
}

#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccessControlRulesRequest {
    pub revision: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateAccessControlRulesRequest {
    pub content: String,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateAccessControlRulesResponse {
    pub valid: bool,
    pub diagnostics: Vec<PolicyDiagnostic>,
    pub rules: Vec<AccessControlRule>,
}

#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateAccessControlRequest {
    pub subject: String,
    pub resource: String,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateAccessControlResponse {
    pub allowed: bool,
    pub matched_policies: Vec<usize>,
    pub applied_policies: Vec<usize>,
    pub allow_count: usize,
    pub deny_count: usize,
}

#[derive(Clone, Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessControlCapabilityEnvironment {
    #[serde(default)]
    pub ip_address: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub country_code: String,
    #[serde(default)]
    pub country_name: String,
    #[serde(default)]
    pub city_name: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub day_of_week: String,
}

impl From<AccessControlCapabilityEnvironment> for Env {
    fn from(value: AccessControlCapabilityEnvironment) -> Self {
        Self {
            ip_address: value.ip_address.into(),
            user_agent: value.user_agent.into(),
            country_code: value.country_code.into(),
            country_name: value.country_name.into(),
            city_name: value.city_name.into(),
            date: value.date.into(),
            time: value.time.into(),
            day_of_week: value.day_of_week.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaluateAccessControlCapabilitiesRequest {
    pub subject: SubjectType,
    #[serde(default)]
    pub attrs: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub org_role: Option<String>,
    #[serde(default)]
    pub env: AccessControlCapabilityEnvironment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum AccessControlCapabilityAction {
    Read,
    Write,
    Delete,
    Create,
    Update,
    Execute,
    Admin,
}

impl AccessControlCapabilityAction {
    fn from_concrete(action: ResourceAction) -> Self {
        match action {
            ResourceAction::Read => Self::Read,
            ResourceAction::Write => Self::Write,
            ResourceAction::Delete => Self::Delete,
            ResourceAction::Create => Self::Create,
            ResourceAction::Update => Self::Update,
            ResourceAction::Execute => Self::Execute,
            ResourceAction::Admin => Self::Admin,
            ResourceAction::Any => unreachable!("ANY is not a concrete capability action"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccessControlCapability {
    pub resource: String,
    pub unscoped_allowed: bool,
    pub actions: Vec<AccessControlCapabilityAction>,
}

#[derive(Clone, Debug, PartialEq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateAccessControlCapabilitiesResponse {
    pub revision: String,
    pub capabilities: Vec<AccessControlCapability>,
}

#[derive(Default)]
struct PendingMetadata {
    id: Option<String>,
    description: Option<String>,
}

struct ParsedRule {
    schema: AccessControlRule,
    policy: Policy,
}

struct ParsedDocument {
    rules: Vec<ParsedRule>,
    diagnostics: Vec<PolicyDiagnostic>,
}

static POLICY_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn read_rules() -> Result<AccessControlRulesResponse, ErrorResponse> {
    let path = crate::etc::gate::get_policies_path();
    let content = read_policy_file(&path)?;
    let parsed = parse_policy_document(&content);

    Ok(AccessControlRulesResponse {
        revision: revision_for(&content),
        content,
        valid: parsed.diagnostics.is_empty(),
        diagnostics: parsed.diagnostics,
        rules: parsed.rules.into_iter().map(|rule| rule.schema).collect(),
    })
}

pub fn validate_rules(
    request: ValidateAccessControlRulesRequest,
) -> ValidateAccessControlRulesResponse {
    let parsed = parse_policy_document(&request.content);
    ValidateAccessControlRulesResponse {
        valid: parsed.diagnostics.is_empty(),
        diagnostics: parsed.diagnostics,
        rules: parsed.rules.into_iter().map(|rule| rule.schema).collect(),
    }
}

pub fn update_rules(
    request: UpdateAccessControlRulesRequest,
) -> Result<AccessControlRulesResponse, ErrorResponse> {
    let _guard = POLICY_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| ErrorResponse::internal("policy write lock poisoned"))?;

    let path = crate::etc::gate::get_policies_path();
    let current = read_policy_file(&path)?;
    let current_revision = revision_for(&current);
    if request.revision != current_revision {
        return Err(ErrorResponse::new(ErrorCode::AccessControlRevisionConflict)
            .with_param("revision", request.revision));
    }

    let parsed = parse_policy_document(&request.content);
    if !parsed.diagnostics.is_empty() {
        return Err(ErrorResponse::new(ErrorCode::AccessControlPoliciesInvalid)
            .with_param("diagnostics", format_diagnostics(&parsed.diagnostics)));
    }

    atomic_write(&path, request.content.as_bytes()).map_err(ErrorResponse::internal)?;

    Ok(AccessControlRulesResponse {
        revision: revision_for(&request.content),
        content: request.content,
        valid: true,
        diagnostics: Vec::new(),
        rules: parsed.rules.into_iter().map(|rule| rule.schema).collect(),
    })
}

pub fn evaluate_rules(
    request: EvaluateAccessControlRequest,
    engine: &PolicyEngine,
) -> Result<EvaluateAccessControlResponse, ErrorResponse> {
    let (resource, action_from_resource) = split_resource_action(&request.resource)?;
    let action = resolve_requested_action(request.action.as_deref(), action_from_resource)?;
    let context = request
        .context
        .iter()
        .map(|(key, value)| (key.clone(), ace::Value::from(value)))
        .collect::<HashMap<_, _>>();

    let result = match action {
        Some(action) => {
            engine.evaluate_with_details_and_action(&request.subject, resource, &action, &context)
        }
        None => engine.evaluate_with_details(&request.subject, resource, &context),
    };

    Ok(EvaluateAccessControlResponse {
        allowed: result.decision,
        matched_policies: result.matched_policies,
        applied_policies: result.applied_policies,
        allow_count: result.allow_count,
        deny_count: result.deny_count,
    })
}

pub fn evaluate_capabilities(
    request: EvaluateAccessControlCapabilitiesRequest,
    snapshot: &PolicySnapshot,
) -> EvaluateAccessControlCapabilitiesResponse {
    evaluate_capabilities_with_engine(request, &snapshot.engine, snapshot.revision.clone())
}

fn evaluate_capabilities_with_engine(
    request: EvaluateAccessControlCapabilitiesRequest,
    engine: &PolicyEngine,
    revision: String,
) -> EvaluateAccessControlCapabilitiesResponse {
    let subject_type = request.subject.as_str().to_string();
    let resources = engine
        .get_policies()
        .iter()
        .filter(|policy| policy.subject == subject_type)
        .map(|policy| policy.resource.clone())
        .collect::<BTreeSet<_>>();

    let attrs = request.attrs.into_iter().collect::<serde_json::Map<_, _>>();
    let mut subject = Subject::new(
        "access-control-capability-evaluation".to_string(),
        request.subject,
        Some(serde_json::Value::Object(attrs)),
    );
    subject.org_id = request.org_id;
    subject.org_role = request.org_role;
    let context = create_context(&subject, &request.env.into());

    let capabilities = resources
        .into_iter()
        .filter_map(|resource| {
            let unscoped_allowed = engine.evaluate(&subject_type, &resource, &context);
            let actions = ResourceAction::CONCRETE
                .into_iter()
                .filter(|action| {
                    engine.evaluate_with_action(&subject_type, &resource, action, &context)
                })
                .map(AccessControlCapabilityAction::from_concrete)
                .collect::<Vec<_>>();

            (unscoped_allowed || !actions.is_empty()).then_some(AccessControlCapability {
                resource,
                unscoped_allowed,
                actions,
            })
        })
        .collect();

    EvaluateAccessControlCapabilitiesResponse {
        revision,
        capabilities,
    }
}

pub(crate) fn load_policy_snapshot(path: &str) -> Result<PolicySnapshot, ErrorResponse> {
    let content = read_policy_file(path)?;
    compile_policy_snapshot(&content)
}

fn compile_policy_snapshot(content: &str) -> Result<PolicySnapshot, ErrorResponse> {
    let parsed = parse_evaluable_document(content)?;
    let mut engine = PolicyEngine::new();
    for rule in parsed.rules {
        engine.add_policy(rule.policy);
    }
    Ok(PolicySnapshot::new(engine, revision_for(content)))
}

fn read_policy_file(path: &str) -> Result<String, ErrorResponse> {
    fs::read_to_string(path).map_err(ErrorResponse::internal)
}

fn parse_policy_document(content: &str) -> ParsedDocument {
    let parser = PolicyEngine::new();
    let mut pending = PendingMetadata::default();
    let mut seen_ids = HashSet::new();
    let mut rules = Vec::new();
    let mut diagnostics = Vec::new();

    for (idx, line) in content.lines().enumerate() {
        let line_number = idx + 1;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            pending = PendingMetadata::default();
            continue;
        }

        if let Some(comment) = trimmed.strip_prefix('#') {
            apply_metadata_comment(comment.trim(), line_number, &mut pending, &mut diagnostics);
            continue;
        }

        match parser.parse_policy(trimmed) {
            Ok(policy) => {
                if let Some(id) = pending.id.as_deref()
                    && !seen_ids.insert(id.to_string())
                {
                    diagnostics.push(PolicyDiagnostic {
                        line: line_number,
                        message: format!("Duplicate policy id '{}'", id),
                        statement: trimmed.to_string(),
                    });
                }

                rules.push(ParsedRule {
                    schema: rule_schema(trimmed, line_number, &pending, &policy),
                    policy,
                });
            }
            Err(error) => diagnostics.push(PolicyDiagnostic {
                line: line_number,
                message: error.to_string(),
                statement: trimmed.to_string(),
            }),
        }

        pending = PendingMetadata::default();
    }

    ParsedDocument { rules, diagnostics }
}

fn parse_evaluable_document(content: &str) -> Result<ParsedDocument, ErrorResponse> {
    let parsed = parse_policy_document(content);
    if parsed.diagnostics.is_empty() {
        return Ok(parsed);
    }

    Err(
        ErrorResponse::new(ErrorCode::AccessControlEvaluationUnavailable)
            .with_param("diagnostics", format_diagnostics(&parsed.diagnostics)),
    )
}

fn apply_metadata_comment(
    comment: &str,
    line: usize,
    pending: &mut PendingMetadata,
    diagnostics: &mut Vec<PolicyDiagnostic>,
) {
    if let Some(value) = metadata_value(comment, "@id") {
        if value.is_empty() {
            diagnostics.push(PolicyDiagnostic {
                line,
                message: "Policy metadata @id cannot be empty".to_string(),
                statement: format!("# {}", comment),
            });
        } else {
            pending.id = Some(value.to_string());
        }
    } else if let Some(value) = metadata_value(comment, "@description") {
        pending.description = (!value.is_empty()).then(|| value.to_string());
    }
}

fn metadata_value<'a>(comment: &'a str, key: &str) -> Option<&'a str> {
    let rest = comment.strip_prefix(key)?;
    if !rest.is_empty()
        && !rest
            .chars()
            .next()
            .is_some_and(|ch| ch == ':' || ch.is_whitespace())
    {
        return None;
    }

    let rest = rest.trim_start();
    Some(rest.strip_prefix(':').unwrap_or(rest).trim())
}

fn rule_schema(
    statement: &str,
    line: usize,
    metadata: &PendingMetadata,
    policy: &Policy,
) -> AccessControlRule {
    AccessControlRule {
        id: metadata.id.clone(),
        description: metadata.description.clone(),
        effect: match policy.action {
            PolicyAction::Allow => "allow".to_string(),
            PolicyAction::Deny => "deny".to_string(),
        },
        subject: policy.subject.clone(),
        resource: policy.resource.clone(),
        action: policy.resource_action.map(|action| action.to_string()),
        condition: policy
            .condition
            .as_ref()
            .map(|condition| condition.to_string()),
        statement: statement.to_string(),
        line,
    }
}

fn split_resource_action(resource: &str) -> Result<(&str, Option<ResourceAction>), ErrorResponse> {
    let Some((resource, action)) = resource.split_once(':') else {
        return Ok((resource, None));
    };

    let action = parse_resource_action(action)?;
    Ok((resource, Some(action)))
}

fn resolve_requested_action(
    explicit: Option<&str>,
    from_resource: Option<ResourceAction>,
) -> Result<Option<ResourceAction>, ErrorResponse> {
    let Some(explicit) = explicit else {
        return Ok(from_resource);
    };

    let explicit = parse_resource_action(explicit)?;
    if let Some(from_resource) = from_resource
        && explicit != from_resource
    {
        return Err(ErrorResponse::new(ErrorCode::AccessControlActionConflict));
    }

    Ok(Some(explicit))
}

fn parse_resource_action(action: &str) -> Result<ResourceAction, ErrorResponse> {
    ResourceAction::parse(action).ok_or_else(|| {
        ErrorResponse::new(ErrorCode::AccessControlActionInvalid).with_param("action", action)
    })
}

fn format_diagnostics(diagnostics: &[PolicyDiagnostic]) -> String {
    diagnostics
        .iter()
        .take(5)
        .map(|diagnostic| format!("line {}: {}", diagnostic.line, diagnostic.message))
        .collect::<Vec<_>>()
        .join("; ")
}

fn revision_for(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let hex = digest
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>();
    format!("sha256:{}", hex)
}

fn atomic_write(path: &str, content: &[u8]) -> std::io::Result<()> {
    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = temp_path_for(path);
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp_path, path)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }

    write_result
}

fn temp_path_for(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("policies");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    parent.join(format!(
        ".{}.{}.{}.tmp",
        file_name,
        std::process::id(),
        nanos
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn capability_request(attrs: serde_json::Value) -> EvaluateAccessControlCapabilitiesRequest {
        EvaluateAccessControlCapabilitiesRequest {
            subject: SubjectType::User,
            attrs: attrs
                .as_object()
                .expect("test attributes are an object")
                .clone()
                .into_iter()
                .collect(),
            org_id: None,
            org_role: None,
            env: AccessControlCapabilityEnvironment::default(),
        }
    }

    fn evaluate_capability_document(
        request: EvaluateAccessControlCapabilitiesRequest,
        content: &str,
    ) -> Result<EvaluateAccessControlCapabilitiesResponse, ErrorResponse> {
        let snapshot = compile_policy_snapshot(content)?;
        Ok(evaluate_capabilities(request, &snapshot))
    }

    #[test]
    fn parses_policy_metadata_and_statement() {
        let parsed = parse_policy_document(
            r#"# @id reports-read-admin
# @description Admins can read reports
ALLOW user FOR "reports:READ" WHEN user.role == "admin";
"#,
        );

        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.rules.len(), 1);
        let rule = &parsed.rules[0].schema;
        assert_eq!(rule.id.as_deref(), Some("reports-read-admin"));
        assert_eq!(rule.description.as_deref(), Some("Admins can read reports"));
        assert_eq!(rule.effect, "allow");
        assert_eq!(rule.resource, "reports");
        assert_eq!(rule.action.as_deref(), Some("READ"));
    }

    #[test]
    fn rejects_duplicate_policy_ids() {
        let parsed = parse_policy_document(
            r#"# @id duplicate
ALLOW user FOR "one";
# @id duplicate
ALLOW user FOR "two";
"#,
        );

        assert_eq!(parsed.diagnostics.len(), 1);
        assert!(
            parsed.diagnostics[0]
                .message
                .contains("Duplicate policy id")
        );
    }

    #[test]
    fn reports_invalid_policy_line() {
        let parsed = parse_policy_document("ALLOW user WHEN bad;\n");

        assert_eq!(parsed.diagnostics.len(), 1);
        assert_eq!(parsed.diagnostics[0].line, 1);
    }

    #[test]
    fn ignores_metadata_key_prefixes() {
        let parsed = parse_policy_document(
            r#"# @identifier not-an-id
ALLOW user FOR "reports";
"#,
        );

        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.rules[0].schema.id, None);
    }

    #[test]
    fn validates_resource_action_conflicts() {
        let err = resolve_requested_action(Some("WRITE"), Some(ResourceAction::Read)).unwrap_err();

        assert_eq!(err.status, http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn single_resource_evaluation_uses_the_compiled_snapshot() {
        let snapshot = compile_policy_snapshot(
            r#"ALLOW user FOR "reports:READ" WHEN user.role == "analyst";"#,
        )
        .unwrap();
        let response = evaluate_rules(
            EvaluateAccessControlRequest {
                subject: "user".to_string(),
                resource: "reports".to_string(),
                action: Some("READ".to_string()),
                context: HashMap::from([("user.role".to_string(), json!("analyst"))]),
            },
            &snapshot.engine,
        )
        .unwrap();

        assert!(response.allowed);
        assert_eq!(response.allow_count, 1);
    }

    #[test]
    fn evaluates_declared_capabilities_with_action_and_deny_semantics() {
        let content = r#"
ALLOW user FOR "dashboard" WHEN user.role == "analyst";
ALLOW user FOR "reports:READ" WHEN user.role == "analyst";
ALLOW user FOR "reports:DELETE" WHEN user.role == "analyst";
DENY user FOR "reports:DELETE" WHEN user.suspended == true;
DENY user FOR "hidden:READ";
ALLOW api_key FOR "ingest:WRITE";
"#;

        let response = evaluate_capability_document(
            capability_request(json!({ "role": "analyst", "suspended": true })),
            content,
        )
        .unwrap();

        assert!(response.revision.starts_with("sha256:"));
        assert_eq!(response.capabilities.len(), 2);

        let dashboard = &response.capabilities[0];
        assert_eq!(dashboard.resource, "dashboard");
        assert!(dashboard.unscoped_allowed);
        assert_eq!(
            dashboard.actions,
            vec![
                AccessControlCapabilityAction::Read,
                AccessControlCapabilityAction::Write,
                AccessControlCapabilityAction::Delete,
                AccessControlCapabilityAction::Create,
                AccessControlCapabilityAction::Update,
                AccessControlCapabilityAction::Execute,
                AccessControlCapabilityAction::Admin,
            ]
        );

        let reports = &response.capabilities[1];
        assert_eq!(reports.resource, "reports");
        assert!(!reports.unscoped_allowed);
        assert_eq!(reports.actions, vec![AccessControlCapabilityAction::Read]);
    }

    #[test]
    fn trusted_organization_context_overrides_spoofed_attributes() {
        let content = r#"
ALLOW user FOR "real-org:READ" WHEN user.org_id == "org_real";
ALLOW user FOR "spoofed-org:READ" WHEN user.org_id == "org_spoofed";
"#;
        let mut request = capability_request(json!({ "org_id": "org_spoofed" }));
        request.org_id = Some("org_real".to_string());

        let response = evaluate_capability_document(request, content).unwrap();

        assert_eq!(response.capabilities.len(), 1);
        assert_eq!(response.capabilities[0].resource, "real-org");
        assert_eq!(
            response.capabilities[0].actions,
            vec![AccessControlCapabilityAction::Read]
        );
    }

    #[test]
    fn capability_environment_uses_gateway_context_names_and_types() {
        let content = r#"
ALLOW user FOR "office:READ" WHEN env.country_code == "IT" AND env.time >= "09:00";
"#;
        let mut request = capability_request(json!({}));
        request.env.country_code = "IT".to_string();
        request.env.time = "10:30".to_string();

        let response = evaluate_capability_document(request, content).unwrap();

        assert_eq!(response.capabilities.len(), 1);
        assert_eq!(response.capabilities[0].resource, "office");
        assert_eq!(
            response.capabilities[0].actions,
            vec![AccessControlCapabilityAction::Read]
        );
    }

    #[test]
    fn invalid_policy_document_cannot_be_enumerated() {
        let error =
            evaluate_capability_document(capability_request(json!({})), "ALLOW user WHEN invalid;")
                .unwrap_err();

        assert_eq!(error.status, http::StatusCode::BAD_REQUEST);
        assert_eq!(error.code, ErrorCode::AccessControlEvaluationUnavailable);
    }
}
