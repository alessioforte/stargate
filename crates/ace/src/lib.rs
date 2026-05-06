//! Access Control Engine (ACE) for Stargate
//!
//! This crate provides a policy-based access control system that parses
//! policy definitions from comments and evaluates them against runtime context.
//!
//! # Policy Syntax
//!
//! Policies are defined with the following syntax:
//!
//! ```text
//! ALLOW|DENY subject FOR "resource" [WHEN condition];
//! ```
//!
//! Where:
//! - `ALLOW|DENY` - The action to take
//! - `subject` - The entity requesting access (e.g., "user", "api_key")
//! - `resource` - The resource being accessed (quoted string)
//! - `condition` - Optional boolean expression using AND/OR logic
//!
//! # Examples
//!
//! ```text
//! ALLOW user FOR "feature1" WHEN user.role == "admin" OR user.role == "editor";
//! DENY user FOR "feature2" WHEN user.role == "guest";
//! ALLOW api_key FOR "feature3" WHEN api_key.valid == true AND api_key.scope == "read";
//! ```

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime};
use std::collections::HashMap;
use std::fmt;

mod evaluator;
mod parser;

pub use evaluator::{EvaluationResult, PolicyEvaluator};
pub use parser::PolicyParser;

/// Represents the action a policy should take
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyAction {
    Allow,
    Deny,
}

impl fmt::Display for PolicyAction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PolicyAction::Allow => write!(f, "ALLOW"),
            PolicyAction::Deny => write!(f, "DENY"),
        }
    }
}

/// A complete policy definition
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    pub action: PolicyAction,
    pub subject: String,
    pub resource: String,
    pub resource_action: Option<ResourceAction>,
    pub condition: Option<Condition>,
}

/// Resource actions that can be performed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceAction {
    Read,
    Write,
    Delete,
    Create,
    Update,
    Execute,
    Admin,
    Any,
}

impl fmt::Display for ResourceAction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ResourceAction::Read => write!(f, "READ"),
            ResourceAction::Write => write!(f, "WRITE"),
            ResourceAction::Delete => write!(f, "DELETE"),
            ResourceAction::Create => write!(f, "CREATE"),
            ResourceAction::Update => write!(f, "UPDATE"),
            ResourceAction::Execute => write!(f, "EXECUTE"),
            ResourceAction::Admin => write!(f, "ADMIN"),
            ResourceAction::Any => write!(f, "*"),
        }
    }
}

impl ResourceAction {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "READ" => Some(ResourceAction::Read),
            "WRITE" => Some(ResourceAction::Write),
            "DELETE" => Some(ResourceAction::Delete),
            "CREATE" => Some(ResourceAction::Create),
            "UPDATE" => Some(ResourceAction::Update),
            "EXECUTE" => Some(ResourceAction::Execute),
            "ADMIN" => Some(ResourceAction::Admin),
            "*" | "ANY" => Some(ResourceAction::Any),
            _ => None,
        }
    }
}

impl Policy {
    pub fn new(action: PolicyAction, subject: String, resource: String) -> Self {
        Self {
            action,
            subject,
            resource,
            resource_action: None,
            condition: None,
        }
    }

    pub fn with_condition(mut self, condition: Condition) -> Self {
        self.condition = Some(condition);
        self
    }

    pub fn with_resource_action(mut self, resource_action: ResourceAction) -> Self {
        self.resource_action = Some(resource_action);
        self
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} {} FOR \"{}\"",
            self.action, self.subject, self.resource
        )?;
        if let Some(ref resource_action) = self.resource_action {
            write!(f, ":{}", resource_action)?;
        }
        if let Some(ref condition) = self.condition {
            write!(f, " WHEN {}", condition)?;
        }
        write!(f, ";")
    }
}

/// Represents a condition in a policy
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Expression(Expression),
    And(Box<Condition>, Box<Condition>),
    Or(Box<Condition>, Box<Condition>),
    Not(Box<Condition>),
}

impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Condition::Expression(expr) => write!(f, "{}", expr),
            Condition::And(left, right) => write!(f, "{} AND {}", left, right),
            Condition::Or(left, right) => write!(f, "{} OR {}", left, right),
            Condition::Not(condition) => write!(f, "NOT ({})", condition),
        }
    }
}

