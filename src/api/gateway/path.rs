use gate::graph::PathPredicate;

pub(super) fn path_matches(predicate: &PathPredicate, path: &str) -> bool {
    match predicate {
        PathPredicate::Exact(expected) => path == expected,
        PathPredicate::Prefix(expected) => path_prefix_matches(path, expected),
        PathPredicate::Template(expected) => template_matches(expected, path),
        PathPredicate::Regex(expected) => expected.is_match(path),
    }
}

pub(super) fn path_prefix_matches(path: &str, prefix: &str) -> bool {
    let prefix = normalize_path(prefix);
    if prefix == "/" {
        return path.starts_with('/');
    }

    path == prefix
        || path
            .strip_prefix(&prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn template_matches(template: &str, path: &str) -> bool {
    let template = normalize_path(template);
    let template_segments = path_segments(&template);
    let path_segments = path_segments(path);

    if template_segments.len() != path_segments.len() {
        return false;
    }

    template_segments
        .iter()
        .zip(path_segments.iter())
        .all(|(template_segment, path_segment)| {
            is_template_segment(template_segment) || template_segment == path_segment
        })
}

fn is_template_segment(segment: &str) -> bool {
    segment.starts_with('{') && segment.ends_with('}') && segment.len() > 2
}

fn path_segments(path: &str) -> Vec<&str> {
    if path == "/" {
        return Vec::new();
    }
    path.trim_start_matches('/').split('/').collect()
}

pub(super) fn normalize_path(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() || path == "/" {
        return "/".to_string();
    }

    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    };

    if path.len() > 1 {
        path.trim_end_matches('/').to_string()
    } else {
        path
    }
}

pub(super) fn strip_prefix(path: &str, prefix: &str) -> String {
    let prefix = normalize_path(prefix);
    if path == prefix {
        return "/".to_string();
    }

    if let Some(rest) = path.strip_prefix(&prefix) {
        if rest.starts_with('/') {
            return normalize_path(rest);
        }
    }

    path.to_string()
}

pub(super) fn add_prefix(path: &str, prefix: &str) -> String {
    let path = normalize_path(path);
    let prefix = normalize_path(prefix);
    if path == "/" {
        prefix
    } else if prefix == "/" {
        path
    } else {
        format!("{}{}", prefix, path)
    }
}
