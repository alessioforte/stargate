use super::{Service, pool_max_connections};
use anyhow::{Context, Result};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

pub async fn init_with_options(options: PgConnectOptions) -> Result<Service> {
    let pool = PgPoolOptions::new()
        .max_connections(pool_max_connections())
        .connect_with(options)
        .await?;
    Ok(Service::new(pool))
}

/// Create the target database through `postgres`, retaining credentials and TLS settings.
pub async fn ensure_database(options: &PgConnectOptions) -> Result<()> {
    let database = options
        .get_database()
        .filter(|database| !database.is_empty())
        .context("A target database name is required to create a PostgreSQL database")?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone().database("postgres"))
        .await
        .context("Cannot connect to the PostgreSQL maintenance database")?;

    let result = async {
        if database_exists(&pool, database).await? {
            return Ok(());
        }

        // Identifiers cannot be bound; escape embedded quotes before quoting the name.
        let statement = format!("CREATE DATABASE \"{}\"", database.replace('"', "\"\""));
        match sqlx::query(sqlx::AssertSqlSafe(statement))
            .execute(&pool)
            .await
        {
            Ok(_) => tracing::info!(database, "Created PostgreSQL database"),
            Err(error) => {
                if let Some(database_error) = error.as_database_error() {
                    let code = database_error.code();
                    let constraint = database_error.constraint();
                    if is_duplicate_database(code.as_deref(), constraint) {
                        let exists = database_exists(&pool, database).await?;
                        if is_creation_race(code.as_deref(), constraint, exists) {
                            tracing::debug!(
                                database,
                                "Another process created the PostgreSQL database"
                            );
                            return Ok(());
                        }
                    }
                }
                return Err(error).context("Failed to create PostgreSQL database");
            }
        }
        Ok(())
    }
    .await;
    pool.close().await;
    result
}

async fn database_exists(pool: &sqlx::PgPool, database: &str) -> Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
        .bind(database)
        .fetch_one(pool)
        .await
        .context("Cannot check whether the PostgreSQL database exists")
}

fn is_duplicate_database(code: Option<&str>, constraint: Option<&str>) -> bool {
    code == Some("42P04")
        || (code == Some("23505") && constraint == Some("pg_database_datname_index"))
}

fn is_creation_race(code: Option<&str>, constraint: Option<&str>, database_exists: bool) -> bool {
    database_exists && is_duplicate_database(code, constraint)
}

#[cfg(test)]
mod tests;
