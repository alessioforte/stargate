use crate::ent::Audit;
use anyhow::Result;
// use chrono::{DateTime, Utc};

pub const AUDIT: &str = "audits";

#[derive(Clone)]
pub struct AuditRepository {}

impl AuditRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn insert(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        log: &Audit,
    ) -> Result<Audit> {
        let row = sqlx::query_as::<_, Audit>(
            format!(
                "
            INSERT INTO {audits} (id, timestamp, resource, action, account_id, request_id, ip_address, user_agent, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING *
        ",
                audits = AUDIT
            )
            .as_str(),
        )
        .bind(&log.id)
        .bind(&log.timestamp)
        .bind(&log.resource)
        .bind(&log.action)
        .bind(&log.account_id)
        .bind(&log.request_id)
        .bind(&log.ip_address)
        .bind(&log.user_agent)
        .bind(&log.metadata)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    // pub async fn insert_batch(
    //     &self,
    //     #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    //     #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    //     logs: &[Audit],
    // ) -> Result<()> {
    //     if logs.is_empty() {
    //         return Ok(());
    //     }

    //     Ok(())
    // }

    // pub async fn search(
    //     &self,
    //     #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    //     #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    //     filter: &Filter,
    // ) -> Result<Vec<Audit>> {
    //     let rows = vec![];

    //     Ok(rows)
    // }
}

// #[derive(Debug, Clone)]
// pub struct Filter {
//     start_time: Option<DateTime<Utc>>,
//     end_time: Option<DateTime<Utc>>,
//     actor_id: Option<String>,
//     event_type: Option<String>,
//     resource_type: Option<String>,
//     limit: i64,
//     offset: i64,
// }

// impl Default for Filter {
//     fn default() -> Self {
//         Self {
//             start_time: None,
//             end_time: None,
//             actor_id: None,
//             event_type: None,
//             resource_type: None,
//             limit: 100,
//             offset: 0,
//         }
//     }
// }
