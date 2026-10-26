//! Where the hosted apps are served on this server. The API mounts them there,
//! and links, redirects, and the Console's default OAuth callback point there.

const DEFAULT_AUTH_APP_BASE_PATH: &str = "/auth";
const DEFAULT_CONSOLE_APP_BASE_PATH: &str = "/stargate";

/// Base path of the hosted auth app (`AUTH_APP_BASE_PATH`, default `/auth`).
pub fn auth_app_base_path() -> String {
    base_path("AUTH_APP_BASE_PATH", DEFAULT_AUTH_APP_BASE_PATH)
}

/// Base path of the Console (`CONSOLE_APP_BASE_PATH`, default `/stargate`).
pub fn console_app_base_path() -> String {
    base_path("CONSOLE_APP_BASE_PATH", DEFAULT_CONSOLE_APP_BASE_PATH)
}

fn base_path(name: &str, default: &str) -> String {
    normalize_base_path(std::env::var(name).ok().as_deref(), default)
}

/// `/{path}` without surrounding spaces or slashes; `default` when the value is
/// missing or holds nothing else, so an app is never mounted at the root.
fn normalize_base_path(value: Option<&str>, default: &str) -> String {
    match value.map(|value| value.trim().trim_matches('/')) {
        Some(path) if !path.is_empty() => format!("/{path}"),
        _ => default.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_base_path;

    #[test]
    fn base_paths_have_one_leading_slash() {
        assert_eq!(normalize_base_path(Some("auth"), "/x"), "/auth");
        assert_eq!(normalize_base_path(Some("/auth/"), "/x"), "/auth");
        assert_eq!(
            normalize_base_path(Some("  console/admin  "), "/x"),
            "/console/admin"
        );
    }

    #[test]
    fn missing_or_empty_base_paths_use_the_default() {
        assert_eq!(normalize_base_path(None, "/auth"), "/auth");
        assert_eq!(normalize_base_path(Some(""), "/auth"), "/auth");
        assert_eq!(normalize_base_path(Some(" / "), "/stargate"), "/stargate");
    }
}
