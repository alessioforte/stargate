use super::plane::{Plane, PlaneSet};
use crate::err::{ErrorCode, ErrorType};
use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;
use utoipa::{OpenApi, openapi::Server};

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorCatalogEntry {
    #[schema(value_type = String, example = "user.not_found")]
    pub code: ErrorCode,
    pub status: u16,
    #[serde(rename = "type")]
    pub error_type: ErrorType,
    pub message: &'static str,
    pub link: String,
}

/// Documents only the planes served by the receiving listener.
pub(super) fn router(planes: PlaneSet) -> Router {
    Router::new()
        .route("/docs", get(get_api_doc))
        .route("/docs/errors", get(get_error_catalog))
        .with_state(planes)
}

pub(super) fn openapi(planes: PlaneSet) -> utoipa::openapi::OpenApi {
    let mut doc = utoipa::openapi::OpenApi::default();
    doc.info = utoipa::openapi::Info::new("Stargate APIs ✨", env!("CARGO_PKG_VERSION"));
    doc.info.description = Some("APIs for user authentication and management in Stargate.".into());
    if planes.contains(Plane::Identity) || planes.contains(Plane::Admin) {
        doc.merge(super::DocumentationApiDoc::openapi());
    }
    if planes.contains(Plane::Management) {
        doc.merge(super::ManagementApiDoc::openapi());
    }
    if planes.contains(Plane::Identity) {
        doc.merge(super::IdentityApiDoc::openapi());
    }
    if planes.contains(Plane::Admin) {
        doc.merge(super::AdminApiDoc::openapi());
    }
    doc
}

#[utoipa::path(
    get,
    path = "/docs",
    tags = ["Documentation"],
    responses(
        (status = 200, description = "OK")
    )
)]
pub async fn get_api_doc(State(planes): State<PlaneSet>) -> Json<utoipa::openapi::OpenApi> {
    let mut doc = openapi(planes);
    doc.servers = Some(vec![Server::new(crate::conf::public_base_url())]);
    Json(doc)
}

#[utoipa::path(
    get,
    path = "/docs/errors",
    tags = ["Documentation"],
    responses(
        (status = 200, description = "Published API error catalog", body = [ErrorCatalogEntry])
    )
)]
pub async fn get_error_catalog() -> Json<Vec<ErrorCatalogEntry>> {
    Json(
        ErrorCode::ALL
            .iter()
            .map(|code| ErrorCatalogEntry {
                code: *code,
                status: code.status().as_u16(),
                error_type: code.error_type(),
                message: code.message(),
                link: code.url(),
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_admin_operation_documents_security_state_unavailability() {
        let document = serde_json::to_value(openapi(PlaneSet::of(Plane::Admin))).unwrap();
        let paths = document["paths"].as_object().unwrap();
        assert!(paths.len() > 30);
        for (path, item) in paths.iter().filter(|(path, _)| path.starts_with("/admin/")) {
            for (method, operation) in item.as_object().unwrap() {
                let response = &operation["responses"]["503"];
                assert!(
                    response["description"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("auth.state_unavailable"),
                    "{method} {path}"
                );
                assert!(
                    response["headers"]["Retry-After"].is_object(),
                    "{method} {path}"
                );
                assert!(
                    response["content"]["application/json"]["schema"].is_object(),
                    "{method} {path}"
                );
            }
        }
    }
}
