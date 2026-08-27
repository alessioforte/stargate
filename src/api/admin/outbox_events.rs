use super::{SUPER_ADMIN, extract_path, extract_query};
use crate::err::{ErrorCode, ErrorResponse};
use crate::require_grants;
use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use db::ent::{OutboxEventFilter, OutboxEventRow};
use serde::{Deserialize, Serialize};

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutboxEventStatus {
    Pending,
    Published,
}

impl OutboxEventStatus {
    fn published(self) -> bool {
        matches!(self, Self::Published)
    }
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListOutboxEventsQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub status: Option<OutboxEventStatus>,
}

impl ListOutboxEventsQuery {
    fn into_filter(self) -> OutboxEventFilter {
        OutboxEventFilter {
            event_id: non_empty(self.event_id),
            operation_id: non_empty(self.operation_id),
            published: self.status.map(OutboxEventStatus::published),
        }
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutboxEventSchema {
    pub event_id: String,
    pub payload: serde_json::Value,
    pub seq: i64,
    pub operation_id: Option<String>,
    pub status: OutboxEventStatus,
    pub created_at: String,
    pub published_at: Option<String>,
}

impl TryFrom<OutboxEventRow> for OutboxEventSchema {
    type Error = serde_json::Error;

    fn try_from(row: OutboxEventRow) -> Result<Self, Self::Error> {
        let payload = serde_json::from_str(&row.payload)?;
        let status = if row.published_at.is_some() {
            OutboxEventStatus::Published
        } else {
            OutboxEventStatus::Pending
        };

        Ok(Self {
            event_id: row.event_id,
            payload,
            seq: row.seq,
            operation_id: row.operation_id,
            status,
            created_at: row.created_at.to_rfc3339(),
            published_at: row.published_at.map(|value| value.to_rfc3339()),
        })
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedOutboxEventsResponse {
    pub data: Vec<OutboxEventSchema>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[utoipa::path(
    get,
    path = "/admin/outbox-events",
    tags = ["Admin", "Audit Outbox"],
    summary = "Query audit outbox events",
    description = "Returns audit outbox rows without claiming, locking, publishing, or otherwise changing them. Requires super_admin.",
    params(ListOutboxEventsQuery),
    responses(
        (status = 200, description = "Outbox events retrieved successfully", body = PaginatedOutboxEventsResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_outbox_events(req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN);

    let query: ListOutboxEventsQuery = extract_query(&req)?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset = query.offset.unwrap_or(0).max(0);
    let filter = query.into_filter();

    let rows = crate::db::query_outbox_events(&filter, limit, offset)
        .await
        .map_err(ErrorResponse::internal)?;
    let total = crate::db::count_outbox_events(&filter)
        .await
        .map_err(ErrorResponse::internal)?;
    let data = rows
        .into_iter()
        .map(OutboxEventSchema::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(ErrorResponse::internal)?;

    Ok(Json(PaginatedOutboxEventsResponse {
        data,
        total,
        limit,
        offset,
    })
    .into_response())
}

#[utoipa::path(
    get,
    path = "/admin/outbox-events/{event_id}",
    tags = ["Admin", "Audit Outbox"],
    summary = "Get an audit outbox event",
    description = "Returns one audit outbox row without changing its delivery state. Requires super_admin.",
    params(("event_id" = String, Path, description = "Audit event ID")),
    responses(
        (status = 200, description = "Outbox event retrieved successfully", body = OutboxEventSchema),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 404, description = "Outbox event not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_outbox_event(mut req: Request) -> Result<Response, ErrorResponse> {
    require_grants!(req, SUPER_ADMIN);

    let event_id: String = extract_path(&mut req).await?;
    let row = crate::db::get_outbox_event_by_id(&event_id)
        .await
        .map_err(ErrorResponse::internal)?
        .ok_or_else(|| {
            ErrorResponse::new(ErrorCode::OutboxEventNotFound).with_param("id", event_id.clone())
        })?;
    let response = OutboxEventSchema::try_from(row).map_err(ErrorResponse::internal)?;

    Ok(Json(response).into_response())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    #[test]
    fn list_query_builds_trimmed_filter() {
        let query: ListOutboxEventsQuery = serde_json::from_value(serde_json::json!({
            "eventId": " 01JZ0000000000000000000001 ",
            "operationId": " ",
            "status": "pending"
        }))
        .expect("query must deserialize");

        assert_eq!(
            query.into_filter(),
            OutboxEventFilter {
                event_id: Some("01JZ0000000000000000000001".to_string()),
                operation_id: None,
                published: Some(false),
            }
        );
    }

    #[test]
    fn schema_exposes_payload_as_json_and_delivery_status() {
        let row = OutboxEventRow {
            event_id: "01JZ0000000000000000000001".to_string(),
            payload: r#"{"event_id":"01JZ0000000000000000000001"}"#.to_string(),
            seq: 42,
            operation_id: None,
            created_at: Utc
                .with_ymd_and_hms(2026, 7, 23, 12, 0, 0)
                .single()
                .expect("test timestamp"),
            published_at: None,
        };

        let schema = OutboxEventSchema::try_from(row).expect("valid payload");
        let json = serde_json::to_value(schema).expect("schema must serialize");

        assert_eq!(json["payload"]["event_id"], "01JZ0000000000000000000001");
        assert_eq!(json["status"], "pending");
        assert_eq!(json["seq"], 42);
        assert!(json["publishedAt"].is_null());
    }
}
