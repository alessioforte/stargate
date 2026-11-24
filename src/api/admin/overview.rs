use super::authorization::{self, Permission};
use crate::err::ErrorResponse;
use crate::gate::Gate;
use crate::iam::sess::summary::SessionSummary;
use axum::Json;
use axum::extract::Request;
use chrono::{DateTime, Utc};
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

/// Estimated totals from the last completed reconciliation of the session
/// indexes. They lag behind sessions opened or ended since `reconciledAt`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionOverview {
    active_sessions: u64,
    active_users: u64,
    /// When the reconciliation that produced these totals completed.
    reconciled_at: String,
}

impl SessionOverview {
    fn from_summary(summary: SessionSummary) -> Result<Self, ErrorResponse> {
        let reconciled_at = i64::try_from(summary.reconciled_at_unix)
            .ok()
            .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0))
            .ok_or_else(|| ErrorResponse::internal("invalid session reconciliation time"))?;
        Ok(Self {
            active_sessions: summary.active_sessions,
            active_users: summary.active_users,
            reconciled_at: reconciled_at.to_rfc3339(),
        })
    }
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
    /// `null` until the first reconciliation completes.
    sessions: Option<SessionOverview>,
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
    description = "Returns a compact snapshot of resource counts, estimated active-session totals with the time of their last reconciliation, audit delivery backlog, and the active gateway graph.",
    responses(
        (status = 503, description = "Security state unavailable (`auth.state_unavailable`); retry after 10 seconds", body = ErrorResponse, headers(("Retry-After" = String, description = "10 seconds for auth.state_unavailable"))),
        (status = 200, description = "Overview retrieved successfully", body = AdminOverview),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse),
    )
)]
pub async fn get_admin_overview(req: Request) -> Result<Json<AdminOverview>, ErrorResponse> {
    authorization::require(&req, Permission::OverviewRead)?;

    let gate = req
        .extensions()
        .get::<Arc<Gate>>()
        .cloned()
        .ok_or_else(|| ErrorResponse::internal("gateway runtime is unavailable"))?;
    let stats = crate::data::db::get_admin_overview_stats()
        .await
        .map_err(ErrorResponse::internal)?;
    let sessions = crate::iam::sess::summary::summary()
        .await?
        .map(SessionOverview::from_summary)
        .transpose()?;
    let runtime = req
        .extensions()
        .get::<Arc<crate::gate::RuntimeSnapshot>>()
        .cloned()
        .unwrap_or_else(|| gate.snapshot());
    let graph = &runtime.core.graph;
    let policies = gate.policy_snapshot.load();

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
        sessions,
        audit: AuditOverview {
            mode: crate::aud::delivery_mode(),
            pending: stats.audit_pending,
            oldest_pending_at: stats
                .audit_oldest_pending_at
                .map(|value| value.to_rfc3339()),
        },
        gateway: GatewayOverview {
            config_version: runtime.version,
            policy_revision: Some(policies.revision.clone()),
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
    use super::{SessionOverview, SessionSummary, revocable};

    #[test]
    fn session_totals_carry_their_reconciliation_time() {
        let overview = SessionOverview::from_summary(SessionSummary {
            active_sessions: 3,
            active_users: 2,
            reconciled_at_unix: 1_800_000_000,
        })
        .unwrap();
        assert_eq!(
            serde_json::to_value(overview).unwrap(),
            serde_json::json!({
                "activeSessions": 3,
                "activeUsers": 2,
                "reconciledAt": "2027-01-15T08:00:00+00:00",
            })
        );
    }

    #[test]
    fn revocable_counts_active_resources() {
        let count = revocable(10, 3);
        assert_eq!(count.total, 10);
        assert_eq!(count.active, 7);
        assert_eq!(count.revoked, 3);
    }
}
