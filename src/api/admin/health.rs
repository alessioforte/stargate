use crate::err::ErrorResponse;
use actix_web::{HttpResponse, get};
use actix_web_grants::protect;
use serde::Serialize;
use store::Store;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ComponentStatus {
    status: &'static str,
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
struct AdminHealth {
    name: &'static str,
    version: &'static str,
    status: &'static str,
    database: ComponentStatus,
    #[cfg(feature = "redis")]
    redis: ComponentStatus,
    #[cfg(feature = "memory")]
    store: StoreStats,
}

#[utoipa::path(
    context_path = "/admin",
    path = "/health",
    tags = ["Admin"],
    summary = "Service Health",
    description = "Returns detailed status of Stargate and its dependencies. Requires super_admin.",
    responses(
        (status = 200, description = "All dependencies healthy", body = AdminHealth),
        (status = 503, description = "One or more dependencies unhealthy", body = AdminHealth),
    )
)]
#[get("/health")]
#[protect("super_admin")]
pub async fn get() -> Result<HttpResponse, ErrorResponse> {
    let version = env!("CARGO_PKG_VERSION");

    // ── Database ──────────────────────────────────────────────────────────────
    let database = match crate::db::ping().await {
        Ok(_) => ComponentStatus {
            status: "healthy",
            detail: None,
        },
        Err(e) => ComponentStatus {
            status: "unhealthy",
            detail: Some(e.to_string()),
        },
    };

    // ── Redis (feature-gated) ─────────────────────────────────────────────────
    #[cfg(feature = "redis")]
    let redis = {
        let store = crate::etc::store::use_store();
        match store.ping().await {
            Ok(_) => ComponentStatus {
                status: "healthy",
                detail: None,
            },
            Err(e) => ComponentStatus {
                status: "unhealthy",
                detail: Some(e.to_string()),
            },
        }
    };

    // ── Memory store stats (feature-gated) ────────────────────────────────────
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

    // ── Overall status ────────────────────────────────────────────────────────
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
        database,
        #[cfg(feature = "redis")]
        redis,
        #[cfg(feature = "memory")]
        store: store_stats,
    };

    if all_healthy {
        Ok(HttpResponse::Ok().json(body))
    } else {
        Ok(HttpResponse::ServiceUnavailable().json(body))
    }
}
