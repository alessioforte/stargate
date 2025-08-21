use crate::{Condition, Expression, Operator, Policy, PolicyAction, ResourceAction, Value};
use std::collections::HashMap;

pub struct PolicyEvaluator;

impl PolicyEvaluator {
    pub fn new() -> Self {
        Self
    }

    /// Evaluate a list of policies against a subject, resource, and context
    /// Returns true if access is allowed, false if denied
    pub fn evaluate(
        &self,
        policies: &[Policy],
        subject: &str,
        resource: &str,
        resource_action: Option<&ResourceAction>,
        context: &HashMap<String, Value>,
    ) -> bool {
        let mut explicit_allow = false;
        let mut explicit_deny = false;

        // Process policies in order
        for policy in policies {
            if self.policy_matches(policy, subject, resource, resource_action) {
                if let Some(ref condition) = policy.condition {
                    if self.evaluate_condition(condition, context) {
                        match policy.action {
                            PolicyAction::Allow => explicit_allow = true,
                            PolicyAction::Deny => explicit_deny = true,
                        }
                    }
                } else {
                    // No condition means the policy applies unconditionally
                    match policy.action {
                        PolicyAction::Allow => explicit_allow = true,
                        PolicyAction::Deny => explicit_deny = true,
                    }
                }
            }
        }

        // Deny takes precedence over allow
        if explicit_deny {
            false
        } else {
            explicit_allow
        }
    }

    /// Check if a policy matches the given subject, resource, and action
    fn policy_matches(
        &self,
        policy: &Policy,
        subject: &str,
        resource: &str,
        resource_action: Option<&ResourceAction>,
    ) -> bool {
        let subject_matches = policy.subject == subject;
        let resource_matches = policy.resource == resource;

        let action_matches = match (&policy.resource_action, resource_action) {
            (None, _) => true,                      // Policy without action matches any action
            (Some(ResourceAction::Any), _) => true, // Policy with ANY action matches any action
            (Some(policy_action), Some(requested_action)) => policy_action == requested_action,
            (Some(_), None) => false, // Policy with specific action doesn't match request without action
        };

        subject_matches && resource_matches && action_matches
    }

    /// Evaluate a condition against the provided context
    pub fn evaluate_condition(
        &self,
        condition: &Condition,
        context: &HashMap<String, Value>,
    ) -> bool {
        match condition {
            Condition::Expression(expr) => self.evaluate_expression(expr, context),
            Condition::And(left, right) => {
                self.evaluate_condition(left, context) && self.evaluate_condition(right, context)
            }
            Condition::Or(left, right) => {
                self.evaluate_condition(left, context) || self.evaluate_condition(right, context)
            }
            Condition::Not(condition) => !self.evaluate_condition(condition, context),
        }
    }

    /// Evaluate a single expression against the provided context
    fn evaluate_expression(&self, expr: &Expression, context: &HashMap<String, Value>) -> bool {
        if let Some(context_value) = context.get(&expr.left) {
            match expr.operator {
                Operator::Equal => context_value == &expr.right,
                Operator::NotEqual => context_value != &expr.right,
                Operator::GreaterThan => {
                    self.compare_values(context_value, &expr.right, |a, b| a > b)
                }
                Operator::LessThan => self.compare_values(context_value, &expr.right, |a, b| a < b),
                Operator::GreaterThanOrEqual => {
                    self.compare_values(context_value, &expr.right, |a, b| a >= b)
                }
                Operator::LessThanOrEqual => {
                    self.compare_values(context_value, &expr.right, |a, b| a <= b)
                }
            }
        } else {
            // If the context key doesn't exist, the expression is false
            false
        }
    }

    /// Helper method to compare values for numeric comparisons
    fn compare_values<F>(&self, left: &Value, right: &Value, op: F) -> bool
    where
        F: Fn(f64, f64) -> bool,
    {
        match (left, right) {
            (Value::Number(l), Value::Number(r)) => op(*l as f64, *r as f64),
            (Value::Number(l), Value::Float(r)) => op(*l as f64, *r),
            (Value::Float(l), Value::Number(r)) => op(*l, *r as f64),
            (Value::Float(l), Value::Float(r)) => op(*l, *r),
            // For non-numeric values, fall back to partial ordering
            _ => match left.partial_cmp(right) {
                Some(std::cmp::Ordering::Greater) => op(1.0, 0.0),
                Some(std::cmp::Ordering::Less) => op(0.0, 1.0),
                Some(std::cmp::Ordering::Equal) => op(0.0, 0.0),
                None => false,
            },
        }
    }

