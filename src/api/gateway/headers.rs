use super::types::ResponseHeaderMutations;
use ::http::{HeaderMap, HeaderName, HeaderValue};
use gate::graph::HeaderValueNode;

pub(super) fn apply_request_headers(
    headers: &mut HeaderMap,
    add: &[HeaderValueNode],
    set: &[HeaderValueNode],
    remove: &[String],
) {
    for name in remove {
        headers.remove(name);
    }
    for header in add {
        append_header(headers, header);
    }
    for header in set {
        insert_header(headers, header);
    }
}

pub(super) fn apply_response_header_mutations(
    headers: &mut HeaderMap,
    mutations: &ResponseHeaderMutations,
) {
    for name in &mutations.remove {
        headers.remove(name);
    }
    for header in &mutations.add {
        append_header(headers, header);
    }
    for header in &mutations.set {
        insert_header(headers, header);
    }
}

fn append_header(headers: &mut HeaderMap, header: &HeaderValueNode) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(header.name.as_bytes()),
        HeaderValue::from_str(&header.value),
    ) {
        headers.append(name, value);
    }
}

pub(super) fn insert_header(headers: &mut HeaderMap, header: &HeaderValueNode) {
    if let (Ok(name), Ok(value)) = (
        HeaderName::from_bytes(header.name.as_bytes()),
        HeaderValue::from_str(&header.value),
    ) {
        headers.insert(name, value);
    }
}

pub(super) fn apply_gateway_headers(headers: &mut HeaderMap, additions: &HeaderMap) {
    for (name, value) in additions {
        headers.insert(name, value.clone());
    }
}
