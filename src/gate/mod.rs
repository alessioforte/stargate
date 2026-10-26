mod authentication;
mod execution;
mod lifecycle;
pub mod limits;
mod middlewares;
#[cfg(all(test, feature = "memory"))]
mod performance;
mod policies;
mod request;
pub mod resources;
mod response;
mod routing;
mod snap;
#[cfg(test)]
pub(crate) mod test_support;
mod transport;
mod upstream;

pub(crate) use policies::org_limits::invalidate as invalidate_org_limits;
#[cfg(all(test, feature = "memory"))]
pub(crate) use request::IngressPause;
pub(crate) use snap::prepare_config;
#[cfg(all(test, feature = "memory"))]
pub(crate) use snap::reload_gateway_config_from_path;
pub use snap::{Gate, RuntimeSnapshot, reload_policy_engine, retain_runtime};
#[cfg(test)]
pub(crate) use transport::DeadlineConnector;
pub use transport::{HyperClient, PreparedTransport, connection_timed_out};

use crate::data::store::use_store;
use crate::iam::authz::storage::get_policies_path;
use ::http::Request;
use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use snap::{get_config_path, load_config_from_path, watch_config_file, watch_policies_file};
use std::{convert::Infallible, sync::Arc};

pub fn init() -> anyhow::Result<Arc<Gate>> {
    let config_file_path = get_config_path()?;
    let prepared = load_config_from_path(&config_file_path)?;
    let store = use_store();
    let policies_path = get_policies_path();
    let policies = crate::iam::authz::rules::load_policy_snapshot(&policies_path)
        .map_err(|error| anyhow::anyhow!("Unable to load access-control policies: {error}"))?;

    let gate = Arc::new(Gate::new(Arc::new(store.clone()), prepared, policies)?);
    watch_config_file(&config_file_path, &gate);
    watch_policies_file(&policies_path, &gate);
    Ok(gate)
}

pub async fn service(req: Request<Body>) -> Result<Response, Infallible> {
    match request::handle_hyper(req).await {
        Ok(response) => Ok(response),
        Err(error) => Ok(error.into_response()),
    }
}

#[cfg(test)]
mod tests;
