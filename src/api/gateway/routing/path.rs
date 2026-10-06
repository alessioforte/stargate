use gate::graph::PathPredicate;
use std::borrow::Cow;

pub(in crate::api::gateway) fn path_matches(predicate: &PathPredicate, path: &str) -> bool {
    match predicate {
        PathPredicate::Exact(expected) => path == expected,
        PathPredicate::Prefix(expected) => path_prefix_matches(path, expected),
        PathPredicate::Template(expected) => template_matches(expected, path),
        PathPredicate::Regex(expected) => expected.is_match(path),
    }
}

pub(in crate::api::gateway) fn path_prefix_matches(path: &str, prefix: &str) -> bool {
    let prefix = normalized_path(prefix);
    if prefix == "/" {
        return path.starts_with('/');
    }

    path == prefix
        || path
            .strip_prefix(prefix.as_ref())
            .is_some_and(|rest| rest.starts_with('/'))
}

fn template_matches(template: &str, path: &str) -> bool {
    let template = normalized_path(template);
    let mut templates = path_segments(&template);
    let mut paths = path_segments(path);
    loop {
        match (templates.next(), paths.next()) {
            (None, None) => return true,
            (Some(template), Some(path)) if is_template_segment(template) || template == path => {}
            _ => return false,
        }
    }
}

fn is_template_segment(segment: &str) -> bool {
    segment.starts_with('{') && segment.ends_with('}') && segment.len() > 2
}

fn path_segments(path: &str) -> impl Iterator<Item = &str> {
    (path != "/")
        .then(|| path.trim_start_matches('/').split('/'))
        .into_iter()
        .flatten()
}

fn normalized_path(path: &str) -> Cow<'_, str> {
    let path = path.trim();
    if path.is_empty() || path == "/" {
        return Cow::Borrowed("/");
    }
    if path.starts_with('/') {
        Cow::Borrowed(path.trim_end_matches('/'))
    } else {
        Cow::Owned(format!("/{}", path.trim_end_matches('/')))
    }
}

pub(in crate::api::gateway) fn normalize_path(path: &str) -> String {
    normalized_path(path).into_owned()
}

pub(in crate::api::gateway) fn strip_prefix(path: &str, prefix: &str) -> String {
    let prefix = normalized_path(prefix);
    if path == prefix {
        return "/".to_string();
    }

    if let Some(rest) = path.strip_prefix(prefix.as_ref())
        && rest.starts_with('/')
    {
        return normalize_path(rest);
    }

    path.to_string()
}

pub(in crate::api::gateway) fn add_prefix(path: &str, prefix: &str) -> String {
    let path = normalize_path(path);
    let prefix = normalized_path(prefix);
    if path == "/" {
        prefix.into_owned()
    } else if prefix == "/" {
        path
    } else {
        format!("{}{}", prefix, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_and_matching_preserve_segment_boundaries() {
        for (raw, expected) in [
            ("", "/"),
            (" / ", "/"),
            ("users///", "/users"),
            ("///", ""),
            (" /a//b/ ", "/a//b"),
        ] {
            assert_eq!(normalize_path(raw), expected);
        }
        for (path, prefix, expected) in [
            ("/users/42", " users/ ", true),
            ("/users2", "/users", false),
            ("/", "/", true),
            ("/users/", "/users", true),
        ] {
            assert_eq!(path_prefix_matches(path, prefix), expected);
        }
        for (template, path, expected) in [
            (" /users/{id}/ ", "/users/42", true),
            ("/users/{id}", "/users/42/", false),
            ("/users/{id}", "/users/", true),
            ("/users/{}", "/users/42", false),
            ("/", "/", true),
            ("/users//{id}", "/users//42", true),
        ] {
            assert_eq!(template_matches(template, path), expected);
        }
    }
}
