use super::*;
use crate::ent::{ConfigSource, config_revision};
use chrono::Utc;

async fn exercise_history(pool: crate::backend::Pool) {
    // A connection-local table isolates histories from other PostgreSQL tests.
    let mut tx = pool.begin().await.unwrap();
    let mut schema = sqlx::QueryBuilder::<crate::backend::Db>::new(
        "CREATE TEMP TABLE config_documents (
            id TEXT PRIMARY KEY, kind TEXT NOT NULL, version BIGINT NOT NULL,
            revision TEXT NOT NULL, content TEXT NOT NULL, source TEXT NOT NULL,
            created_by TEXT, created_at ",
    );
    #[cfg(feature = "sqlite")]
    schema.push("TEXT");
    #[cfg(feature = "postgres")]
    schema.push("TIMESTAMPTZ");
    schema.push(" NOT NULL)");
    schema.build().execute(&mut *tx).await.unwrap();
    let repo = ConfigDocumentRepository::new();
    assert!(
        repo.list(&mut *tx, ConfigKind::Gateway, 20, 0)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        repo.get(&mut *tx, ConfigKind::Gateway, 1)
            .await
            .unwrap()
            .is_none()
    );
    let now = Utc::now();
    let mut documents = Vec::new();
    for (kind, version, source, content) in [
        (
            ConfigKind::Gateway,
            1,
            ConfigSource::Seed,
            "# Initial configuration\n",
        ),
        (
            ConfigKind::Gateway,
            2,
            ConfigSource::AdminApi,
            "# Configuration è pronta ✓\r\n",
        ),
        (ConfigKind::Gateway, 3, ConfigSource::CliImport, ""),
        (
            ConfigKind::Policies,
            1,
            ConfigSource::Seed,
            "# Policy seed\n",
        ),
        (
            ConfigKind::Policies,
            4,
            ConfigSource::CliImport,
            "# Policy import\n",
        ),
    ] {
        let document = ConfigDocument {
            id: ulid::Ulid::generate().to_string(),
            kind: kind.as_str().to_owned(),
            version,
            revision: config_revision(content),
            content: content.to_owned(),
            source: source.as_str().to_owned(),
            created_by: (source == ConfigSource::AdminApi)
                .then(|| "01JZ000000000000000000000A".to_string()),
            created_at: now,
        };
        documents.push(repo.insert(&mut tx, &document).await.unwrap());
    }
    let versions = repo
        .list(&mut *tx, ConfigKind::Gateway, 20, 0)
        .await
        .unwrap();
    assert_eq!(
        versions
            .iter()
            .map(|entry| entry.version)
            .collect::<Vec<_>>(),
        vec![3, 2, 1]
    );
    for (summary, expected) in versions.iter().zip(documents[..3].iter().rev()) {
        assert_eq!(summary.revision, expected.revision);
        assert_eq!(summary.source.as_str(), expected.source);
        assert_eq!(summary.created_by, expected.created_by);
        assert_eq!(summary.created_at, expected.created_at);
        assert_eq!(
            summary.size_bytes,
            i64::try_from(expected.content.len()).unwrap()
        );
        if expected.version == 2 {
            assert!(expected.content.len() > expected.content.chars().count());
        }
    }
    let page = repo
        .list(&mut *tx, ConfigKind::Gateway, 1, 1)
        .await
        .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].version, 2);
    assert!(
        repo.list(&mut *tx, ConfigKind::Gateway, 20, 3)
            .await
            .unwrap()
            .is_empty()
    );
    let policies = repo
        .list(&mut *tx, ConfigKind::Policies, 20, 0)
        .await
        .unwrap();
    assert_eq!(
        policies
            .iter()
            .map(|entry| entry.version)
            .collect::<Vec<_>>(),
        vec![4, 1]
    );
    for expected in documents {
        let kind = ConfigKind::parse(&expected.kind).unwrap();
        let document = repo
            .get(&mut *tx, kind, expected.version)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(document.id, expected.id);
        assert_eq!(document.content.as_bytes(), expected.content.as_bytes());
        assert_eq!(document.revision, expected.revision);
    }
    assert!(
        repo.get(&mut *tx, ConfigKind::Gateway, 4)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        repo.get(&mut *tx, ConfigKind::Policies, 3)
            .await
            .unwrap()
            .is_none()
    );
    tx.rollback().await.unwrap();
    pool.close().await;
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn sqlite_configuration_history_is_ordered_paginated_and_isolated_by_kind() {
    exercise_history(
        crate::backend::Pool::connect("sqlite::memory:")
            .await
            .unwrap(),
    )
    .await;
}

#[cfg(feature = "postgres")]
#[tokio::test]
#[ignore = "requires PostgreSQL"]
async fn postgres_configuration_history_is_ordered_paginated_and_isolated_by_kind() {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL must be set");
    exercise_history(crate::backend::Pool::connect(&url).await.unwrap()).await;
}
