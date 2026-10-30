//! Shared persistence algorithm for validated configuration seeds.

use ::db::ent::{ConfigDocument, ConfigKind, ConfigSaveOutcome, ConfigSource, TrustedAuditContext};

pub(crate) async fn active_or_seed<F, Fut>(
    kind: ConfigKind,
    seed: F,
) -> anyhow::Result<ConfigDocument>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<String>>,
{
    if let Some(document) = super::get_active_config_document(kind).await? {
        return Ok(document);
    }
    let content = seed().await?;
    let context = TrustedAuditContext::system_control_plane();
    match super::save_config_document(kind, content, None, ConfigSource::Seed, context).await? {
        ConfigSaveOutcome::Saved(document) | ConfigSaveOutcome::Unchanged(document) => Ok(document),
        ConfigSaveOutcome::Conflict { .. } => super::get_active_config_document(kind)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} configuration disappeared after concurrent seeding",
                    kind.as_str()
                )
            }),
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use crate::data::db::{service, with_test_database};
    use ::db::ent::OutboxEventFilter;

    #[tokio::test]
    async fn seeds_once_and_never_reads_a_seed_when_an_active_document_exists() {
        for kind in [ConfigKind::Gateway, ConfigKind::Policies] {
            with_test_database(async {
                let document = active_or_seed(kind, || async { Ok("exact seed\n".to_owned()) })
                    .await
                    .unwrap();
                assert_eq!(document.content, "exact seed\n");
                assert_eq!(document.version, 1);
                let active =
                    active_or_seed(kind, || async { anyhow::bail!("seed must not be read") })
                        .await
                        .unwrap();
                assert_eq!(active.id, document.id);
                let events = service()
                    .query_outbox_events(&OutboxEventFilter::default(), 10, 0)
                    .await
                    .unwrap();
                assert_eq!(events.len(), 1);
                let event: serde_json::Value = serde_json::from_str(&events[0].payload).unwrap();
                assert_eq!(event["actor"]["type"], "system");
                assert_eq!(event["scope"]["kind"], "control_plane");
                assert_eq!(event["metadata"]["source"], "seed");
            })
            .await;
        }
    }

    #[tokio::test]
    async fn a_failed_seed_can_retry_and_a_concurrent_seed_returns_the_winner() {
        for kind in [ConfigKind::Gateway, ConfigKind::Policies] {
            with_test_database(async {
                assert!(
                    active_or_seed(kind, || async { anyhow::bail!("invalid seed") })
                        .await
                        .is_err()
                );
                assert!(
                    super::super::get_active_config_document(kind)
                        .await
                        .unwrap()
                        .is_none()
                );
                assert_eq!(
                    service()
                        .count_outbox_events(&OutboxEventFilter::default())
                        .await
                        .unwrap(),
                    0
                );
                let document = active_or_seed(kind, || async {
                    // Commit another seed after the empty-database check.
                    active_or_seed(kind, || async { Ok("winner".to_owned()) }).await?;
                    Ok("loser".to_owned())
                })
                .await
                .unwrap();
                assert_eq!(document.content, "winner");
                assert_eq!(document.version, 1);
                assert_eq!(
                    service()
                        .count_outbox_events(&OutboxEventFilter::default())
                        .await
                        .unwrap(),
                    1
                );
            })
            .await;
        }
    }
}
