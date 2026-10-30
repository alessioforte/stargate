use super::*;
use crate::data::db::with_test_database;
use crate::iam::authz::rules::compile_policy_snapshot;
use ::db::ent::{OutboxEventFilter, TrustedAuditContext};

fn seed_path() -> String {
    std::env::temp_dir()
        .join(format!("stargate-policy-seed-{}", ulid::Ulid::generate()))
        .to_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn missing_policy_seed_creates_an_empty_document_without_creating_files() {
    with_test_database(async {
        let path = format!("{}/missing/policies", seed_path());
        let document = load_policies_document_from_path(&path).await.unwrap();
        assert_eq!(document.content, "");
        assert_eq!(document.version, 1);
        assert_eq!(document.source, "seed");
        assert!(compile_policy_snapshot(&document.content).is_ok());
        assert!(!std::path::Path::new(&path).parent().unwrap().exists());
    })
    .await;
}

#[tokio::test]
async fn invalid_active_policies_fail_compilation_instead_of_using_the_seed() {
    with_test_database(async {
        let path = seed_path();
        let ConfigSaveOutcome::Saved(document) = db::save_config_document(
            ConfigKind::Policies,
            "ALLOW user WHEN invalid;".into(),
            None,
            ConfigSource::CliImport,
            TrustedAuditContext::system_control_plane(),
        )
        .await
        .unwrap() else {
            panic!("new invalid active document")
        };
        assert!(compile_policy_snapshot(&document.content).is_err());
        std::fs::write(&path, "").unwrap();
        assert_eq!(
            load_policies_document_from_path(&path).await.unwrap().id,
            document.id
        );
        std::fs::remove_file(path).unwrap();
    })
    .await;
}

#[tokio::test]
async fn invalid_policy_seed_leaves_no_document_or_audit_and_a_corrected_seed_starts_at_one() {
    with_test_database(async {
        let path = seed_path();
        std::fs::write(&path, "\nALLOW user WHEN invalid;").unwrap();
        let error = load_policies_document_from_path(&path).await.unwrap_err();
        assert!(error.to_string().contains("line 2:"));
        assert!(
            db::get_active_config_document(ConfigKind::Policies)
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
        std::fs::write(&path, "ALLOW user FOR \"reports:READ\";").unwrap();
        let document = load_policies_document_from_path(&path).await.unwrap();
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
