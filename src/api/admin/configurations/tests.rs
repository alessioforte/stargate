use super::*;
use crate::api::admin::access_control_rules::{
    get_access_control_rules, update_access_control_rules,
};
use crate::api::admin::{AdminPrincipal, authorization::AdminAuthorization};
use crate::data::db::with_test_database;
use ::db::ent::{
    ConfigSaveOutcome, ConfigSource, OutboxEventFilter, TrustedAdminActor, TrustedAuditBoundary,
    TrustedAuditContext, TrustedAuditRequest, TrustedBackgroundActor,
};
use axum::{Router, body::Body, routing::get};
use http::StatusCode;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn context() -> TrustedAuditContext {
    TrustedAuditContext::admin_control_plane(
        TrustedAdminActor::admin("01JZ000000000000000000000A"),
        TrustedAuditRequest::from_http(ulid::Ulid::generate().to_string(), None, None, None),
    )
}

fn router(gate: Arc<Gate>) -> Router {
    Router::new()
        .route(
            "/admin/configurations",
            get(get_configurations).put(update_configurations),
        )
        .route(
            "/admin/access-control/rules",
            get(get_access_control_rules).put(update_access_control_rules),
        )
        .layer(axum::Extension(gate))
}

fn request(method: &str, uri: &str, payload: Option<Value>) -> Request {
    let body = payload
        .map(|value| Body::from(serde_json::to_vec(&value).unwrap()))
        .unwrap_or_else(Body::empty);
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap();
    req.extensions_mut().insert(AdminAuthorization::admin_key(
        AdminPrincipal::AdminKey {
            key_id: "01JZ000000000000000000000A".to_string(),
        },
        [
            Permission::ConfigurationsRead,
            Permission::ConfigurationsUpdate,
            Permission::AccessControlRead,
            Permission::AccessControlUpdate,
        ]
        .into_iter()
        .collect(),
    ));
    req.extensions_mut().insert(context());
    req
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

async fn seed(kind: ConfigKind, content: String) -> ConfigDocument {
    let outcome = db::save_config_document(
        kind,
        content,
        None,
        ConfigSource::Seed,
        TrustedAuditContext::background(
            TrustedAuditBoundary::control_plane(),
            TrustedBackgroundActor::system(None),
        ),
    )
    .await
    .unwrap();
    match outcome {
        ConfigSaveOutcome::Saved(document) => document,
        other => panic!("expected seed, got {other:?}"),
    }
}

async fn fixture() -> (Arc<Gate>, ConfigDocument) {
    let config = Config::default();
    let document = seed(ConfigKind::Gateway, config.to_yaml_string().unwrap()).await;
    (Arc::new(crate::gate::test_support::gate(config)), document)
}

fn routed_config() -> Value {
    let mut config = serde_json::to_value(Config::default()).unwrap();
    config["http"] = json!({
        "services": {"ready": {"kind": "direct_response", "status": 200, "body": {"text": "ready"}}},
        "routers": {"ready": {"match": {"path": {"prefix": "/"}}, "service": "ready"}},
    });
    config
}

#[tokio::test]
async fn get_returns_document_metadata_and_yaml_with_etags() {
    with_test_database(async {
        let (gate, document) = fixture().await;
        let app = router(gate);
        let response = app
            .clone()
            .oneshot(request("GET", "/admin/configurations", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[ETAG],
            format!("\"{}\"", document.revision)
        );
        let body = json_body(response).await;
        assert_eq!(body["revision"], document.revision);
        assert_eq!(body["version"], 1);
        assert_eq!(body["updatedAt"], document.created_at.to_rfc3339());
        assert!(body["updatedBy"].is_null());
        assert_eq!(body["config"]["schema"], "stargate/v1");
        let response = app
            .oneshot(request("GET", "/admin/configurations?format=yaml", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "application/yaml");
        assert_eq!(
            response.headers()[ETAG],
            format!("\"{}\"", document.revision)
        );
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            document.content
        );
    })
    .await;
}

#[tokio::test]
async fn missing_gateway_document_returns_the_catalogued_not_found_error() {
    with_test_database(async {
        let gate = Arc::new(crate::gate::test_support::gate(Config::default()));
        let response = router(gate)
            .oneshot(request("GET", "/admin/configurations", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = json_body(response).await;
        assert_eq!(body["code"], "configuration.not_found");
        assert_eq!(body["message"], "Gateway configuration not found");
    })
    .await;
}

#[tokio::test]
async fn updates_publish_after_saving_and_stale_revisions_change_neither_document_nor_runtime() {
    with_test_database(async {
        let (gate, initial) = fixture().await;
        let app = router(gate.clone());
        let payload = json!({"revision": initial.revision, "config": routed_config()});
        let response = app
            .clone()
            .oneshot(request(
                "PUT",
                "/admin/configurations",
                Some(payload.clone()),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let saved = json_body(response).await;
        assert_eq!(saved["activation"], "applied");
        assert_eq!(saved["version"], 2);
        assert_ne!(saved["revision"], initial.revision);
        assert_eq!(saved["updatedBy"], context().actor().id().unwrap());
        assert_eq!(gate.snapshot().version, 1);
        assert_eq!(gate.snapshot().core.graph.routers[0].service, "ready");
        let active = gate.snapshot();
        let response = app
            .clone()
            .oneshot(request("PUT", "/admin/configurations", Some(payload)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            json_body(response).await["code"],
            "configuration.revision_conflict"
        );
        assert!(Arc::ptr_eq(&active, &gate.snapshot()));
        let current = db::get_active_config_document(ConfigKind::Gateway)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current.version, 2);
        assert_eq!(current.revision, saved["revision"]);
        let events = db::service()
            .query_outbox_events(&OutboxEventFilter::default(), 10, 0)
            .await
            .unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|row| {
            let event: Value = serde_json::from_str(&row.payload).unwrap();
            event["action"] == "config_document.updated"
                && event["metadata"]["source"] == "admin_api"
        }));
        let response = app
            .oneshot(request(
                "PUT",
                "/admin/configurations",
                Some(json!({
                    "revision": current.revision, "config": saved["config"],
                })),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await["version"], 2);
        assert_eq!(gate.snapshot().version, 2);
        assert_eq!(
            db::service()
                .count_outbox_events(&OutboxEventFilter::default())
                .await
                .unwrap(),
            2
        );
        // A fresh runtime uses the persisted routes after a restart.
        let stored = db::get_active_config_document(ConfigKind::Gateway)
            .await
            .unwrap()
            .unwrap();
        let raw = RuntimeConfig::from_yaml_str(&stored.content).unwrap().raw;
        assert_eq!(
            crate::gate::test_support::gate(raw)
                .snapshot()
                .core
                .graph
                .routers[0]
                .service,
            "ready"
        );
    })
    .await;
}

#[tokio::test]
async fn invalid_submissions_and_unprepared_transports_do_not_save_or_publish() {
    use crate::gate::test_support::{SERVER_KEY, TlsFiles};
    with_test_database(async {
        let (gate, initial) = fixture().await;
        let active = gate.snapshot();
        let app = router(gate.clone());
        let mut candidates = Vec::new();
        for schema in [Value::Null, json!("stargate/v2"), json!("")] {
            let mut candidate = routed_config();
            candidate["schema"] = schema;
            candidates.push(candidate);
        }
        let mut missing_schema = routed_config();
        missing_schema.as_object_mut().unwrap().remove("schema");
        candidates.push(missing_schema);
        let files = TlsFiles::new();
        let valid = files.config("https://localhost:8443");
        let mut missing_key = valid.clone();
        missing_key.mtls.as_mut().unwrap().client_key_path = files.path("missing-key.pem");
        let mut mismatched = valid.clone();
        mismatched.mtls.as_mut().unwrap().client_key_path =
            files.write("other-key.pem", SERVER_KEY);
        candidates.extend([
            serde_json::to_value(missing_key).unwrap(),
            serde_json::to_value(mismatched).unwrap(),
        ]);
        for (path, value) in [
            ("primary_concurrency", json!(0)),
            ("response_header_timeout", json!("0s")),
        ] {
            let mut candidate = routed_config();
            candidate["runtime"][path] = value;
            candidates.push(candidate);
        }
        let mut invalid_ingress = routed_config();
        invalid_ingress["ingress"]["limit"] = json!("missing");
        candidates.push(invalid_ingress);
        for config in candidates {
            let response = app
                .clone()
                .oneshot(request(
                    "PUT",
                    "/admin/configurations",
                    Some(json!({
                        "revision": initial.revision, "config": config,
                    })),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert!(Arc::ptr_eq(&active, &gate.snapshot()));
        }
        let response = app
            .oneshot(request(
                "PUT",
                "/admin/configurations",
                Some(json!({
                    "revision": initial.revision, "config": routed_config(), "unexpected": true,
                })),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            db::get_active_config_document(ConfigKind::Gateway)
                .await
                .unwrap()
                .unwrap()
                .id,
            initial.id
        );
        assert_eq!(
            db::service()
                .count_outbox_events(&OutboxEventFilter::default())
                .await
                .unwrap(),
            1
        );
    })
    .await;
}

#[tokio::test]
async fn capacity_changes_are_saved_for_restart_without_publishing() {
    with_test_database(async {
        let (gate, initial) = fixture().await;
        let active = gate.snapshot();
        let mut config = routed_config();
        config["runtime"]["primary_concurrency"] = json!(2048);
        let response = router(gate.clone())
            .oneshot(request(
                "PUT",
                "/admin/configurations",
                Some(json!({
                    "revision": initial.revision, "config": config,
                })),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let saved = json_body(response).await;
        assert_eq!(saved["activation"], "restart_required");
        assert_eq!(saved["version"], 2);
        assert!(Arc::ptr_eq(&active, &gate.snapshot()));
        let stored = db::get_active_config_document(ConfigKind::Gateway)
            .await
            .unwrap()
            .unwrap();
        let raw = RuntimeConfig::from_yaml_str(&stored.content).unwrap().raw;
        let restarted = crate::gate::test_support::gate(raw);
        assert_eq!(restarted.resources.budgets.primary_concurrency, 2048);
        assert_eq!(restarted.snapshot().core.graph.routers[0].service, "ready");
    })
    .await;
}

#[tokio::test]
async fn policy_updates_publish_and_conflicts_leave_the_snapshot_unchanged() {
    with_test_database(async {
        let (gate, _) = fixture().await;
        let initial = seed(ConfigKind::Policies, String::new()).await;
        let app = router(gate.clone());
        let response = app
            .clone()
            .oneshot(request("GET", "/admin/access-control/rules", None))
            .await
            .unwrap();
        assert_eq!(json_body(response).await["revision"], initial.revision);
        let payload =
            json!({"revision": initial.revision, "content": "ALLOW user FOR \"reports:READ\";"});
        let response = app
            .clone()
            .oneshot(request(
                "PUT",
                "/admin/access-control/rules",
                Some(payload.clone()),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let saved = json_body(response).await;
        assert_eq!(gate.policy_snapshot.load().revision, saved["revision"]);
        assert!(gate.policy_snapshot.load().engine.evaluate_with_action(
            "user",
            "reports",
            &ace::ResourceAction::Read,
            &Default::default()
        ));
        let active = gate.policy_snapshot.load_full();
        let response = app
            .oneshot(request("PUT", "/admin/access-control/rules", Some(payload)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            json_body(response).await["code"],
            "access_control.revision_conflict"
        );
        assert!(Arc::ptr_eq(&active, &gate.policy_snapshot.load_full()));
        assert_eq!(
            db::get_active_config_document(ConfigKind::Policies)
                .await
                .unwrap()
                .unwrap()
                .version,
            2
        );
    })
    .await;
}
