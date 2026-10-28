pub mod apps;
pub mod env;
pub mod features;
pub mod hosts;

pub use hosts::iam_hosts;

/// Public base URL of this server: `OAUTH_BASE_URL`, else the issuer
/// (`JWT_ISSUER`, default `http://localhost:$PORT`). Provider endpoints, links
/// sent outside the server (emails), and the OpenAPI `servers` entry use it;
/// the host and port the process listens on are not public.
pub fn public_base_url() -> String {
    let configured = std::env::var("OAUTH_BASE_URL").ok();
    let port = std::env::var("PORT").unwrap_or_else(|_| "5050".to_string());
    oidc::metadata::endpoint_base_url(&jwt::issuer_from_env(), configured.as_deref(), &port)
}

/// Directory holding gateway/policy seed files and other runtime assets
/// (`CONFIG_PATH`, default `.stargate`).
pub fn config_dir() -> String {
    std::env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string())
}
