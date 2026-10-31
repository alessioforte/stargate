use super::authorization::{self, Permission};
use crate::err::ErrorResponse;
use crate::gate::Gate;
use crate::srv::health::{ComponentHealth, DependencyHealth, check_dependencies};
use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ConfigStatus {
    gateway_revision: String,
    policies_revision: String,
}

impl ConfigStatus {
    fn from_gate(gate: &Gate) -> Self {
        Self {
            gateway_revision: gate.snapshot().config_revision.clone(),
            policies_revision: gate.policy_snapshot.load().revision.clone(),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ComponentStatus {
    status: &'static str,
    latency_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl From<ComponentHealth> for ComponentStatus {
    fn from(check: ComponentHealth) -> Self {
        Self {
            status: if check.is_healthy() {
                "healthy"
            } else {
                "unhealthy"
            },
            latency_ms: check.latency_ms,
            detail: check.error,
        }
    }
}

/// Capabilities switched on for this server (`src/conf/features.rs`).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct FeatureStatus {
    signup_mode: &'static str,
    signup_allowed_domains: Vec<String>,
    passwordless_login: bool,
    console: bool,
    social_providers: Vec<&'static str>,
}

#[cfg(feature = "memory")]
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct StoreStats {
    total_keys: usize,
    estimated_memory_bytes: usize,
    cache_hit_ratio: f64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct LimitsStatus {
    store_degraded: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminHealth {
    name: &'static str,
    version: &'static str,
    status: &'static str,
    checked_at: String,
    runtime_profile: &'static str,
    database_backend: &'static str,
    state_backend: &'static str,
    features: FeatureStatus,
    config: ConfigStatus,
    limits: LimitsStatus,
    database: ComponentStatus,
    #[cfg(feature = "redis")]
    redis: ComponentStatus,
    #[cfg(feature = "memory")]
    store: StoreStats,
}

#[utoipa::path(
    get,
    path = "/admin/health",
    tags = ["Admin"],
    summary = "Service Health",
    description = "Returns detailed status of Stargate and its dependencies. Requires super_admin.",
    responses(
        (status = 200, description = "Diagnostics available (inspect dependency and limit statuses)", body = AdminHealth),
        (status = 503, description = "Database unhealthy", body = AdminHealth),
    )
)]
pub async fn get_admin_health(req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::HealthRead)?;
    let gate = req
        .extensions()
        .get::<Arc<Gate>>()
        .cloned()
        .ok_or_else(|| ErrorResponse::internal("Gateway runtime extension is missing"))?;

    Ok(health_response(&gate, check_dependencies().await))
}

fn health_response(gate: &Gate, dependencies: DependencyHealth) -> Response {
    let version = env!("CARGO_PKG_VERSION");
    let all_healthy = dependencies.is_healthy();
    let database_healthy = dependencies.database.is_healthy();
    let limits = LimitsStatus {
        store_degraded: gate.snapshot().limit_store_health.is_degraded(),
    };

    #[cfg(feature = "memory")]
    let store_stats = {
        let store = crate::data::store::use_store();
        let stats = store.get_storage_stats();
        StoreStats {
            total_keys: stats.total_keys,
            estimated_memory_bytes: stats.estimated_memory_bytes,
            cache_hit_ratio: store.get_cache_hit_ratio(),
        }
    };

    let overall = if !all_healthy {
        "unhealthy"
    } else if limits.store_degraded {
        "degraded"
    } else {
        "healthy"
    };
    let features = crate::conf::features::features();

    let body = AdminHealth {
        name: "stargate",
        version,
        status: overall,
        checked_at: chrono::Utc::now().to_rfc3339(),
        runtime_profile: crate::srv::profile::COMPILED_PROFILE,
        database_backend: crate::srv::profile::COMPILED_DB_BACKEND,
        state_backend: crate::srv::profile::COMPILED_STATE_BACKEND,
        features: FeatureStatus {
            signup_mode: features.signup.as_str(),
            signup_allowed_domains: features.signup_allowed_domains.clone(),
            passwordless_login: features.passwordless_login,
            console: features.console,
            social_providers: features.social_providers(),
        },
        config: ConfigStatus::from_gate(gate),
        limits,
        database: dependencies.database.into(),
        #[cfg(feature = "redis")]
        redis: dependencies.redis.into(),
        #[cfg(feature = "memory")]
        store: store_stats,
    };

    let status = if database_healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "memory")]
    use crate::gate::{apply_policy_document, prepare_config, test_support};
    #[cfg(feature = "memory")]
    use gate::cfg::Config;