    /// Get all matching policies for a subject and resource
    pub fn get_matching_policies<'a>(
        &self,
        policies: &'a [Policy],
        subject: &str,
        resource: &str,
        resource_action: Option<&ResourceAction>,
    ) -> Vec<&'a Policy> {
        policies
            .iter()
            .filter(|policy| self.policy_matches(policy, subject, resource, resource_action))
            .collect()
    }

    /// Evaluate with detailed results showing which policies matched
    pub fn evaluate_with_details(
        &self,
        policies: &[Policy],
        subject: &str,
        resource: &str,
        resource_action: Option<&ResourceAction>,
        context: &HashMap<String, Value>,
    ) -> EvaluationResult {
        let mut matched_policies = Vec::new();
        let mut applied_policies = Vec::new();
        let mut allow_count = 0;
        let mut deny_count = 0;

        for (index, policy) in policies.iter().enumerate() {
            if self.policy_matches(policy, subject, resource, resource_action) {
                matched_policies.push(index);

                let condition_result = if let Some(ref condition) = policy.condition {
                    self.evaluate_condition(condition, context)
                } else {
                    true
                };

                if condition_result {
                    applied_policies.push(index);
                    match policy.action {
                        PolicyAction::Allow => allow_count += 1,
                        PolicyAction::Deny => deny_count += 1,
                    }
                }
            }
        }

        let final_decision = if deny_count > 0 {
            false
        } else {
            allow_count > 0
        };

        EvaluationResult {
            decision: final_decision,
            matched_policies,
            applied_policies,
            allow_count,
            deny_count,
        }
    }
}

impl Default for PolicyEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Detailed result of policy evaluation
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationResult {
    pub decision: bool,
    pub matched_policies: Vec<usize>,
    pub applied_policies: Vec<usize>,
    pub allow_count: usize,
    pub deny_count: usize,
}

impl EvaluationResult {
    pub fn is_allowed(&self) -> bool {
        self.decision
    }

    pub fn is_denied(&self) -> bool {
        !self.decision
    }

    pub fn has_explicit_decision(&self) -> bool {
        self.allow_count > 0 || self.deny_count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Expression, Operator, Policy, PolicyAction, Value};

    fn create_test_policy(
        action: PolicyAction,
        subject: &str,
        resource: &str,
        condition: Option<Condition>,
    ) -> Policy {
        Policy {
            action,
            subject: subject.to_string(),
            resource: resource.to_string(),
            resource_action: None,
            condition,
        }
    }

    #[test]
    fn test_simple_allow_policy() {
        let evaluator = PolicyEvaluator::new();
        let policies = vec![create_test_policy(
            PolicyAction::Allow,
            "user",
            "feature1",
            Some(Condition::Expression(Expression {
                left: "user.role".to_string(),
                operator: Operator::Equal,
                right: Value::String("admin".to_string()),
            })),
        )];

        let mut context = HashMap::new();
        context.insert("user.role".to_string(), Value::String("admin".to_string()));

        assert!(evaluator.evaluate(&policies, "user", "feature1", None, &context));

        context.insert("user.role".to_string(), Value::String("guest".to_string()));
        assert!(!evaluator.evaluate(&policies, "user", "feature1", None, &context));
    }

    #[test]
    fn test_deny_overrides_allow() {
        let evaluator = PolicyEvaluator::new();
        let policies = vec![
            create_test_policy(
                PolicyAction::Allow,
                "user",
                "feature1",
                Some(Condition::Expression(Expression {
                    left: "user.role".to_string(),
                    operator: Operator::Equal,
                    right: Value::String("admin".to_string()),
                })),
            ),
            create_test_policy(
                PolicyAction::Deny,
                "user",
                "feature1",
                Some(Condition::Expression(Expression {
                    left: "user.blocked".to_string(),
                    operator: Operator::Equal,
                    right: Value::Boolean(true),
                })),
            ),
        ];

        let mut context = HashMap::new();
        context.insert("user.role".to_string(), Value::String("admin".to_string()));
        context.insert("user.blocked".to_string(), Value::Boolean(true));

        // Deny should override allow
        assert!(!evaluator.evaluate(&policies, "user", "feature1", None, &context));
    }

