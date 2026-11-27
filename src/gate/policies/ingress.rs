use crate::err::{ErrorCode, ErrorResponse};
use crate::{
    gate::{
        RuntimeSnapshot,
        limits::{LimitPoint, TrafficLimit, check_traffic_limit, ingress_key},
        usage::StoreUsage,
    },
    obs::telemetry,
    web::limits::limited_response,
};
use lim::LimitKind;

/// The plane whose ingress bucket gateway traffic charges.
const GATEWAY_PLANE: &str = "gateway";

pub(in crate::gate) async fn apply_ingress_limit(
    runtime: &RuntimeSnapshot,
    client: &str,
    execution: &crate::gate::lifecycle::Execution,
    usage: &mut StoreUsage,
) -> Result<(), ErrorResponse> {
    let ingress = &runtime.ingress;
    let key = ingress_key(GATEWAY_PLANE, client);
    let (decision, commands) = store::observe::count_commands(execution.run(
        "ingress",
        None,
        check_traffic_limit(
            runtime,
            LimitPoint::GatewayIngress,
            ingress.on_store_error,
            TrafficLimit {
                name: &ingress.limit,
                key: &key,
                scope: "ingress",
                cost: None,
            },
        ),
    ))
    .await;
    usage.record_limit(LimitKind::Rate, commands);
    let Some(decision) = decision?? else {
        return Ok(());
    };
    if decision.is_allowed() {
        telemetry::record_gateway_policy("ingress", "allowed");
        return Ok(());
    }
    telemetry::record_gateway_policy("ingress", "denied");
    telemetry::record_gateway_rejection("ingress");
    Err(limited_response(
        &decision,
        "ingress",
        ErrorCode::GatewayIngressRateLimitExceeded,
        "x-ratelimit",
    ))
}
