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
        let yaml = Config::default().to_yaml_string().unwrap();
        std::fs::write(&path, &yaml).unwrap();
        let first = load_gateway_document_from_path(&path).await.unwrap();
        assert_eq!(first.version, 1);
        assert_eq!(first.source, "seed");
        assert_eq!(first.content, yaml);
        assert_eq!(first.created_by, None);
        assert!(prepare_gateway_config(&first.content).is_ok());
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
        std::fs::write(&path, "schema: invalid").unwrap();
        let document = load_gateway_document_from_path(&path).await.unwrap();
        assert!(prepare_gateway_config(&document.content).is_err());
        std::fs::write(&path, Config::default().to_yaml_string().unwrap()).unwrap();
        let active = load_gateway_document_from_path(&path).await.unwrap();
        assert_eq!(active.id, document.id);
        assert!(prepare_gateway_config(&active.content).is_err());
        std::fs::remove_file(path).unwrap();
    })
    .await;
}
