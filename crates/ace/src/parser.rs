use crate::{
    Condition, Expression, Operator, ParseError, Policy, PolicyAction, ResourceAction, Value,
    parse_iso_date, parse_time_of_day,
};

pub struct PolicyParser;

impl PolicyParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse_line(&self, line: &str) -> Result<Policy, ParseError> {
        let line = line.trim();

        if line.is_empty() {
            return Err(ParseError::InvalidSyntax("Empty policy".to_string()));
        }

        self.parse_policy(line)
    }

    fn parse_policy(&self, line: &str) -> Result<Policy, ParseError> {
        let tokens: Vec<&str> = line.split_whitespace().collect();

        if tokens.len() < 4 {
            return Err(ParseError::InvalidSyntax(
                "Policy must have at least action, subject, FOR, and resource".to_string(),
            ));
        }

        // Parse action
        let action = match tokens[0].to_uppercase().as_str() {
            "ALLOW" => PolicyAction::Allow,
            "DENY" => PolicyAction::Deny,
            _ => return Err(ParseError::UnexpectedToken(tokens[0].to_string())),
        };

        // Parse subject
        let subject = tokens[1].to_string();

        // Expect "FOR"
        if tokens[2].to_uppercase() != "FOR" {
            return Err(ParseError::MissingToken("FOR".to_string()));
        }

        // Parse resource and optional action (e.g., "resource:READ")
        let mut resource_token = tokens[3];
        let mut resource_action = None;

        // If there's no condition, the semicolon might be part of the resource token
        if tokens.len() <= 4 || tokens[4].to_uppercase() != "WHEN" {
            resource_token = resource_token.trim_end_matches(';');
        }

        // Remove quotes first to get the actual resource string
        let resource_content = resource_token.trim_matches('"');

        // Check if resource contains an action (resource:ACTION format)
        let resource_name = if let Some(colon_pos) = resource_content.find(':') {
            let (res_name, action_str) = resource_content.split_at(colon_pos);
            let action_str = &action_str[1..]; // Remove the colon
            resource_action = ResourceAction::from_str(action_str);
            res_name.to_string()
        } else {
            resource_content.to_string()
        };

        // Check if there's a WHEN clause first
        let condition = if tokens.len() > 4 {
            if tokens[4].to_uppercase() != "WHEN" {
                return Err(ParseError::MissingToken("WHEN".to_string()));
            }

            // Join the rest of the tokens and parse the condition
            let condition_str = tokens[5..].join(" ");
            Some(self.parse_condition(&condition_str)?)
        } else {
            None
        };

        Ok(Policy {
            action,
            subject,
            resource: resource_name,
            resource_action,
            condition,
        })
    }

    pub fn parse_condition(&self, condition_str: &str) -> Result<Condition, ParseError> {
        let condition_str = condition_str.trim_end_matches(';').trim();

        // Handle NOT conditions first (highest precedence)
        if condition_str.to_uppercase().starts_with("NOT ") {
            let inner_condition_str = condition_str[4..].trim();
            // If the NOT is followed by parentheses, remove them
            let inner_condition_str =
                if inner_condition_str.starts_with('(') && inner_condition_str.ends_with(')') {
                    &inner_condition_str[1..inner_condition_str.len() - 1]
                } else {
                    inner_condition_str
                };
            let inner_condition = self.parse_condition(inner_condition_str)?;
            return Ok(Condition::Not(Box::new(inner_condition)));
        }

        // Handle OR conditions (lower precedence)
        if let Some(or_pos) = self.find_logical_operator(condition_str, "OR") {
            let left_part = condition_str[..or_pos].trim();
            let right_part = condition_str[or_pos + 2..].trim();

            let left_condition = self.parse_condition(left_part)?;
            let right_condition = self.parse_condition(right_part)?;

            return Ok(Condition::Or(
                Box::new(left_condition),
                Box::new(right_condition),
            ));
        }

        // Handle AND conditions (higher precedence)
        if let Some(and_pos) = self.find_logical_operator(condition_str, "AND") {
            let left_part = condition_str[..and_pos].trim();
            let right_part = condition_str[and_pos + 3..].trim();

            let left_condition = self.parse_condition(left_part)?;
            let right_condition = self.parse_condition(right_part)?;

            return Ok(Condition::And(
                Box::new(left_condition),
                Box::new(right_condition),
            ));
        }

        // Parse as a simple expression
        let expression = self.parse_expression(condition_str)?;
        Ok(Condition::Expression(expression))
    }

    fn find_logical_operator(&self, s: &str, op: &str) -> Option<usize> {
        let mut depth = 0;
        let chars: Vec<char> = s.chars().collect();
        let op_chars: Vec<char> = op.chars().collect();

        for i in 0..chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {
                    if depth == 0 && i + op_chars.len() <= chars.len() {
                        let slice: String = chars[i..i + op_chars.len()].iter().collect();
                        if slice == op {
                            // Check if it's a word boundary
                            let prev_ok = i == 0 || !chars[i - 1].is_alphanumeric();
                            let next_ok = i + op_chars.len() >= chars.len()
                                || !chars[i + op_chars.len()].is_alphanumeric();
                            if prev_ok && next_ok {
                                return Some(i);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn parse_expression(&self, expr_str: &str) -> Result<Expression, ParseError> {
        // Check for keyword operators first (case-insensitive, longest first)
        let keyword_operators = vec![("CONTAINS", Operator::Contains)];

        let upper = expr_str.to_uppercase();
        for (op_str, operator) in keyword_operators {
            if let Some(pos) = upper.find(&format!(" {} ", op_str)) {
                let left = expr_str[..pos].trim().to_string();
                let right_str = expr_str[pos + op_str.len() + 2..].trim();
                let right = self.parse_value(right_str)?;

                return Ok(Expression {
                    left,
                    operator,
                    right,
                });
            }
        }

        // Check for symbolic operators in order of precedence (longest first to avoid conflicts)
        let operators = vec![
            (" >= ", Operator::GreaterThanOrEqual),
            (" <= ", Operator::LessThanOrEqual),
            (" == ", Operator::Equal),
            (" != ", Operator::NotEqual),
            (" > ", Operator::GreaterThan),
            (" < ", Operator::LessThan),
        ];

        for (op_str, operator) in operators {
            if let Some(pos) = expr_str.find(op_str) {
                let left = expr_str[..pos].trim().to_string();
                let right_str = expr_str[pos + op_str.len()..].trim();
                let right = self.parse_value(right_str)?;

                return Ok(Expression {
                    left,
                    operator,
                    right,
                });
            }
        }

        Err(ParseError::InvalidSyntax(format!(
            "Invalid expression: {}",
            expr_str
        )))
    }

    fn parse_value(&self, value_str: &str) -> Result<Value, ParseError> {
        let value_str = value_str.trim();

        // String value (quoted)
        if value_str.starts_with('"') && value_str.ends_with('"') {
            let content = &value_str[1..value_str.len() - 1];
            if let Some(date) = parse_iso_date(content) {
                return Ok(Value::Date(date));
            }
            if let Some(time) = parse_time_of_day(content) {
                return Ok(Value::Time(time));
            }
            return Ok(Value::String(content.to_string()));
        }

        // Single quoted strings
        if value_str.starts_with('\'') && value_str.ends_with('\'') {
            let content = &value_str[1..value_str.len() - 1];
            if let Some(date) = parse_iso_date(content) {
                return Ok(Value::Date(date));
            }
            if let Some(time) = parse_time_of_day(content) {
                return Ok(Value::Time(time));
            }
            return Ok(Value::String(content.to_string()));
        }

        // Boolean values
        if value_str == "true" {
            return Ok(Value::Boolean(true));
        }
        if value_str == "false" {
            return Ok(Value::Boolean(false));
        }

        // Date and time values (ISO-8601 date, HH:MM[:SS] time)
        if let Some(date) = parse_iso_date(value_str) {
            return Ok(Value::Date(date));
        }
        if let Some(time) = parse_time_of_day(value_str) {
            return Ok(Value::Time(time));
        }

        // Number value (try integer first, then float)
        if let Ok(num) = value_str.parse::<i64>() {
            return Ok(Value::Number(num));
        }

        if let Ok(num) = value_str.parse::<f64>() {
            return Ok(Value::Float(num));
        }

        Err(ParseError::InvalidValue(value_str.to_string()))
    }
}

impl Default for PolicyParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_policy() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"ALLOW user FOR "feature1" WHEN user.role == "admin";"#)
            .unwrap();

        assert_eq!(policy.action, PolicyAction::Allow);
        assert_eq!(policy.subject, "user");
        assert_eq!(policy.resource, "feature1");
        assert!(policy.condition.is_some());
        assert!(policy.resource_action.is_none());
    }

    #[test]
    fn test_parse_complex_condition() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(
                r#"ALLOW user FOR "feature1" WHEN user.role == "admin" OR user.role == "editor";"#,
            )
            .unwrap();

        if let Some(Condition::Or(_, _)) = &policy.condition {
            // Success - it's an OR condition
        } else {
            panic!("Expected OR condition");
        }
    }

    #[test]
    fn test_parse_and_condition() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"ALLOW api_key FOR "feature3" WHEN api_key.valid == true AND api_key.scope == "read";"#)
            .unwrap();

        if let Some(Condition::And(_, _)) = &policy.condition {
            // Success - it's an AND condition
        } else {
            panic!("Expected AND condition");
        }
    }

    #[test]
    fn test_parse_boolean_values() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"ALLOW api_key FOR "feature3" WHEN api_key.valid == true;"#)
            .unwrap();

        if let Some(Condition::Expression(expr)) = &policy.condition {
            assert_eq!(expr.right, Value::Boolean(true));
        } else {
            panic!("Expected expression condition");
        }
    }

    #[test]
    fn test_parse_not_equal_operator() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"DENY api_key FOR "feature4" WHEN api_key.scope != "write";"#)
            .unwrap();

        if let Some(Condition::Expression(expr)) = &policy.condition {
            assert_eq!(expr.operator, Operator::NotEqual);
            assert_eq!(expr.right, Value::String("write".to_string()));
        } else {
            panic!("Expected expression condition");
        }
    }

    #[test]
    fn test_parse_without_condition() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"ALLOW user FOR "public_feature";"#)
            .unwrap();

        assert_eq!(policy.action, PolicyAction::Allow);
        assert_eq!(policy.subject, "user");
        assert_eq!(policy.resource, "public_feature");
        assert!(policy.condition.is_none());
    }

    #[test]
    fn test_invalid_syntax() {
        let parser = PolicyParser::new();

        // Missing FOR keyword
        let result = parser.parse_line(r#"ALLOW user "feature1";"#);
        assert!(result.is_err());

        // Invalid action
        let result = parser.parse_line(r#"INVALID user FOR "feature1";"#);
        assert!(result.is_err());

        // Empty policy
        let result = parser.parse_line(r#""#);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_resource_with_action() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"ALLOW user FOR "database:READ" WHEN user.role == "analyst";"#)
            .unwrap();

        assert_eq!(policy.action, PolicyAction::Allow);
        assert_eq!(policy.subject, "user");
        assert_eq!(policy.resource, "database");
        assert_eq!(policy.resource_action, Some(ResourceAction::Read));
    }

    #[test]
    fn test_parse_not_condition() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"DENY user FOR "feature1" WHEN NOT user.active == true;"#)
            .unwrap();

        if let Some(Condition::Not(_)) = &policy.condition {
            // Success - it's a NOT condition
        } else {
            panic!("Expected NOT condition");
        }
    }

    #[test]
    fn test_parse_comparison_operators() {
        let parser = PolicyParser::new();

        // Test greater than
        let policy = parser
            .parse_line(r#"ALLOW user FOR "feature1" WHEN user.age > 18;"#)
            .unwrap();
        if let Some(Condition::Expression(expr)) = &policy.condition {
            assert_eq!(expr.operator, Operator::GreaterThan);
        } else {
            panic!("Expected expression condition");
        }

        // Test less than or equal
        let policy = parser
            .parse_line(r#"ALLOW user FOR "feature1" WHEN user.score <= 100;"#)
            .unwrap();
        if let Some(Condition::Expression(expr)) = &policy.condition {
            assert_eq!(expr.operator, Operator::LessThanOrEqual);
        } else {
            panic!("Expected expression condition");
        }
    }

    #[test]
    fn test_parse_float_values() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"ALLOW user FOR "feature1" WHEN user.score >= 95.5;"#)
            .unwrap();

        if let Some(Condition::Expression(expr)) = &policy.condition {
            assert_eq!(expr.right, Value::Float(95.5));
        } else {
            panic!("Expected expression condition");
        }
    }

    #[test]
    fn test_parse_contains_operator() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"ALLOW user FOR "admin_panel" WHEN user.roles CONTAINS "admin";"#)
            .unwrap();

        if let Some(Condition::Expression(expr)) = &policy.condition {
            assert_eq!(expr.left, "user.roles");
            assert_eq!(expr.operator, Operator::Contains);
            assert_eq!(expr.right, Value::String("admin".to_string()));
        } else {
            panic!("Expected expression condition with CONTAINS");
        }
    }

    #[test]
    fn test_parse_not_contains_condition() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"DENY user FOR "feature1" WHEN NOT user.tags CONTAINS "verified";"#)
            .unwrap();

        if let Some(Condition::Not(inner)) = &policy.condition {
            if let Condition::Expression(expr) = inner.as_ref() {
                assert_eq!(expr.left, "user.tags");
                assert_eq!(expr.operator, Operator::Contains);
                assert_eq!(expr.right, Value::String("verified".to_string()));
            } else {
                panic!("Expected inner CONTAINS expression");
            }
        } else {
            panic!("Expected NOT condition wrapping CONTAINS");
        }
    }

    #[test]
    fn test_parse_contains_with_and() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"ALLOW user FOR "feature1" WHEN user.roles CONTAINS "editor" AND user.active == true;"#)
            .unwrap();

        if let Some(Condition::And(left, _right)) = &policy.condition {
            if let Condition::Expression(expr) = left.as_ref() {
                assert_eq!(expr.operator, Operator::Contains);
            } else {
                panic!("Expected left side to be CONTAINS expression");
            }
        } else {
            panic!("Expected AND condition");
        }
    }

    #[test]
    fn test_parse_date_value() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"ALLOW user FOR "feature1" WHEN time.date >= 2026-01-01;"#)
            .unwrap();

        if let Some(Condition::Expression(expr)) = &policy.condition {
            assert_eq!(
                expr.right,
                Value::Date(parse_iso_date("2026-01-01").unwrap())
            );
        } else {
            panic!("Expected expression condition");
        }
    }

    #[test]
    fn test_parse_time_value() {
        let parser = PolicyParser::new();
        let policy = parser
            .parse_line(r#"ALLOW user FOR "feature1" WHEN time.time < "18:30";"#)
            .unwrap();

        if let Some(Condition::Expression(expr)) = &policy.condition {
            assert_eq!(expr.right, Value::Time(parse_time_of_day("18:30").unwrap()));
        } else {
            panic!("Expected expression condition");
        }
    }
}
