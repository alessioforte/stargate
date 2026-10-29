use super::*;
use crate::data::db::with_test_database;
use ::db::ent::OutboxEventFilter;

fn seed_path() -> String {
    std::env::temp_dir()
        .join(format!("stargate-seed-{}.yaml", ulid::Ulid::generate()))
        .to_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn gateway_seed_is_audited_once_and_existing_document_ignores_the_file() {
    with_test_database(async {
        let path = seed_path();
        let yaml = format!(
            "# Preserve the exact seed revision\n{}",
            Config::default().to_yaml_string().unwrap()
        );
        std::fs::write(&path, &yaml).unwrap();
        let first = load_gateway_document_from_path(&path).await.unwrap();
        assert_eq!(first.version, 1);
        assert_eq!(first.source, "seed");
        assert_eq!(first.content, yaml);
        assert_eq!(first.created_by, None);
        assert_eq!(
            prepare_gateway_config(&first.content)
                .unwrap()
                .config_revision,
            first.revision
        );
        let events = db::service()
            .query_outbox_events(&OutboxEventFilter::default(), 10, 0)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let event: serde_json::Value = serde_json::from_str(&events[0].payload).unwrap();
        assert_eq!(event["action"], "config_document.created");
        assert_eq!(event["metadata"]["source"], "seed");
        assert_eq!(event["actor"]["type"], "system");
        assert_eq!(event["scope"]["kind"], "control_plane");
        std::fs::write(&path, "invalid replacement seed").unwrap();
        assert_eq!(
            load_gateway_document_from_path(&path).await.unwrap().id,
            first.id
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "invalid replacement seed"
        );
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            load_gateway_document_from_path(&path).await.unwrap().id,
            first.id
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
async fn missing_gateway_seed_fails_without_creating_files_or_documents() {
    with_test_database(async {
        let path = format!("{}/missing/config.yaml", seed_path());
        let error = load_gateway_document_from_path(&path).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("no gateway configuration in the database and no seed file at {path}")
        );
        assert!(!std::path::Path::new(&path).exists());
        assert!(!std::path::Path::new(&path).parent().unwrap().exists());
        assert!(
            db::get_active_config_document(ConfigKind::Gateway)
                .await
                .unwrap()
                .is_none()
        );
    })
    .await;
}

#[tokio::test]
async fn invalid_active_gateway_document_fails_preparation_instead_of_using_the_seed() {
    with_test_database(async {
        let path = seed_path();
        let ConfigSaveOutcome::Saved(document) = db::save_config_document(
            ConfigKind::Gateway,
            "schema: invalid".into(),
            None,
            ConfigSource::CliImport,
            TrustedAuditContext::background(
                TrustedAuditBoundary::control_plane(),
                TrustedBackgroundActor::system(None),
            ),
        )
        .await
        .unwrap() else {
            panic!("new invalid active document")
        };
        assert!(prepare_gateway_config(&document.content).is_err());
        std::fs::write(&path, Config::default().to_yaml_string().unwrap()).unwrap();
        let active = load_gateway_document_from_path(&path).await.unwrap();
        assert_eq!(active.id, document.id);
        assert!(prepare_gateway_config(&active.content).is_err());
        std::fs::remove_file(path).unwrap();
    })
    .await;
}

#[tokio::test]
async fn invalid_gateway_seed_leaves_no_document_or_audit_and_a_corrected_seed_starts_at_one() {
    with_test_database(async {
        let path = seed_path();
        let mut config = serde_json::to_value(Config::default()).unwrap();
        config["ingress"]["limit"] = "missing-limit".into();
        let invalid: Config = serde_json::from_value(config).unwrap();
        std::fs::write(&path, invalid.to_yaml_string().unwrap()).unwrap();
        let error = load_gateway_document_from_path(&path).await.unwrap_err();
        assert!(error.to_string().contains("ingress.limit"));
        assert!(
            db::get_active_config_document(ConfigKind::Gateway)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            db::service()
                .count_outbox_events(&OutboxEventFilter::default())
                .await
                .unwrap(),
            0
        );
        std::fs::write(&path, Config::default().to_yaml_string().unwrap()).unwrap();
        let document = load_gateway_document_from_path(&path).await.unwrap();
        assert_eq!(document.version, 1);
        assert_eq!(
            db::service()
                .count_outbox_events(&OutboxEventFilter::default())
                .await
                .unwrap(),
            1
        );
        std::fs::remove_file(path).unwrap();
    })
    .await;
}
