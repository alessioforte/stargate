//! Preparation, publication, and pinning of the gateway runtime snapshot.

mod body;
mod config;
mod reload;
mod runtime;
mod sync;

pub use body::retain_runtime;
#[cfg(all(test, feature = "memory"))]
pub(crate) use config::GatewayPreparationError;
#[cfg(test)]
pub(crate) use config::prepare_config;
pub(crate) use config::{ConfigActivation, save_gateway_config, validate_gateway_document};
pub(super) use config::{load_gateway_document, prepare_gateway_config};
#[cfg(all(test, feature = "memory"))]
pub(crate) use reload::apply_gateway_config;
pub(crate) use reload::apply_policy_document;
#[cfg(test)]
pub(super) use runtime::test_support;
pub use runtime::{Gate, RuntimeSnapshot};
pub(crate) use sync::{shutdown_config_sync, spawn_config_sync};
