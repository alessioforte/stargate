use crate::err::{ErrorCode, ErrorResponse};
use crate::{
    gate::{RuntimeSnapshot, limits::ingress_key},
    obs::telemetry,
    web::limits::limited_response,
};
use lim::LimitKind;

pub(in crate::gate) async fn apply_ingress_limit(
    runtime: &RuntimeSnapshot,
    client_ip: &str,
    execution: &crate::gate::lifecycle::Execution,
) -> Result<(), ErrorResponse> {
    let limiter = &runtime.core.limiter;
    let ingress = &runtime.ingress;
    if limiter
        .validate_limit(&ingress.limit, LimitKind::Rate)
        .is_err()
    {
        telemetry::record_gateway_policy("ingress", "error");
        return Err(ErrorResponse::new(ErrorCode::GatewayIngressUnavailable));
    }
    let key = ingress_key(client_ip);
    let decision = match execution
        .run(
            "ingress",
            ingress.timeout,
            limiter.check(&ingress.limit, &key, None),
        )
        .await
    {
        Ok(Ok(decision)) => decision,
        Ok(Err(error)) => {
            telemetry::record_gateway_policy("ingress", "error");
            tracing::error!(%error, "Ingress limiter failed");
            return Err(ErrorResponse::new(ErrorCode::GatewayIngressUnavailable));
        }
        Err(error)
            if error.code == ErrorCode::GatewayTimeout
                && error
                    .params
                    .get("phase")
                    .is_some_and(|phase| phase == "ingress") =>
        {
            telemetry::record_gateway_policy("ingress", "timeout");
            return Err(ErrorResponse::new(ErrorCode::GatewayIngressUnavailable)
                .with_param("phase", "ingress"));
        }
        Err(error) => return Err(error),
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
