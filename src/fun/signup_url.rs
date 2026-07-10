use crate::fun::get_base_url;

const DEFAULT_AUTH_APP_BASE_PATH: &str = "/auth";

pub fn signup_url(token: &str) -> String {
    let base_url = std::env::var("AUTH_PUBLIC_BASE_URL").unwrap_or_else(|_| {
        let auth_app_base_path = std::env::var("AUTH_APP_BASE_PATH")
            .unwrap_or_else(|_| DEFAULT_AUTH_APP_BASE_PATH.to_string());
        format!(
            "{}{}",
            get_base_url().trim_end_matches('/'),
            normalize_base_path(&auth_app_base_path)
        )
    });

    signup_url_from_base(&base_url, token)
}

fn normalize_base_path(path: &str) -> String {
    format!("/{}", path.trim().trim_matches('/'))
}

fn signup_url_from_base(base_url: &str, token: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("token", token)
        .finish();
    format!("{}/signup?{query}", base_url.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::{normalize_base_path, signup_url_from_base};

    #[test]
    fn normalizes_auth_app_base_paths() {
        assert_eq!(normalize_base_path("auth"), "/auth");
        assert_eq!(normalize_base_path("/auth/"), "/auth");
    }

    #[test]
    fn builds_signup_url_with_encoded_token() {
        assert_eq!(
            signup_url_from_base("https://auth.example.com/", "a+b/c"),
            "https://auth.example.com/signup?token=a%2Bb%2Fc"
        );
    }
}
