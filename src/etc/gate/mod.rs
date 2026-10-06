mod body;
mod config;
mod reload;
pub mod resources;
mod runtime;
mod transport;

#[cfg(test)]
pub(crate) mod test_support;
pub use body::retain_runtime;
pub use config::get_policies_path;
pub(crate) use config::{GatewayPreparationError, prepare_config};
#[cfg(all(test, feature = "memory"))]
pub(crate) use reload::reload_gateway_config_from_path;
pub use reload::reload_policy_engine;
pub use runtime::{Gate, RuntimeSnapshot};
#[cfg(test)]
pub(crate) use transport::DeadlineConnector;
pub use transport::{HyperClient, PreparedTransport, connection_timed_out};

use crate::etc::store::use_store;
use config::{get_config_path, load_config_from_path};
use reload::{watch_config_file, watch_policies_file};
use std::sync::Arc;

pub fn init() -> anyhow::Result<Arc<Gate>> {
    let config_file_path = get_config_path()?;
    let prepared = load_config_from_path(&config_file_path)?;
    let store = use_store();
    let policies_path = get_policies_path();
    let policies = crate::act::access_control_rules::load_policy_snapshot(&policies_path)
        .map_err(|error| anyhow::anyhow!("Unable to load access-control policies: {error}"))?;

    let gate = Arc::new(Gate::new(Arc::new(store.clone()), prepared, policies)?);
    watch_config_file(&config_file_path, &gate);
    watch_policies_file(&policies_path, &gate);
    Ok(gate)
}
