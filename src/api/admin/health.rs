use crate::err::ErrorResponse;
use crate::require_grants;
use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use serde::Serialize;
use std::time::Instant;
#[cfg(feature = "redis")]
use store::Store;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ComponentStatus {
    status: &'static str,
    latency_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
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
pub struct AdminHealth {
    name: &'static str,
    version: &'static str,
    status: &'static str,
    checked_at: String,
    runtime_profile: &'static str,
    database_backend: &'static str,
    state_backend: &'static str,
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
        (status = 200, description = "All dependencies healthy", body = AdminHealth),
        (status = 503, description = "One or more dependencies unhealthy", body = AdminHealth),
    )
)]
pub async fn get_admin_health(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, "super_admin");

    let version = env!("CARGO_PKG_VERSION");

    let database_started = Instant::now();
    let database = match crate::db::ping().await {
        Ok(_) => ComponentStatus {
            status: "healthy",
            latency_ms: database_started.elapsed().as_secs_f64() * 1000.0,
            detail: None,
        },
        Err(e) => ComponentStatus {
            status: "unhealthy",
            latency_ms: database_started.elapsed().as_secs_f64() * 1000.0,
            detail: Some(e.to_string()),
        },
    };

    #[cfg(feature = "redis")]
    let redis = {
        let store = crate::etc::store::use_store();
        let redis_started = Instant::now();
        match store.ping().await {
            Ok(_) => ComponentStatus {
                status: "healthy",
                latency_ms: redis_started.elapsed().as_secs_f64() * 1000.0,
                detail: None,
            },
            Err(e) => ComponentStatus {
                status: "unhealthy",
                latency_ms: redis_started.elapsed().as_secs_f64() * 1000.0,
                detail: Some(e.to_string()),
            },
        }
    };

    #[cfg(feature = "memory")]
    let store_stats = {
        let store = crate::etc::store::use_store();
        let stats = store.get_storage_stats();
        StoreStats {
            total_keys: stats.total_keys,
            estimated_memory_bytes: stats.estimated_memory_bytes,
            cache_hit_ratio: store.get_cache_hit_ratio(),
        }
    };

    let all_healthy = database.status == "healthy" && {
        #[cfg(feature = "redis")]
        {
            redis.status == "healthy"
        }
        #[cfg(not(feature = "redis"))]
        {
            true
        }
    };

    let overall = if all_healthy { "healthy" } else { "unhealthy" };

    let body = AdminHealth {
        name: "stargate",
        version,
        status: overall,
        checked_at: chrono::Utc::now().to_rfc3339(),
        runtime_profile: crate::etc::profile::COMPILED_PROFILE,
        database_backend: crate::etc::profile::COMPILED_DB_BACKEND,
        state_backend: crate::etc::profile::COMPILED_STATE_BACKEND,
        database,
        #[cfg(feature = "redis")]
        redis,
        #[cfg(feature = "memory")]
        store: store_stats,
    };

    let status = if all_healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    Ok((status, Json(body)).into_response())
}
