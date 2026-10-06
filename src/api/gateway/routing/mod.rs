pub(in crate::api::gateway) mod path;

use self::path::path_matches;
use crate::etc::http::request::RequestExt;
use ::http::Request;
use axum::body::Body;
use gate::graph::{
    MatchExprNode, NamedValuePredicate, RouterNode, SourceIpPredicate, ValuePredicate,
};
use std::net::IpAddr;

pub(super) fn router_matches(router: &RouterNode, req: &Request<Body>) -> bool {
    matches_expr(&router.matcher, req)
}

pub(super) fn matches_expr(expr: &MatchExprNode, req: &Request<Body>) -> bool {
    match expr {
        MatchExprNode::All(values) => values.iter().all(|value| matches_expr(value, req)),
        MatchExprNode::Any(values) => values.iter().any(|value| matches_expr(value, req)),
        MatchExprNode::Not(value) => !matches_expr(value, req),
        MatchExprNode::Host(predicate) => req
            .get_host()
            .map(|host| host_value_matches(predicate, &host))
            .unwrap_or_else(|| matches!(predicate, ValuePredicate::Present(false))),
        MatchExprNode::Method(methods) => methods
            .iter()
            .any(|method| method.eq_ignore_ascii_case(req.method().as_str())),
        MatchExprNode::Path(predicate) => path_matches(predicate, req.uri().path()),
        MatchExprNode::Header(predicate) => named_value_matches(
            predicate,
            req.headers()
                .get_all(predicate.name.as_str())
                .iter()
                .filter_map(|value| value.to_str().ok()),
        ),
        MatchExprNode::Query(predicate) => {
            named_value_matches(predicate, req.get_query_values(&predicate.name))
        }
        MatchExprNode::Cookie(predicate) => {
            named_value_matches(predicate, req.get_cookie_value(&predicate.name))
        }
        MatchExprNode::SourceIp(predicate) => req
            .get_client_ip_addr()
            .map(|ip| source_ip_matches(predicate, &ip))
            .unwrap_or(false),
    }
}

fn named_value_matches<S: AsRef<str>>(
    predicate: &NamedValuePredicate,
    values: impl IntoIterator<Item = S>,
) -> bool {
    let mut values = values.into_iter();
    match &predicate.predicate {
        ValuePredicate::Present(expected) => values.next().is_some() == *expected,
        other => values.any(|value| value_matches(other, value.as_ref())),
    }
}

fn value_matches(predicate: &ValuePredicate, value: &str) -> bool {
    match predicate {
        ValuePredicate::Eq(expected) => value == expected,
        ValuePredicate::Prefix(expected) => value.starts_with(expected),
        ValuePredicate::Suffix(expected) => value.ends_with(expected),
        ValuePredicate::Contains(expected) => value.contains(expected),
        ValuePredicate::Regex(expected) => expected.is_match(value),
        ValuePredicate::Present(expected) => *expected,
        ValuePredicate::OneOf(expected) => expected.iter().any(|candidate| candidate == value),
    }
}

fn host_value_matches(predicate: &ValuePredicate, host: &str) -> bool {
    match predicate {
        ValuePredicate::Eq(expected) => host.eq_ignore_ascii_case(expected),
        ValuePredicate::Prefix(expected) => host
            .as_bytes()
            .get(..expected.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(expected.as_bytes())),
        ValuePredicate::Suffix(expected) => {
            host.len().checked_sub(expected.len()).is_some_and(|start| {
                host.as_bytes()[start..].eq_ignore_ascii_case(expected.as_bytes())
            })
        }
        ValuePredicate::Contains(expected) => {
            expected.is_empty()
                || host
                    .as_bytes()
                    .windows(expected.len())
                    .any(|part| part.eq_ignore_ascii_case(expected.as_bytes()))
        }
        ValuePredicate::Regex(expected) => expected.is_match(host),
        ValuePredicate::Present(expected) => *expected,
        ValuePredicate::OneOf(expected) => expected
            .iter()
            .any(|candidate| host.eq_ignore_ascii_case(candidate)),
    }
}

fn source_ip_matches(predicate: &SourceIpPredicate, ip: &IpAddr) -> bool {
    predicate
        .networks
        .iter()
        .any(|network| network.contains(ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borrowed_header_values_preserve_presence_and_duplicates() {
        let mut req = Request::new(Body::empty());
        req.headers_mut().append("x-kind", "first".parse().unwrap());
        req.headers_mut()
            .append("x-kind", "second".parse().unwrap());
        req.headers_mut()
            .insert("x-invalid", http::HeaderValue::from_bytes(&[0xff]).unwrap());
        for (name, predicate, expected) in [
            ("x-kind", ValuePredicate::Eq("second".into()), true),
            ("x-kind", ValuePredicate::Present(false), false),
            ("x-missing", ValuePredicate::Present(false), true),
            ("x-invalid", ValuePredicate::Present(false), true),
        ] {
            assert_eq!(
                matches_expr(
                    &MatchExprNode::Header(NamedValuePredicate {
                        name: name.into(),
                        predicate
                    }),
                    &req
                ),
                expected
            );
        }
    }

    #[test]
    fn host_case_comparison_preserves_ascii_and_unicode_behavior() {
        for host in ["API.Example.COM", "", "Ä.Example", "é.test"] {
            for expected in [
                "",
                "EXAMPLE",
                ".COM",
                "Ä",
                "ä",
                "é",
                "a-longer-host-than-any-fixture",
            ] {
                let lower_host = host.to_ascii_lowercase();
                let lower_expected = expected.to_ascii_lowercase();
                for (predicate, matches) in [
                    (
                        ValuePredicate::Prefix(expected.into()),
                        lower_host.starts_with(&lower_expected),
                    ),
                    (
                        ValuePredicate::Suffix(expected.into()),
                        lower_host.ends_with(&lower_expected),
                    ),
                    (
                        ValuePredicate::Contains(expected.into()),
                        lower_host.contains(&lower_expected),
                    ),
                ] {
                    assert_eq!(host_value_matches(&predicate, host), matches);
                }
            }
        }
    }
}