    #[test]
    fn test_or_condition() {
        let evaluator = PolicyEvaluator::new();
        let policies = vec![create_test_policy(
            PolicyAction::Allow,
            "user",
            "feature1",
            Some(Condition::Or(
                Box::new(Condition::Expression(Expression {
                    left: "user.role".to_string(),
                    operator: Operator::Equal,
                    right: Value::String("admin".to_string()),
                })),
                Box::new(Condition::Expression(Expression {
                    left: "user.role".to_string(),
                    operator: Operator::Equal,
                    right: Value::String("editor".to_string()),
                })),
            )),
        )];

        let mut context = HashMap::new();

        // Test admin role
        context.insert("user.role".to_string(), Value::String("admin".to_string()));
        assert!(evaluator.evaluate(&policies, "user", "feature1", None, &context));

        // Test editor role
        context.insert("user.role".to_string(), Value::String("editor".to_string()));
        assert!(evaluator.evaluate(&policies, "user", "feature1", None, &context));

        // Test guest role (should be denied)
        context.insert("user.role".to_string(), Value::String("guest".to_string()));
        assert!(!evaluator.evaluate(&policies, "user", "feature1", None, &context));
    }

    #[test]
    fn test_and_condition() {
        let evaluator = PolicyEvaluator::new();
        let policies = vec![create_test_policy(
            PolicyAction::Allow,
            "api_key",
            "feature3",
            Some(Condition::And(
                Box::new(Condition::Expression(Expression {
                    left: "api_key.valid".to_string(),
                    operator: Operator::Equal,
                    right: Value::Boolean(true),
                })),
                Box::new(Condition::Expression(Expression {
                    left: "api_key.scope".to_string(),
                    operator: Operator::Equal,
                    right: Value::String("read".to_string()),
                })),
            )),
        )];

        let mut context = HashMap::new();

        // Both conditions true
        context.insert("api_key.valid".to_string(), Value::Boolean(true));
        context.insert(
            "api_key.scope".to_string(),
            Value::String("read".to_string()),
        );
        assert!(evaluator.evaluate(&policies, "api_key", "feature3", None, &context));

        // Only first condition true
        context.insert("api_key.valid".to_string(), Value::Boolean(true));
        context.insert(
            "api_key.scope".to_string(),
            Value::String("write".to_string()),
        );
        assert!(!evaluator.evaluate(&policies, "api_key", "feature3", None, &context));

        // Only second condition true
        context.insert("api_key.valid".to_string(), Value::Boolean(false));
        context.insert(
            "api_key.scope".to_string(),
            Value::String("read".to_string()),
        );
        assert!(!evaluator.evaluate(&policies, "api_key", "feature3", None, &context));
    }

    #[test]
    fn test_not_equal_operator() {
        let evaluator = PolicyEvaluator::new();
        let policies = vec![create_test_policy(
            PolicyAction::Deny,
            "api_key",
            "feature4",
            Some(Condition::Expression(Expression {
                left: "api_key.scope".to_string(),
                operator: Operator::NotEqual,
                right: Value::String("write".to_string()),
            })),
        )];

        let mut context = HashMap::new();

        // api_key.scope != "write" should trigger deny
        context.insert(
            "api_key.scope".to_string(),
            Value::String("read".to_string()),
        );
        assert!(!evaluator.evaluate(&policies, "api_key", "feature4", None, &context));

        // api_key.scope == "write" should not trigger deny (default allow would be false though)
        context.insert(
            "api_key.scope".to_string(),
            Value::String("write".to_string()),
        );
        assert!(!evaluator.evaluate(&policies, "api_key", "feature4", None, &context));
        // Still false due to default deny
    }

    #[test]
    fn test_evaluation_details() {
        let evaluator = PolicyEvaluator::new();
        let policies = vec![
            create_test_policy(PolicyAction::Allow, "user", "feature1", None),
            create_test_policy(
                PolicyAction::Deny,
                "user",
                "feature1",
                Some(Condition::Expression(Expression {
                    left: "user.blocked".to_string(),
                    operator: Operator::Equal,
                    right: Value::Boolean(true),
                })),
            ),
            create_test_policy(PolicyAction::Allow, "user", "feature2", None),
        ];

        let mut context = HashMap::new();
        context.insert("user.blocked".to_string(), Value::Boolean(false));

        let result = evaluator.evaluate_with_details(&policies, "user", "feature1", None, &context);

        assert_eq!(result.matched_policies, vec![0, 1]);
        assert_eq!(result.applied_policies, vec![0]); // Only first policy applies (second condition is false)
        assert_eq!(result.allow_count, 1);
        assert_eq!(result.deny_count, 0);
        assert!(result.is_allowed());
    }

