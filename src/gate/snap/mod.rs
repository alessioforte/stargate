//! Preparation, publication, and pinning of the gateway runtime snapshot.

mod body;
mod config;
mod reload;
mod runtime;

pub use body::retain_runtime;
pub(crate) use config::{GatewayPreparationError, prepare_config};
pub(super) use config::{get_config_path, load_config_from_path};
#[cfg(all(test, feature = "memory"))]
pub(crate) use reload::reload_gateway_config_from_path;
pub use reload::reload_policy_engine;
pub(super) use reload::{watch_config_file, watch_policies_file};
#[cfg(test)]
pub(super) use runtime::test_support;
pub use runtime::{Gate, RuntimeSnapshot};
