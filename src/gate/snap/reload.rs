use super::Gate;
use super::config::GatewayPreparationError;
use crate::obs::telemetry;
use db::ent::ConfigDocument;
use std::sync::Arc;

#[cfg(all(test, feature = "memory"))]
pub(crate) fn apply_gateway_config(
    gate: &Gate,
    yaml: &str,
) -> Result<u64, GatewayPreparationError> {
    record(super::config::prepare_gateway_config(yaml).and_then(|prepared| gate.activate(prepared)))
}

pub(super) async fn apply_gateway_document(
    gate: &Arc<Gate>,
    document: ConfigDocument,
) -> Result<u64, GatewayPreparationError> {
    let version = document.version;
    let runtime = gate.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        runtime.prepare_activation(super::config::prepare_gateway_document(&document)?)
    })
    .await?;
    record(match prepared {
        Ok(activation) => gate.publish_document(activation, version).await,
        Err(error) => Err(error),
    })
}

fn record(result: Result<u64, GatewayPreparationError>) -> Result<u64, GatewayPreparationError> {
    telemetry::record_config_reload(
        "gateway_config",
        result
            .as_ref()
            .map_or_else(|error| error.reload_outcome(), |_| "success"),
    );
    result
}

pub(crate) fn apply_policy_document(
    gate: &Gate,
    content: &str,
) -> Result<bool, crate::err::ErrorResponse> {
    let policies =
        crate::iam::authz::rules::compile_policy_snapshot(content).inspect_err(|_| {
            telemetry::record_config_reload("policies", "error");
        })?;
    if gate.policy_snapshot.load().revision == policies.revision {
        return Ok(false);
    }
    gate.policy_snapshot.store(Arc::new(policies));
    telemetry::record_config_reload("policies", "success");
    Ok(true)
}

#[cfg(all(test, feature = "memory"))]
mod tests;