/// A single boolean expression
#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub left: String,
    pub operator: Operator,
    pub right: Value,
}

impl fmt::Display for Expression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {} {}", self.left, self.operator, self.right)
    }
}

/// Comparison operators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operator {
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Contains,
}

impl fmt::Display for Operator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Operator::Equal => write!(f, "=="),
            Operator::NotEqual => write!(f, "!="),
            Operator::GreaterThan => write!(f, ">"),
            Operator::LessThan => write!(f, "<"),
            Operator::GreaterThanOrEqual => write!(f, ">="),
            Operator::LessThanOrEqual => write!(f, "<="),
            Operator::Contains => write!(f, "CONTAINS"),
        }
    }
}

/// Represents a value in an expression
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Boolean(bool),
    Number(i64),
    Float(f64),
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(DateTime<FixedOffset>),
    Array(Vec<Value>),
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            (Value::Number(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
            (Value::Float(a), Value::Number(b)) => a.partial_cmp(&(*b as f64)),
            (Value::Date(a), Value::Date(b)) => a.partial_cmp(b),
            (Value::Time(a), Value::Time(b)) => a.partial_cmp(b),
            (Value::DateTime(a), Value::DateTime(b)) => a.partial_cmp(b),
            (Value::String(a), Value::String(b)) => a.partial_cmp(b),
            (Value::Boolean(a), Value::Boolean(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::Float(f_val) => write!(f, "{}", f_val),
            Value::Date(date) => write!(f, "{}", date),
            Value::Time(time) => write!(f, "{}", time.format("%H:%M:%S")),
            Value::DateTime(dt) => write!(f, "{}", dt.to_rfc3339()),
            Value::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
        }
    }
}

/// from serde_json::Value
/// Convert serde_json::Value to our Value enum
impl From<serde_json::Value> for Value {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::String(s) => Value::String(s),
            serde_json::Value::Bool(b) => Value::Boolean(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Number(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::String(n.to_string())
                }
            }
            serde_json::Value::Array(arr) => {
                Value::Array(arr.into_iter().map(Value::from).collect())
            }
            _ => Value::String(v.to_string()),
        }
    }
}

/// from &serde_json::Value
/// Convert borrowed serde_json::Value to our Value enum.
impl From<&serde_json::Value> for Value {
    fn from(v: &serde_json::Value) -> Self {
        match v {
            serde_json::Value::String(s) => {
                if let Some(dt) = parse_iso_datetime(s) {
                    Value::DateTime(dt)
                } else if let Some(dt) = parse_time_with_offset(s) {
                    Value::DateTime(dt)
                } else if let Some(date) = parse_iso_date(s) {
                    Value::Date(date)
                } else if let Some(time) = parse_time_of_day(s) {
                    Value::Time(time)
                } else {
                    Value::String(s.clone())
                }
            }
            serde_json::Value::Bool(b) => Value::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Number(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::String(n.to_string())
                }
            }
            serde_json::Value::Array(arr) => Value::Array(arr.iter().map(Value::from).collect()),
            _ => Value::String(v.to_string()),
        }
    }
}

pub(crate) fn parse_iso_date(input: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(input, "%Y-%m-%d").ok()
}

pub(crate) fn parse_time_of_day(input: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(input, "%H:%M:%S")
        .ok()
        .or_else(|| NaiveTime::parse_from_str(input, "%H:%M").ok())
}

pub(crate) fn parse_iso_datetime(input: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(input)
        .ok()
        .or_else(|| DateTime::parse_from_str(input, "%Y-%m-%dT%H:%M%:z").ok())
        .or_else(|| {
            if let Some(stripped) = input.strip_suffix('Z') {
                let s = format!("{}+00:00", stripped);
                DateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M%:z").ok()
            } else {
                None
            }
        })
}

pub(crate) fn parse_time_with_offset(input: &str) -> Option<DateTime<FixedOffset>> {
    let full = format!("2000-01-01T{}", input);
    parse_iso_datetime(&full)
}

