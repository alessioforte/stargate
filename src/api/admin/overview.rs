use super::SUPER_ADMIN;
use crate::err::ErrorResponse;
use crate::require_grants;
use axum::Json;
use axum::extract::Request;
use chrono::Utc;
use gate::Gate;
use serde::Serialize;
use std::sync::Arc;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceCount {
    total: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnabledResourceCount {
    total: i64,
    enabled: i64,
    disabled: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevocableResourceCount {
    total: i64,
    active: i64,
    revoked: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminResourceOverview {
    users: ResourceCount,
    organizations: ResourceCount,
    service_accounts: ResourceCount,
    oauth_clients: EnabledResourceCount,
    api_keys: RevocableResourceCount,
    admin_keys: RevocableResourceCount,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditOverview {
    mode: &'static str,
    pending: i64,
    oldest_pending_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayOverview {
    config_version: u64,
    policy_revision: Option<String>,
    routers: usize,
    services: usize,
    upstreams: usize,
    targets: usize,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminOverview {
    generated_at: String,
    resources: AdminResourceOverview,
    audit: AuditOverview,
    gateway: GatewayOverview,
}

fn revocable(total: i64, revoked: i64) -> RevocableResourceCount {
    RevocableResourceCount {
        total,
        active: total.saturating_sub(revoked),
        revoked,
    }
}

#[utoipa::path(
    get,
    path = "/admin/overview",
    tags = ["Admin"],
    summary = "Admin overview",
    description = "Returns a compact snapshot of resource counts, audit delivery backlog, and the active gateway graph.",
    responses(
        (status = 200, description = "Overview retrieved successfully", body = AdminOverview),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn get_admin_overview(req: Request) -> Result<Json<AdminOverview>, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN);

    let gate = req
        .extensions()
        .get::<Arc<Gate>>()
        .cloned()
        .ok_or_else(|| ErrorResponse::internal("gateway runtime is unavailable"))?;
    let stats = crate::db::get_admin_overview_stats()
        .await
        .map_err(ErrorResponse::internal)?;
    let graph = gate.http_graph.load();

    Ok(Json(AdminOverview {
        generated_at: Utc::now().to_rfc3339(),
        resources: AdminResourceOverview {
            users: ResourceCount {
                total: stats.users_total,
            },
            organizations: ResourceCount {
                total: stats.organizations_total,
            },
            service_accounts: ResourceCount {
                total: stats.service_accounts_total,
            },
            oauth_clients: EnabledResourceCount {
                total: stats.oauth_clients_total,
                enabled: stats.oauth_clients_enabled,
                disabled: stats
                    .oauth_clients_total
                    .saturating_sub(stats.oauth_clients_enabled),
            },
            api_keys: revocable(stats.api_keys_total, stats.api_keys_revoked),
            admin_keys: revocable(stats.admin_keys_total, stats.admin_keys_revoked),
        },
        audit: AuditOverview {
            mode: crate::aud::delivery_mode(),
            pending: stats.audit_pending,
            oldest_pending_at: stats
                .audit_oldest_pending_at
                .map(|value| value.to_rfc3339()),
        },
        gateway: GatewayOverview {
            config_version: crate::etc::gate::get_config_version(),
            policy_revision: crate::etc::gate::get_policy_revision(),
            routers: graph.routers.len(),
            services: graph.services.len(),
            upstreams: graph.upstreams.len(),
            targets: graph
                .upstreams
                .values()
                .map(|upstream| upstream.targets.len())
                .sum(),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::revocable;

    #[test]
    fn revocable_counts_active_resources() {
        let count = revocable(10, 3);
        assert_eq!(count.total, 10);
        assert_eq!(count.active, 7);
        assert_eq!(count.revoked, 3);
    }
}
