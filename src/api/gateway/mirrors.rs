use super::{
    disposal::{DisposalOutcome, discard_response},
    execution::execute_plan_from_replay,
    types::{ExecutionPlan, ReplayRequest, RequestState},
};
use crate::{
    err::ErrorCode,
    etc::{
        gate::{RuntimeSnapshot, resources::ResourcePermit},
        telemetry,
    },
};
use ctx::DispatchKind;
use std::sync::Arc;
use tracing::Instrument;

pub(super) fn record_skipped_mirrors(
    mirrors: &[(ExecutionPlan, ResourcePermit)],
    outcome: &'static str,
) {
    for _ in mirrors {
        telemetry::record_gateway_mirror(outcome);
    }
}

pub(super) fn admit_mirrors(
    runtime: &RuntimeSnapshot,
    mirrors: &[ExecutionPlan],
) -> Vec<(ExecutionPlan, ResourcePermit)> {
    mirrors
        .iter()
        .filter_map(|mirror| {
            let permit = runtime.resources.admit_mirror();
            if permit.is_none() {
                telemetry::record_gateway_mirror("skipped_capacity");
            }
            permit.map(|permit| (mirror.clone(), permit))
        })
        .collect()
}

pub(super) fn spawn_mirrors(
    runtime: Arc<RuntimeSnapshot>,
    mirrors: Vec<(ExecutionPlan, ResourcePermit)>,
    replay: ReplayRequest,
    state: RequestState,
) {
    for (mirror, permit) in mirrors {
        let resources = runtime.resources.clone();
        let runtime = runtime.clone();
        let replay = replay.clone();
        let mut state = state.clone();
        // Mirrors have their own total budget and never retain primary admission.
        state.execution = super::lifecycle::Execution::new(
            runtime.settings.mirror_timeout,
            resources.shutdown.child_token(),
        );
        let spawned = resources.spawn(
            async move {
                let outcome = execute_mirror(&runtime, &mirror, &replay, &state).await;
                telemetry::record_gateway_mirror(outcome);
                drop(permit);
            }
            .instrument(tracing::debug_span!("gateway.mirror")),
        );
        telemetry::record_gateway_mirror(if spawned {
            "dispatched"
        } else {
            "skipped_shutdown"
        });
    }
}

pub(super) async fn execute_mirror(
    runtime: &Arc<RuntimeSnapshot>,
    mirror: &ExecutionPlan,
    replay: &ReplayRequest,
    state: &RequestState,
) -> &'static str {
    // Shadow outcomes never feed the circuit breaker, including during failover.
    match execute_plan_from_replay(runtime, mirror, replay, state, DispatchKind::Shadow).await {
        Ok(response) => {
            match discard_response(response, "mirror", &runtime.settings, &state.execution).await {
                DisposalOutcome::Drained => "success",
                DisposalOutcome::ByteLimit => "abandoned",
                DisposalOutcome::Timeout => "timeout",
                DisposalOutcome::BodyError => "body_error",
                DisposalOutcome::Cancelled => "cancelled",
            }
        }
        Err(error) => {
            tracing::warn!(%error, "Mirror request failed");
            match error.code {
                ErrorCode::GatewayTimeout | ErrorCode::GatewayUploadTimeout => "timeout",
                ErrorCode::GatewayCancelled => "cancelled",
                _ => "error",
            }
        }
    }
}