    #[test]
    fn test_missing_context_key() {
        let evaluator = PolicyEvaluator::new();
        let policies = vec![create_test_policy(
            PolicyAction::Allow,
            "user",
            "feature1",
            Some(Condition::Expression(Expression {
                left: "user.missing_key".to_string(),
                operator: Operator::Equal,
                right: Value::String("value".to_string()),
            })),
        )];

        let context = HashMap::new(); // Empty context

        // Should return false because the context key doesn't exist
        assert!(!evaluator.evaluate(&policies, "user", "feature1", None, &context));
    }

    #[test]
    fn test_not_condition() {
        let evaluator = PolicyEvaluator::new();
        let policies = vec![create_test_policy(
            PolicyAction::Allow,
            "user",
            "feature1",
            Some(Condition::Not(Box::new(Condition::Expression(
                Expression {
                    left: "user.suspended".to_string(),
                    operator: Operator::Equal,
                    right: Value::Boolean(true),
                },
            )))),
        )];

        let mut context = HashMap::new();

        // user.suspended == false, so NOT (user.suspended == true) should be true
        context.insert("user.suspended".to_string(), Value::Boolean(false));
        assert!(evaluator.evaluate(&policies, "user", "feature1", None, &context));

        // user.suspended == true, so NOT (user.suspended == true) should be false
        context.insert("user.suspended".to_string(), Value::Boolean(true));
        assert!(!evaluator.evaluate(&policies, "user", "feature1", None, &context));
    }

    #[test]
    fn test_comparison_operators() {
        let evaluator = PolicyEvaluator::new();
        let policies = vec![
            create_test_policy(
                PolicyAction::Allow,
                "user",
                "adult_content",
                Some(Condition::Expression(Expression {
                    left: "user.age".to_string(),
                    operator: Operator::GreaterThanOrEqual,
                    right: Value::Number(18),
                })),
            ),
            create_test_policy(
                PolicyAction::Allow,
                "user",
                "senior_discount",
                Some(Condition::Expression(Expression {
                    left: "user.age".to_string(),
                    operator: Operator::GreaterThan,
                    right: Value::Number(65),
                })),
            ),
            create_test_policy(
                PolicyAction::Allow,
                "user",
                "high_score",
                Some(Condition::Expression(Expression {
                    left: "user.score".to_string(),
                    operator: Operator::GreaterThanOrEqual,
                    right: Value::Float(95.5),
                })),
            ),
        ];

        let mut context = HashMap::new();

        // Test adult content (age >= 18)
        context.insert("user.age".to_string(), Value::Number(25));
        assert!(evaluator.evaluate(&policies, "user", "adult_content", None, &context));

        context.insert("user.age".to_string(), Value::Number(16));
        assert!(!evaluator.evaluate(&policies, "user", "adult_content", None, &context));

        // Test senior discount (age > 65)
        context.insert("user.age".to_string(), Value::Number(70));
        assert!(evaluator.evaluate(&policies, "user", "senior_discount", None, &context));

        context.insert("user.age".to_string(), Value::Number(65));
        assert!(!evaluator.evaluate(&policies, "user", "senior_discount", None, &context));

        // Test high score with float comparison
        context.insert("user.score".to_string(), Value::Float(96.0));
        assert!(evaluator.evaluate(&policies, "user", "high_score", None, &context));

        context.insert("user.score".to_string(), Value::Float(95.0));
        assert!(!evaluator.evaluate(&policies, "user", "high_score", None, &context));
    }

    #[test]
    fn test_resource_action_matching() {
        let evaluator = PolicyEvaluator::new();
        let mut read_policy = create_test_policy(PolicyAction::Allow, "user", "database", None);
        read_policy.resource_action = Some(ResourceAction::Read);

        let mut write_policy = create_test_policy(PolicyAction::Allow, "user", "database", None);
        write_policy.resource_action = Some(ResourceAction::Write);

        let policies = vec![read_policy, write_policy];
        let context = HashMap::new();

        // Test read access
        assert!(evaluator.evaluate(
            &policies,
            "user",
            "database",
            Some(&ResourceAction::Read),
            &context
        ));

        // Test write access
        assert!(evaluator.evaluate(
            &policies,
            "user",
            "database",
            Some(&ResourceAction::Write),
            &context
        ));

        // Test delete access (should be denied)
        assert!(!evaluator.evaluate(
            &policies,
            "user",
            "database",
            Some(&ResourceAction::Delete),
            &context
        ));
    }
}
