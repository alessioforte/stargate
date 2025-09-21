//! Access Control Engine (ACE) for Stargate
//!
//! This crate provides a policy-based access control system that parses
//! policy definitions from comments and evaluates them against runtime context.
//!
//! # Policy Syntax
//!
//! Policies are defined as comments with the following syntax:
//!
//! ```text
//! // ALLOW|DENY subject FOR "resource" [WHEN condition];
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
//! // ALLOW user FOR "feature1" WHEN user.role == "admin" OR user.role == "editor";
//! // DENY user FOR "feature2" WHEN user.role == "guest";
//! // ALLOW api_key FOR "feature3" WHEN api_key.valid == true AND api_key.scope == "read";
//! ```

use std::collections::HashMap;
use std::fmt;

mod evaluator;
mod parser;

pub use evaluator::{EvaluationResult, PolicyEvaluator};
pub use parser::PolicyParser;

/// Represents the action a policy should take
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
    pub fn from_str(s: &str) -> Option<Self> {
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
#[derive(Debug, Clone, PartialEq)]
pub enum Operator {
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
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
        }
    }
}

/// Represents a value in an expression
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Value {
    String(String),
    Boolean(bool),
    Number(i64),
    Float(f64),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::Float(f_val) => write!(f, "{}", f_val),
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
            _ => Value::String(v.to_string()),
        }
    }
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
    parser: PolicyParser,
    evaluator: PolicyEvaluator,
}

impl PolicyEngine {
    /// Create a new policy engine
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
            parser: PolicyParser::new(),
            evaluator: PolicyEvaluator::new(),
        }
    }

    /// Parse policy definitions from a string (typically file content)
    pub fn parse_file(&mut self, content: &str) -> Result<(), ParseError> {
        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and non-comment lines
            if line.is_empty() || !line.starts_with("//") {
                continue;
            }

            // Try to parse as policy, skip invalid lines
            if let Ok(policy) = self.parser.parse_line(line) {
                self.policies.push(policy);
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
        self.evaluator
            .evaluate(&self.policies, subject, resource, None, context)
    }

    /// Evaluate access for a subject and resource with specific action
    pub fn evaluate_with_action(
        &self,
        subject: &str,
        resource: &str,
        resource_action: &ResourceAction,
        context: &HashMap<String, Value>,
    ) -> bool {
        self.evaluator.evaluate(
            &self.policies,
            subject,
            resource,
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
        self.evaluator
            .evaluate_with_details(&self.policies, subject, resource, None, context)
    }

    /// Evaluate with detailed results for specific action
    pub fn evaluate_with_details_and_action(
        &self,
        subject: &str,
        resource: &str,
        resource_action: &ResourceAction,
        context: &HashMap<String, Value>,
    ) -> EvaluationResult {
        self.evaluator.evaluate_with_details(
            &self.policies,
            subject,
            resource,
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
    }

    /// Clear all policies
    pub fn clear_policies(&mut self) {
        self.policies.clear();
    }

    /// Get policies that match a subject and resource
    pub fn get_matching_policies(&self, subject: &str, resource: &str) -> Vec<&Policy> {
        self.evaluator
            .get_matching_policies(&self.policies, subject, resource, None)
    }

    /// Get policies that match a subject, resource, and action
    pub fn get_matching_policies_with_action(
        &self,
        subject: &str,
        resource: &str,
        resource_action: &ResourceAction,
    ) -> Vec<&Policy> {
        self.evaluator.get_matching_policies(
            &self.policies,
            subject,
            resource,
            Some(resource_action),
        )
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
        self.policies.iter().map(|p| format!("// {}", p)).collect()
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
        // ALLOW user FOR "feature1" WHEN user.role == "admin" OR user.role == "editor";
        // DENY user FOR "feature2" WHEN user.role == "guest";
        // ALLOW api_key FOR "feature3" WHEN api_key.valid == true AND api_key.scope == "read";
        // DENY api_key FOR "feature4" WHEN api_key.valid == false OR api_key.scope != "write";
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
        assert!(exported[0].contains("// ALLOW user FOR \"test\";"));
    }

    #[test]
    fn test_multiple_sources() {
        let mut engine = PolicyEngine::new();

        let source1 = "// ALLOW user FOR \"feature1\";";
        let source2 = "// DENY user FOR \"feature2\";";

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
}
