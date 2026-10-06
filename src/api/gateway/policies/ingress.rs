use crate::err::{ErrorCode, ErrorResponse};
use crate::etc::{
    gate::RuntimeSnapshot, http::headers::retry_after_header_value, observability::telemetry,
};
use lim::LimitKind;

pub(in crate::api::gateway) async fn apply_ingress_limit(
    runtime: &RuntimeSnapshot,
    client_ip: &str,
    execution: &crate::api::gateway::lifecycle::Execution,
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
    // This namespace never intersects subject/org rate or quota buckets, even
    // when the ingress and resource policies reference the same named limit.
    let key = format!("ingress:ip:{client_ip}");
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
    let retry_after = retry_after_header_value(decision.retry_after);
    let mut error = ErrorResponse::new(ErrorCode::GatewayIngressRateLimitExceeded);
    error
        .insert_header("Retry-After", &retry_after)
        .insert_header("X-RateLimit-Limit", &decision.limit.to_string())
        .insert_header("X-RateLimit-Remaining", &decision.remaining.to_string())
        .insert_header("X-RateLimit-Scope", "ingress");
    Err(error)
}
