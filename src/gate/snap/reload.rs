use super::Gate;
#[cfg(any(feature = "postgres", all(test, feature = "memory")))]
use super::config::GatewayPreparationError;
use crate::obs::telemetry;
use std::sync::Arc;

#[cfg(any(feature = "postgres", all(test, feature = "memory")))]
pub(crate) fn apply_gateway_config(
    gate: &Gate,
    yaml: &str,
) -> Result<u64, GatewayPreparationError> {
    let result =
        super::config::prepare_gateway_config(yaml).and_then(|prepared| gate.activate(prepared));
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
