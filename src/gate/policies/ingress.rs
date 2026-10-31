use crate::err::{ErrorCode, ErrorResponse};
use crate::{
    gate::{
        RuntimeSnapshot,
        limits::{LimitPoint, TrafficLimit, check_traffic_limit, ingress_key},
    },
    obs::telemetry,
    web::limits::limited_response,
};

pub(in crate::gate) async fn apply_ingress_limit(
    runtime: &RuntimeSnapshot,
    client_ip: &str,
    execution: &crate::gate::lifecycle::Execution,
) -> Result<(), ErrorResponse> {
    let ingress = &runtime.ingress;
    let key = ingress_key(client_ip);
    let decision = execution
        .run(
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
        )
        .await??;
    let Some(decision) = decision else {
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