    #[cfg(feature = "redis")]
    #[test]
    fn redis_failure_is_reported_as_unhealthy() {
        let component = ComponentStatus::from(ComponentHealth {
            latency_ms: 1.0,
            error: Some("Redis unavailable".into()),
        });
        let json = serde_json::to_value(component).unwrap();
        assert_eq!(json["status"], "unhealthy");
        assert_eq!(json["detail"], "Redis unavailable");
    }

    #[cfg(feature = "memory")]
    #[test]
    fn health_reports_active_revisions_and_pinned_snapshots_keep_their_revision() {
        let initial = Config::default();
        let gate = test_support::gate(initial.clone());
        let pinned = gate.snapshot();
        assert_eq!(
            pinned.config_revision,
            db::ent::config_revision(&initial.to_yaml_string().unwrap())
        );
        let mut changed = initial;
        changed.ingress.timeout = "300ms".to_string();
        gate.activate(prepare_config(changed.clone()).unwrap())
            .unwrap();
        apply_policy_document(&gate, "ALLOW user FOR \"health:READ\";").unwrap();
        let status = serde_json::to_value(ConfigStatus::from_gate(&gate)).unwrap();
        assert_eq!(
            status["gatewayRevision"],
            db::ent::config_revision(&changed.to_yaml_string().unwrap())
        );
        assert_eq!(
            status["policiesRevision"],
            gate.policy_snapshot.load().revision
        );
        assert_ne!(status["gatewayRevision"], pinned.config_revision);
        let mut needs_restart = changed;
        needs_restart.runtime.primary_concurrency += 1;
        assert!(
            gate.activate(prepare_config(needs_restart).unwrap())
                .is_err()
        );
        assert_eq!(
            serde_json::to_value(ConfigStatus::from_gate(&gate)).unwrap(),
            status
        );
    }
}

#[cfg(all(test, feature = "memory"))]
mod fail_open_tests {
    use super::*;
    use crate::{data::db, gate::test_support};
    use axum::{Extension, Router, body::Body, middleware::from_fn, routing::get};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    struct Unavailable;
    #[async_trait::async_trait]
    impl lim::RateLimit for Unavailable {
        fn kind(&self) -> lim::LimitKind {
            lim::LimitKind::Rate
        }
        async fn check(&self, _: &str, _: u64) -> lim::Result<lim::RateLimitDecision> {
            Err(lim::RateLimitError::BackendError("unavailable".into()))
        }
    }

    #[tokio::test]
    async fn database_admin_key_reaches_degraded_health_without_fabricated_limit_headers() {
        db::with_test_database(async {
            let raw_key = "test-admin-key";
            db::create_admin_key(
                &pw::hash_api_key(raw_key),
                None,
                vec!["health:read".into()],
                ::db::ent::TrustedAuditContext::admin_control_plane(
                    ::db::ent::TrustedAdminActor::admin("01JZ000000000000000000000A"),
                    ::db::ent::TrustedAuditRequest::from_http(
                        ulid::Ulid::generate().to_string(),
                        None,
                        None,
                        None,
                    ),
                ),
            )
            .await
            .unwrap();
            let mut limiter = lim::Limiter::new();
            for name in ["ingress", "default"] {
                limiter.add_limit(name.into(), Box::new(Unavailable));
            }
            let gate = Arc::new(test_support::gate_with_limiter(
                gate::cfg::Config::default(),
                limiter,
            ));
            // Use the real authorization and admission middleware. Only SQL health
            // is supplied here: test-scoped SQL is deliberately not the global pool.
            let app = Router::new()
                .route(
                    "/admin/health",
                    get(|req: Request| async move {
                        authorization::require(&req, Permission::HealthRead)?;
                        let gate = req.extensions().get::<Arc<Gate>>().unwrap();
                        Ok::<_, ErrorResponse>(health_response(
                            gate,
                            DependencyHealth {
                                database: ComponentHealth {
                                    latency_ms: 0.0,
                                    error: None,
                                },
                            },
                        ))
                    }),
                )
                .layer(from_fn(super::super::extract_admin_authorization))
                .layer(from_fn(
                    crate::api::middleware::rate_limit::ingress_limit_middleware,
                ))
                .layer(Extension(gate));
            let response = app
                .oneshot(
                    Request::builder()
                        .uri("/admin/health")
                        .header("x-api-key", raw_key)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!response.headers().contains_key("x-ratelimit-limit"));
            let json: serde_json::Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(json["limits"]["storeDegraded"], true);
            assert_eq!(json["status"], "degraded");
        })
        .await;
    }
}
