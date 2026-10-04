use crate::{
    err::{ErrorCode, ErrorResponse},
    etc::gate::{Gate, RuntimeSnapshot},
};
use axum::extract::Request;
use lim::{LimitKind, Limiter};
use serde_json::Value;
use std::sync::Arc;

pub(super) fn runtime(req: &Request) -> Result<Arc<RuntimeSnapshot>, ErrorResponse> {
    req.extensions()
        .get::<Arc<RuntimeSnapshot>>()
        .cloned()
        .or_else(|| {
            req.extensions()
                .get::<Arc<Gate>>()
                .map(|gate| gate.snapshot())
        })
        .ok_or_else(|| ErrorResponse::internal("Gateway runtime extension is missing"))
}

pub(super) fn validate(
    limiter: &Limiter,
    attrs: &Value,
    organization: bool,
) -> Result<(), ErrorResponse> {
    for (field, kind) in [
        ("rate_limit", LimitKind::Rate),
        ("quota", LimitKind::Quota),
        ("rateLimit", LimitKind::Rate),
    ] {
        if field == "rateLimit" && !organization {
            continue;
        }
        let Some(value) = attrs.get(field).filter(|value| !value.is_null()) else {
            continue;
        };
        let invalid = || {
            ErrorResponse::new(ErrorCode::RequestInvalid)
                .with_param("field", format!("attrs.{field}"))
                .with_param("expected", kind.to_string())
        };
        let name = value.as_str().ok_or_else(invalid)?;
        limiter.validate_limit(name, kind).map_err(|_| {
            invalid().with_param("limit", name.chars().take(64).collect::<String>())
        })?;
    }
    Ok(())
}

#[cfg(all(test, feature = "memory"))]
mod tests {
    use super::*;
    use crate::etc::gate::{prepare_config, test_support};
    use gate::cfg::Config;
    use serde_json::json;

    #[tokio::test]
    async fn admin_attrs_require_existing_correctly_typed_names_and_allow_explicit_removal() {
        let mut value = serde_json::to_value(Config::default()).unwrap();
        value["limits"]["daily"] =
            json!({"strategy":"quota_tracker","params":{"limit":3,"period":"day"}});
        let gate = test_support::gate(serde_json::from_value(value).unwrap());
        let runtime = gate.snapshot();
        for attrs in [
            json!({}),
            json!({"quota":null,"rate_limit":null}),
            json!({"rate_limit":"default","quota":"daily"}),
            json!({"rateLimit":"default"}),
        ] {
            validate(&runtime.core.limiter, &attrs, true).unwrap();
        }
        for (attrs, field) in [
            (json!({"rate_limit":"missing"}), "rate_limit"),
            (json!({"rateLimit":"missing"}), "rateLimit"),
            (json!({"quota":"missing"}), "quota"),
            (json!({"rate_limit":""}), "rate_limit"),
            (json!({"rate_limit":"daily"}), "rate_limit"),
            (json!({"quota":"default"}), "quota"),
            (json!({"rate_limit":false}), "rate_limit"),
            (json!({"quota":5}), "quota"),
        ] {
            let error = validate(&runtime.core.limiter, &attrs, true).unwrap_err();
            assert_eq!(error.code, ErrorCode::RequestInvalid);
            assert_eq!(error.params["field"], format!("attrs.{field}"));
        }
        assert_eq!(
            runtime
                .core
                .limiter
                .check("daily", "test", Some(3))
                .await
                .unwrap()
                .remaining,
            0
        );
    }

    #[tokio::test]
    async fn admin_validation_uses_the_pinned_iam_runtime_and_new_writes_use_the_new_config() {
        let mut value = serde_json::to_value(Config::default()).unwrap();
        value["limits"]["old"] = value["limits"]["default"].clone();
        let gate = Arc::new(test_support::gate(serde_json::from_value(value).unwrap()));
        let pinned = gate.snapshot();
        let mut request = Request::new(axum::body::Body::empty());
        request.extensions_mut().insert(gate.clone());
        request.extensions_mut().insert(pinned.clone());
        gate.activate(prepare_config(Config::default()).unwrap())
            .unwrap();
        let old = runtime(&request).unwrap();
        assert!(Arc::ptr_eq(&old, &pinned));
        validate(&old.core.limiter, &json!({"rate_limit":"old"}), false).unwrap();
        request.extensions_mut().remove::<Arc<RuntimeSnapshot>>();
        let new = runtime(&request).unwrap();
        assert!(validate(&new.core.limiter, &json!({"rate_limit":"old"}), false).is_err());
    }
}
