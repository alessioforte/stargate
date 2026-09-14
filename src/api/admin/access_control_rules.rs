use super::{
    authorization::{self, Permission},
    extract_json,
};
use crate::act::access_control_rules::{
    AccessControlRulesResponse, EvaluateAccessControlRequest, EvaluateAccessControlResponse,
    UpdateAccessControlRulesRequest, ValidateAccessControlRulesRequest,
    ValidateAccessControlRulesResponse, evaluate_rules, read_rules, update_rules, validate_rules,
};
use crate::err::{ErrorCode, ErrorResponse};
use axum::Json;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use gate::Gate;
use std::sync::Arc;

#[utoipa::path(
    get,
    path = "/admin/access-control/rules",
    tags = ["Admin", "Access Control"],
    responses(
        (status = 200, description = "Access control rules retrieved successfully", body = AccessControlRulesResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_access_control_rules(req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::AccessControlRead)?;

    Ok(Json(read_rules()?).into_response())
}

#[utoipa::path(
    put,
    path = "/admin/access-control/rules",
    tags = ["Admin", "Access Control"],
    request_body = UpdateAccessControlRulesRequest,
    responses(
        (status = 200, description = "Access control rules updated successfully", body = AccessControlRulesResponse),
        (status = 400, description = "Invalid access control rules", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 409, description = "Policy revision conflict", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn update_access_control_rules(req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::AccessControlUpdate)?;

    let gate = gate_from_request(&req)?;
    let payload: UpdateAccessControlRulesRequest = extract_json(req).await?;
    let response = update_rules(payload)?;
    crate::etc::gate::reload_policy_engine(gate.as_ref()).await;

    Ok(Json(response).into_response())
}

#[utoipa::path(
    post,
    path = "/admin/access-control/rules/validate",
    tags = ["Admin", "Access Control"],
    request_body = ValidateAccessControlRulesRequest,
    responses(
        (status = 200, description = "Access control rules validation result", body = ValidateAccessControlRulesResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn validate_access_control_rules(req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::AccessControlValidate)?;

    let payload: ValidateAccessControlRulesRequest = extract_json(req).await?;

    Ok(Json(validate_rules(payload)).into_response())
}

#[utoipa::path(
    post,
    path = "/admin/access-control/rules/evaluate",
    tags = ["Admin", "Access Control"],
    request_body = EvaluateAccessControlRequest,
    responses(
        (status = 200, description = "Access control evaluation result", body = EvaluateAccessControlResponse),
        (status = 400, description = "Invalid evaluation request or policy file", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 403, description = "Forbidden", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn evaluate_access_control_rules(req: Request) -> Result<Response, ErrorResponse> {
    authorization::require(&req, Permission::AccessControlEvaluate)?;

    let payload: EvaluateAccessControlRequest = extract_json(req).await?;

    Ok(Json(evaluate_rules(payload)?).into_response())
}

fn gate_from_request(req: &Request) -> Result<Arc<Gate>, ErrorResponse> {
    req.extensions()
        .get::<Arc<Gate>>()
        .cloned()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AccessControlRuntimeUnavailable))
}
