use super::*;
use crate::data::db::with_test_database;
use ::db::ent::{OutboxEventFilter, TrustedAuditContext};

fn seed_path() -> String {
    std::env::temp_dir()
        .join(format!("stargate-seed-{}.yaml", ulid::Ulid::generate()))
        .to_str()
        .unwrap()
        .to_string()
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
            TrustedAuditContext::system_control_plane(),
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
