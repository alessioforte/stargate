use super::path::path_matches;
use crate::etc::ext::RequestExt;
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
                .filter_map(|value| value.to_str().ok())
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ),
        MatchExprNode::Query(predicate) => {
            named_value_matches(predicate, req.get_query_values(&predicate.name))
        }
        MatchExprNode::Cookie(predicate) => named_value_matches(
            predicate,
            req.get_cookie_value(&predicate.name).into_iter().collect(),
        ),
        MatchExprNode::SourceIp(predicate) => req
            .get_client_ip_addr()
            .map(|ip| source_ip_matches(predicate, &ip))
            .unwrap_or(false),
    }
}

fn named_value_matches(predicate: &NamedValuePredicate, values: Vec<String>) -> bool {
    match &predicate.predicate {
        ValuePredicate::Present(expected) => {
            (*expected && !values.is_empty()) || (!expected && values.is_empty())
        }
        other => values.iter().any(|value| value_matches(other, value)),
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
            .to_ascii_lowercase()
            .starts_with(&expected.to_ascii_lowercase()),
        ValuePredicate::Suffix(expected) => host
            .to_ascii_lowercase()
            .ends_with(&expected.to_ascii_lowercase()),
        ValuePredicate::Contains(expected) => host
            .to_ascii_lowercase()
            .contains(&expected.to_ascii_lowercase()),
        ValuePredicate::Regex(expected) => expected.is_match(host),
        ValuePredicate::Present(expected) => *expected,
        ValuePredicate::OneOf(expected) => expected
            .iter()
            .any(|candidate| host.eq_ignore_ascii_case(candidate)),
    }
}

fn source_ip_matches(predicate: &SourceIpPredicate, ip: &IpAddr) -> bool {
    predicate.cidrs.iter().any(|cidr| cidr_contains(cidr, ip))
}

fn cidr_contains(raw: &str, ip: &IpAddr) -> bool {
    let raw = raw.trim();
    if raw.is_empty() {
        return false;
    }

    let Some((addr, prefix)) = raw.split_once('/') else {
        return raw
            .parse::<IpAddr>()
            .ok()
            .is_some_and(|candidate| candidate == *ip);
    };

    let Ok(prefix_len) = prefix.parse::<u8>() else {
        return false;
    };
    let Ok(network) = addr.parse::<IpAddr>() else {
        return false;
    };

    match (network, ip) {
        (IpAddr::V4(network), IpAddr::V4(candidate)) if prefix_len <= 32 => {
            let mask = if prefix_len == 0 {
                0
            } else {
                !0u32 << (32 - prefix_len)
            };
            (u32::from(*candidate) & mask) == (u32::from(network) & mask)
        }
        (IpAddr::V6(network), IpAddr::V6(candidate)) if prefix_len <= 128 => {
            let mask = if prefix_len == 0 {
                0
            } else {
                !0u128 << (128 - prefix_len)
            };
            (u128::from(*candidate) & mask) == (u128::from(network) & mask)
        }
        _ => false,
    }
}
