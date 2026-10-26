pub mod env;
pub mod features;

/// Directory holding the gateway configuration and the access-control
/// policies (`CONFIG_PATH`, default `.stargate`).
pub fn config_dir() -> String {
    std::env::var("CONFIG_PATH").unwrap_or_else(|_| ".stargate".to_string())
}
