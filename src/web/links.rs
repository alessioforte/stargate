use crate::conf::apps::auth_app_base_path;

/// Base of the hosted auth app for redirects answered by this server:
/// `AUTH_PUBLIC_BASE_URL` when set, otherwise the app's base path here.
pub fn auth_app_href() -> String {
    std::env::var("AUTH_PUBLIC_BASE_URL").unwrap_or_else(|_| auth_app_base_path())
}

/// Absolute auth-app base for links sent outside this server, such as email.
fn auth_app_public_base() -> String {
    std::env::var("AUTH_PUBLIC_BASE_URL").unwrap_or_else(|_| {
        format!(
            "{}{}",
            crate::conf::public_base_url().trim_end_matches('/'),
            auth_app_base_path()
        )
    })
}

pub fn signup_url(token: &str) -> String {
    link_from_base(&auth_app_public_base(), "signup", Some(token))
}

pub fn change_password_url(token: &str) -> String {
    link_from_base(&auth_app_public_base(), "change-password", Some(token))
}

pub fn login_url() -> String {
    link_from_base(&auth_app_public_base(), "login", None)
}

/// An auth-app page for a redirect answered by this server, with query
/// `params`.
pub fn auth_app_redirect(path: &str, params: &[(&str, &str)]) -> String {
    let url = format!("{}/{path}", auth_app_href().trim_end_matches('/'));
    if params.is_empty() {
        return url;
    }
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();
    format!("{url}?{query}")
}

fn link_from_base(base_url: &str, path: &str, token: Option<&str>) -> String {
    let url = format!("{}/{path}", base_url.trim_end_matches('/'));
    let Some(token) = token else {
        return url;
    };
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("token", token)
        .finish();
    format!("{url}?{query}")
}

#[cfg(test)]
mod tests {
    use super::link_from_base;

    #[test]
    fn builds_auth_app_links_with_encoded_tokens() {
        assert_eq!(
            link_from_base("https://auth.example.com/", "signup", Some("a+b/c")),
            "https://auth.example.com/signup?token=a%2Bb%2Fc"
        );
        assert_eq!(
            link_from_base("https://id.example.com/auth", "change-password", Some("t")),
            "https://id.example.com/auth/change-password?token=t"
        );
        assert_eq!(
            link_from_base("https://id.example.com/auth", "login", None),
            "https://id.example.com/auth/login"
        );
    }
}
