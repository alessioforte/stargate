mod authentication;
mod execution;
mod grpc;
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
#[cfg(all(test, feature = "memory"))]
pub(crate) use snap::apply_gateway_config;
pub(crate) use snap::apply_policy_document;
#[cfg(test)]
pub(crate) use snap::prepare_config;
pub(crate) use snap::{ConfigActivation, save_gateway_config, validate_gateway_document};
pub use snap::{Gate, RuntimeSnapshot, retain_runtime};
pub(crate) use snap::{shutdown_config_sync, spawn_config_sync};
#[cfg(test)]
pub(crate) use transport::DeadlineConnector;
pub use transport::{HyperClient, PreparedTransport, connection_timed_out};

use crate::data::store::use_store;
use ::http::Request;
use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use snap::{load_gateway_document, prepare_gateway_config};
use std::{convert::Infallible, sync::Arc};

pub async fn init() -> anyhow::Result<Arc<Gate>> {
    let document = load_gateway_document().await?;
    let prepared = prepare_gateway_config(&document.content)?;
    let policies = crate::iam::authz::storage::load_policies_document().await?;
    let policies = crate::iam::authz::rules::compile_policy_snapshot(&policies.content)
        .map_err(|error| anyhow::anyhow!("Unable to load access-control policies: {error}"))?;
    Ok(Arc::new(Gate::new(
        Arc::new(use_store().clone()),
        prepared,
        policies,
    )?))
}

pub async fn service(req: Request<Body>) -> Result<Response, Infallible> {
    match request::handle_hyper(req).await {
        Ok(response) => Ok(response),
        Err(error) => Ok(error.into_response()),
    }
}

#[cfg(test)]
mod tests;
