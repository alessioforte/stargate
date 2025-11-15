use crate::ent::Audit;
use anyhow::Result;
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct AuditRepository {}

impl AuditRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn insert_batch(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        logs: &[Audit],
    ) -> Result<()> {
        if logs.is_empty() {
            return Ok(());
        }

        Ok(())
    }

    pub async fn search(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        filter: &Filter,
    ) -> Result<Vec<Audit>> {
        let rows = vec![];

        Ok(rows)
    }
}

#[derive(Debug, Clone)]
pub struct Filter {
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    actor_id: Option<String>,
    event_type: Option<String>,
    resource_type: Option<String>,
    limit: i64,
    offset: i64,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            start_time: None,
            end_time: None,
            actor_id: None,
            event_type: None,
            resource_type: None,
            limit: 100,
            offset: 0,
        }
    }
}
