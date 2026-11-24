//! Diagnose gateway routes that can share an origin with the IAM API.

use gate::graph::{HttpGraph, MatchExprNode, ValuePredicate};

pub(in crate::gate) fn warn_shared_origin(graph: &HttpGraph, hosts: &crate::conf::hosts::IamHosts) {
    if hosts.is_unrestricted()
        && graph
            .routers
            .iter()
            .any(|router| !constrains_host(&router.matcher))
    {
        tracing::warn!(
            "IAM_HOSTS is empty and gateway routers have no host constraint; upstream content can share the IAM origin. Set IAM_HOSTS and constrain gateway routers to separate API hosts"
        );
    }
}

fn constrains_host(matcher: &MatchExprNode) -> bool {
    match matcher {
        MatchExprNode::Host(ValuePredicate::Present(_)) => false,
        MatchExprNode::Host(_) => true,
        MatchExprNode::All(children) => children.iter().any(constrains_host),
        MatchExprNode::Any(children) => {
            !children.is_empty() && children.iter().all(constrains_host)
        }
        // A negated host matcher excludes a host; it does not isolate an origin.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gate::graph::PathPredicate;

    #[test]
    fn every_alternative_must_constrain_the_host() {
        let host = MatchExprNode::Host(ValuePredicate::Eq("api.example.com".into()));
        let path = MatchExprNode::Path(PathPredicate::Prefix("/".into()));
        assert!(constrains_host(&MatchExprNode::All(vec![
            host.clone(),
            path.clone()
        ])));
        assert!(!constrains_host(&MatchExprNode::Any(vec![
            host.clone(),
            path
        ])));
        assert!(constrains_host(&MatchExprNode::Any(vec![host.clone()])));
        assert!(!constrains_host(&MatchExprNode::Not(Box::new(host))));
        assert!(!constrains_host(&MatchExprNode::Host(
            ValuePredicate::Present(true)
        )));
    }
}