/// Errors that can occur during policy parsing
#[derive(Debug, Clone)]
pub enum ParseError {
    InvalidSyntax(String),
    UnexpectedToken(String),
    MissingToken(String),
    InvalidValue(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParseError::InvalidSyntax(msg) => write!(f, "Invalid syntax: {}", msg),
            ParseError::UnexpectedToken(token) => write!(f, "Unexpected token: {}", token),
            ParseError::MissingToken(token) => write!(f, "Missing token: {}", token),
            ParseError::InvalidValue(val) => write!(f, "Invalid value: {}", val),
        }
    }
}

impl std::error::Error for ParseError {}

/// Main policy engine that combines parsing and evaluation
pub struct PolicyEngine {
    policies: Vec<Policy>,
    policy_index: HashMap<String, HashMap<String, Vec<usize>>>,
    parser: PolicyParser,
    evaluator: PolicyEvaluator,
}

impl PolicyEngine {
    /// Create a new policy engine
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
            policy_index: HashMap::new(),
            parser: PolicyParser::new(),
            evaluator: PolicyEvaluator::new(),
        }
    }

    /// Parse policy definitions from a string (typically file content)
    pub fn parse_file(&mut self, content: &str) -> Result<(), ParseError> {
        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comment lines
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Try to parse as policy, skip invalid lines
            if let Ok(policy) = self.parser.parse_line(line) {
                self.add_policy(policy);
            }
        }
        Ok(())
    }

    /// Parse a single policy line
    pub fn parse_policy(&self, line: &str) -> Result<Policy, ParseError> {
        self.parser.parse_line(line)
    }

    /// Evaluate access for a subject and resource with given context
    pub fn evaluate(
        &self,
        subject: &str,
        resource: &str,
        context: &HashMap<String, Value>,
    ) -> bool {
        let Some(candidate_indices) = self.candidate_indices(subject, resource) else {
            return false;
        };
        self.evaluator
            .evaluate_indexed(&self.policies, candidate_indices, None, context)
    }

    /// Evaluate access for a subject and resource with specific action
    pub fn evaluate_with_action(
        &self,
        subject: &str,
        resource: &str,
        resource_action: &ResourceAction,
        context: &HashMap<String, Value>,
    ) -> bool {
        let Some(candidate_indices) = self.candidate_indices(subject, resource) else {
            return false;
        };
        self.evaluator.evaluate_indexed(
            &self.policies,
            candidate_indices,
            Some(resource_action),
            context,
        )
    }

    /// Evaluate with detailed results
    pub fn evaluate_with_details(
        &self,
        subject: &str,
        resource: &str,
        context: &HashMap<String, Value>,
    ) -> EvaluationResult {
        let Some(candidate_indices) = self.candidate_indices(subject, resource) else {
            return EvaluationResult {
                decision: false,
                matched_policies: Vec::new(),
                applied_policies: Vec::new(),
                allow_count: 0,
                deny_count: 0,
            };
        };
        self.evaluator.evaluate_with_details_indexed(
            &self.policies,
            candidate_indices,
            None,
            context,
        )
    }

    /// Evaluate with detailed results for specific action
    pub fn evaluate_with_details_and_action(
        &self,
        subject: &str,
        resource: &str,
        resource_action: &ResourceAction,
        context: &HashMap<String, Value>,
    ) -> EvaluationResult {
        let Some(candidate_indices) = self.candidate_indices(subject, resource) else {
            return EvaluationResult {
                decision: false,
                matched_policies: Vec::new(),
                applied_policies: Vec::new(),
                allow_count: 0,
                deny_count: 0,
            };
        };
        self.evaluator.evaluate_with_details_indexed(
            &self.policies,
            candidate_indices,
            Some(resource_action),
            context,
        )
    }

    /// Get all policies
    pub fn get_policies(&self) -> &[Policy] {
        &self.policies
    }

    /// Add a policy programmatically
    pub fn add_policy(&mut self, policy: Policy) {
        self.policies.push(policy);
        let index = self.policies.len() - 1;
        self.index_policy(index);
    }

    /// Clear all policies
    pub fn clear_policies(&mut self) {
        self.policies.clear();
        self.policy_index.clear();
    }

    /// Get policies that match a subject and resource
    pub fn get_matching_policies(&self, subject: &str, resource: &str) -> Vec<&Policy> {
        self.candidate_indices(subject, resource)
            .map(|indices| indices.iter().map(|&index| &self.policies[index]).collect())
            .unwrap_or_default()
    }

    /// Get policies that match a subject, resource, and action
    pub fn get_matching_policies_with_action(
        &self,
        subject: &str,
        resource: &str,
        resource_action: &ResourceAction,
    ) -> Vec<&Policy> {
        self.candidate_indices(subject, resource)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&index| {
                        let policy = &self.policies[index];
                        if self
                            .evaluator
                            .policy_action_matches(policy, Some(resource_action))
                        {
                            Some(policy)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Load policies from multiple sources
    pub fn load_from_sources(&mut self, sources: Vec<&str>) -> Result<(), ParseError> {
        for source in sources {
            self.parse_file(source)?;
        }
        Ok(())
    }

    /// Export policies as formatted strings
    pub fn export_policies(&self) -> Vec<String> {
        self.policies.iter().map(|p| format!("{}", p)).collect()
    }

    fn index_policy(&mut self, policy_index: usize) {
        let policy = &self.policies[policy_index];
        self.policy_index
            .entry(policy.subject.clone())
            .or_default()
            .entry(policy.resource.clone())
            .or_default()
            .push(policy_index);
    }

    fn candidate_indices(&self, subject: &str, resource: &str) -> Option<&[usize]> {
        self.policy_index
            .get(subject)
            .and_then(|resources| resources.get(resource))
            .map(Vec::as_slice)
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to create a context map
pub fn context() -> HashMap<String, Value> {
    HashMap::new()
}

/// Convenience function to create a context with initial values
pub fn context_with(pairs: Vec<(&str, Value)>) -> HashMap<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Context builder for creating rich context with location, time, etc.
pub struct ContextBuilder {
    context: HashMap<String, Value>,
}

impl ContextBuilder {
    pub fn new() -> Self {
        Self {
            context: HashMap::new(),
        }
    }

    pub fn user_role(mut self, role: &str) -> Self {
        self.context
            .insert("user.role".to_string(), Value::String(role.to_string()));
        self
    }

    pub fn user_department(mut self, department: &str) -> Self {
        self.context.insert(
            "user.department".to_string(),
            Value::String(department.to_string()),
        );
        self
    }

    pub fn location(mut self, location: &str) -> Self {
        self.context
            .insert("location".to_string(), Value::String(location.to_string()));
        self
    }

    pub fn time_of_day(mut self, time: &str) -> Self {
        self.context
            .insert("time.of_day".to_string(), Value::String(time.to_string()));
        self
    }

    pub fn date(mut self, date: &str) -> Self {
        let value = parse_iso_datetime(date)
            .map(Value::DateTime)
            .or_else(|| parse_iso_date(date).map(Value::Date))
            .unwrap_or_else(|| Value::String(date.to_string()));
        self.context.insert("time.date".to_string(), value);
        self
    }

    pub fn time(mut self, time: &str) -> Self {
        let value = parse_time_with_offset(time)
            .map(Value::DateTime)
            .or_else(|| parse_time_of_day(time).map(Value::Time))
            .unwrap_or_else(|| Value::String(time.to_string()));
        self.context.insert("time.time".to_string(), value);
        self
    }

    pub fn datetime(mut self, datetime: &str) -> Self {
        let value = parse_iso_datetime(datetime)
            .map(Value::DateTime)
            .unwrap_or_else(|| Value::String(datetime.to_string()));
        self.context.insert("time.datetime".to_string(), value);
        self
    }

    pub fn day_of_week(mut self, day: &str) -> Self {
        self.context.insert(
            "time.day_of_week".to_string(),
            Value::String(day.to_string()),
        );
        self
    }

    pub fn ip_address(mut self, ip: &str) -> Self {
        self.context
            .insert("request.ip".to_string(), Value::String(ip.to_string()));
        self
    }

    pub fn device_type(mut self, device: &str) -> Self {
        self.context
            .insert("device.type".to_string(), Value::String(device.to_string()));
        self
    }

    pub fn security_level(mut self, level: i64) -> Self {
        self.context
            .insert("security.level".to_string(), Value::Number(level));
        self
    }

    pub fn add(mut self, key: &str, value: Value) -> Self {
        self.context.insert(key.to_string(), value);
        self
    }

    pub fn add_array(mut self, key: &str, values: Vec<Value>) -> Self {
        self.context.insert(key.to_string(), Value::Array(values));
        self
    }

    pub fn build(self) -> HashMap<String, Value> {
        self.context
    }
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_engine_full_workflow() {
        let mut engine = PolicyEngine::new();

        let content = r#"
        ALLOW user FOR "feature1" WHEN user.role == "admin" OR user.role == "editor";
        DENY user FOR "feature2" WHEN user.role == "guest";
        ALLOW api_key FOR "feature3" WHEN api_key.valid == true AND api_key.scope == "read";
        DENY api_key FOR "feature4" WHEN api_key.valid == false OR api_key.scope != "write";
        "#;

        engine.parse_file(content).unwrap();
        let policies = engine.get_policies();
        assert_eq!(policies.len(), 4);

        // Test user with admin role
        let mut context = context_with(vec![("user.role", Value::String("admin".to_string()))]);
        assert!(engine.evaluate("user", "feature1", &context));
        assert!(!engine.evaluate("user", "feature2", &context));

        // Test user with guest role
        context = context_with(vec![("user.role", Value::String("guest".to_string()))]);
        assert!(!engine.evaluate("user", "feature1", &context));
        assert!(!engine.evaluate("user", "feature2", &context)); // Explicit deny

        // Test API key
        context = context_with(vec![
            ("api_key.valid", Value::Boolean(true)),
            ("api_key.scope", Value::String("read".to_string())),
        ]);
        assert!(engine.evaluate("api_key", "feature3", &context));
        assert!(!engine.evaluate("api_key", "feature4", &context)); // scope != "write"
    }

    #[test]
    fn test_policy_display() {
        let policy = Policy::new(
            PolicyAction::Allow,
            "user".to_string(),
            "feature1".to_string(),
        )
        .with_condition(Condition::Expression(Expression {
            left: "user.role".to_string(),
            operator: Operator::Equal,
            right: Value::String("admin".to_string()),
        }));

        let display = format!("{}", policy);
        assert!(display.contains("ALLOW user FOR \"feature1\" WHEN user.role == \"admin\";"));
    }

    #[test]
    fn test_context_helpers() {
        let ctx = context();
        assert!(ctx.is_empty());

        let ctx = context_with(vec![
            ("user.role", Value::String("admin".to_string())),
            ("user.active", Value::Boolean(true)),
        ]);
        assert_eq!(ctx.len(), 2);
        assert_eq!(
            ctx.get("user.role"),
            Some(&Value::String("admin".to_string()))
        );
        assert_eq!(ctx.get("user.active"), Some(&Value::Boolean(true)));
    }

    #[test]
    fn test_export_policies() {
        let mut engine = PolicyEngine::new();
        engine.add_policy(Policy::new(
            PolicyAction::Allow,
            "user".to_string(),
            "test".to_string(),
        ));

        let exported = engine.export_policies();
        assert_eq!(exported.len(), 1);
        assert!(exported[0].contains("ALLOW user FOR \"test\";"));
    }

    #[test]
    fn test_multiple_sources() {
        let mut engine = PolicyEngine::new();

        let source1 = "ALLOW user FOR \"feature1\";";
        let source2 = "DENY user FOR \"feature2\";";

        engine.load_from_sources(vec![source1, source2]).unwrap();

        assert_eq!(engine.get_policies().len(), 2);
    }

    #[test]
    fn test_evaluation_details() {
        let mut engine = PolicyEngine::new();
        engine.add_policy(Policy::new(
            PolicyAction::Allow,
            "user".to_string(),
            "test".to_string(),
        ));

        let context = context();
        let result = engine.evaluate_with_details("user", "test", &context);

        assert!(result.is_allowed());
        assert_eq!(result.allow_count, 1);
        assert_eq!(result.deny_count, 0);
        assert!(result.has_explicit_decision());
    }

    #[test]
    fn test_array_contains_full_workflow() {
        let mut engine = PolicyEngine::new();

        let content = r#"
        ALLOW user FOR "admin_panel" WHEN user.roles CONTAINS "admin";
        DENY user FOR "feature1" WHEN user.tags CONTAINS "banned";
        ALLOW user FOR "internal" WHEN user.email CONTAINS "@company.com";
        ALLOW user FOR "verified_feature" WHEN NOT user.tags CONTAINS "unverified";
        "#;

        engine.parse_file(content).unwrap();
        assert_eq!(engine.get_policies().len(), 4);

        // Test array contains - user has admin role
        let ctx = context_with(vec![(
            "user.roles",
            Value::Array(vec![
                Value::String("editor".to_string()),
                Value::String("admin".to_string()),
            ]),
        )]);
        assert!(engine.evaluate("user", "admin_panel", &ctx));

        // Test array contains - user does NOT have admin role
        let ctx = context_with(vec![(
            "user.roles",
            Value::Array(vec![Value::String("viewer".to_string())]),
        )]);
        assert!(!engine.evaluate("user", "admin_panel", &ctx));

        // Test array contains deny - user has "banned" tag
        let ctx = context_with(vec![(
            "user.tags",
            Value::Array(vec![
                Value::String("active".to_string()),
                Value::String("banned".to_string()),
            ]),
        )]);
        assert!(!engine.evaluate("user", "feature1", &ctx));

        // Test string contains
        let ctx = context_with(vec![(
            "user.email",
            Value::String("alice@company.com".to_string()),
        )]);
        assert!(engine.evaluate("user", "internal", &ctx));

        // Test NOT_CONTAINS - user without "unverified" tag
        let ctx = context_with(vec![(
            "user.tags",
            Value::Array(vec![
                Value::String("active".to_string()),
                Value::String("verified".to_string()),
            ]),
        )]);
        assert!(engine.evaluate("user", "verified_feature", &ctx));

        // Test NOT_CONTAINS - user with "unverified" tag
        let ctx = context_with(vec![(
            "user.tags",
            Value::Array(vec![Value::String("unverified".to_string())]),
        )]);
        assert!(!engine.evaluate("user", "verified_feature", &ctx));
    }

    #[test]
    fn test_time_with_timezone_policy() {
        let mut engine = PolicyEngine::new();

        let content = r#"
        DENY user FOR "feature0" WHEN env.time < "09:00+02:00" OR env.time > "17:00+02:00";
        "#;

        engine.parse_file(content).unwrap();

        // 10:00+02:00 is within working hours -> not denied (but no ALLOW, so false)
        let ctx = context_with(vec![(
            "env.time",
            Value::DateTime(parse_time_with_offset("10:00+02:00").unwrap()),
        )]);
        assert!(!engine.evaluate("user", "feature0", &ctx));

        // 08:00+02:00 is before 09:00+02:00 -> denied
        let ctx = context_with(vec![(
            "env.time",
            Value::DateTime(parse_time_with_offset("08:00+02:00").unwrap()),
        )]);
        assert!(!engine.evaluate("user", "feature0", &ctx));
    }

    #[test]
    fn test_time_with_timezone_allow_and_deny() {
        let mut engine = PolicyEngine::new();

        let content = r#"
        ALLOW user FOR "feature0";
        DENY user FOR "feature0" WHEN env.time < "09:00+02:00" OR env.time > "17:00+02:00";
        "#;

        engine.parse_file(content).unwrap();

        // 10:00+02:00 is within working hours -> allowed
        let ctx = context_with(vec![(
            "env.time",
            Value::DateTime(parse_time_with_offset("10:00+02:00").unwrap()),
        )]);
        assert!(engine.evaluate("user", "feature0", &ctx));

        // 08:00+02:00 is before 09:00+02:00 -> denied
        let ctx = context_with(vec![(
            "env.time",
            Value::DateTime(parse_time_with_offset("08:00+02:00").unwrap()),
        )]);
        assert!(!engine.evaluate("user", "feature0", &ctx));

        // 18:00+02:00 is after 17:00+02:00 -> denied
        let ctx = context_with(vec![(
            "env.time",
            Value::DateTime(parse_time_with_offset("18:00+02:00").unwrap()),
        )]);
        assert!(!engine.evaluate("user", "feature0", &ctx));
    }

    #[test]
    fn test_datetime_with_timezone_comparison() {
        let mut engine = PolicyEngine::new();

        let content = r#"
        ALLOW user FOR "feature1" WHEN time.datetime >= "2026-01-01T00:00:00+00:00";
        "#;

        engine.parse_file(content).unwrap();

        // After the date -> allowed
        let ctx = context_with(vec![(
            "time.datetime",
            Value::DateTime(parse_iso_datetime("2026-06-15T12:00:00+00:00").unwrap()),
        )]);
        assert!(engine.evaluate("user", "feature1", &ctx));

        // Before the date -> denied
        let ctx = context_with(vec![(
            "time.datetime",
            Value::DateTime(parse_iso_datetime("2025-12-31T23:59:59+00:00").unwrap()),
        )]);
        assert!(!engine.evaluate("user", "feature1", &ctx));
    }

    #[test]
    fn test_timezone_cross_offset_comparison() {
        let mut engine = PolicyEngine::new();

        // Policy uses UTC
        let content = r#"
        ALLOW user FOR "feature1" WHEN env.time > "07:00+00:00";
        "#;

        engine.parse_file(content).unwrap();

        // 09:00+02:00 == 07:00 UTC -> not greater, should be denied
        let ctx = context_with(vec![(
            "env.time",
            Value::DateTime(parse_time_with_offset("09:00+02:00").unwrap()),
        )]);
        assert!(!engine.evaluate("user", "feature1", &ctx));

        // 10:00+02:00 == 08:00 UTC -> greater than 07:00 UTC, allowed
        let ctx = context_with(vec![(
            "env.time",
            Value::DateTime(parse_time_with_offset("10:00+02:00").unwrap()),
        )]);
        assert!(engine.evaluate("user", "feature1", &ctx));
    }

    #[test]
    fn test_time_with_utc_z_suffix() {
        let dt = parse_time_with_offset("09:00:00Z").unwrap();
        assert_eq!(dt, parse_time_with_offset("09:00:00+00:00").unwrap());

        let dt = parse_time_with_offset("17:00Z").unwrap();
        assert_eq!(dt, parse_time_with_offset("17:00+00:00").unwrap());
    }

    #[test]
    fn test_parse_iso_datetime_formats() {
        // RFC 3339 with seconds
        assert!(parse_iso_datetime("2026-01-01T09:00:00Z").is_some());
        assert!(parse_iso_datetime("2026-01-01T09:00:00+02:00").is_some());

        // Without seconds
        assert!(parse_iso_datetime("2026-01-01T09:00+02:00").is_some());
        assert!(parse_iso_datetime("2026-01-01T09:00Z").is_some());

        // Not datetime
        assert!(parse_iso_datetime("2026-01-01").is_none());
        assert!(parse_iso_datetime("09:00").is_none());
        assert!(parse_iso_datetime("hello").is_none());
    }

    #[test]
    fn test_context_builder_datetime() {
        let ctx = ContextBuilder::new()
            .datetime("2026-01-01T09:00:00+02:00")
            .build();

        assert!(matches!(ctx.get("time.datetime"), Some(Value::DateTime(_))));
    }

    #[test]
    fn test_context_builder_time_with_offset() {
        let ctx = ContextBuilder::new().time("09:00+02:00").build();

        assert!(matches!(ctx.get("time.time"), Some(Value::DateTime(_))));
    }

    #[test]
    fn test_context_builder_time_without_offset() {
        let ctx = ContextBuilder::new().time("09:00").build();

        assert!(matches!(ctx.get("time.time"), Some(Value::Time(_))));
    }

    #[test]
    fn test_context_builder_add_array() {
        let ctx = ContextBuilder::new()
            .user_role("admin")
            .add_array(
                "user.permissions",
                vec![
                    Value::String("read".to_string()),
                    Value::String("write".to_string()),
                ],
            )
            .build();

        assert_eq!(
            ctx.get("user.permissions"),
            Some(&Value::Array(vec![
                Value::String("read".to_string()),
                Value::String("write".to_string()),
            ]))
        );
    }
}
