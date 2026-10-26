mod svc;

pub use svc::*;

#[cfg(feature = "postgres")]
mod postgres {
    use db::service::{self, Service};
    use once_cell::sync::OnceCell;
    use tracing::info;

    static DB: OnceCell<Service> = OnceCell::new();

    pub async fn init() -> anyhow::Result<()> {
        let password = std::env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "root".to_string());
        let user = std::env::var("POSTGRES_USERNAME").unwrap_or_else(|_| "root".to_string());
        let host = std::env::var("POSTGRES_ENDPOINT").unwrap_or_else(|_| "localhost".to_string());
        let database =
            std::env::var("POSTGRES_DATABASE").unwrap_or_else(|_| "stargate".to_string());

        service::ensure_database(&user, &password, &host, &database).await;

        let db_url = format!("postgres://{}:{}@{}/{}", user, password, host, database);
        let service = service::init(&db_url).await?;

        info!("PostgreSQL service initialized successfully");
        DB.set(service)
            .map_err(|_| anyhow::anyhow!("Failed to set the PostgreSQL service instance"))?;
        Ok(())
    }

    pub fn service() -> Service {
        DB.get()
            .expect("DB service not initialized — init() must be called first")
            .clone()
    }

    pub async fn ping() -> anyhow::Result<()> {
        DB.get()
            .ok_or_else(|| anyhow::anyhow!("DB not initialized"))?
            .ping()
            .await
    }
}

#[cfg(feature = "sqlite")]
mod sqlite {
    use db::service;
    use once_cell::sync::OnceCell;
    use tracing::info;

    static DB: OnceCell<service::Service> = OnceCell::new();

    pub async fn init() -> anyhow::Result<()> {
        if !std::path::Path::new(".stargate/sqlite.db").exists() {
            info!("SQLite database file not found, creating directory and file...");
            std::fs::create_dir_all(".stargate")?;
            std::fs::File::create(".stargate/sqlite.db")?;
            info!("SQLite database file created successfully");
        }

        let service = service::init("sqlite://.stargate/sqlite.db").await?;

        info!("SQLite service initialized successfully");
        DB.set(service)
            .map_err(|_| anyhow::anyhow!("Failed to set the SQLite service instance"))?;
        Ok(())
    }

    pub fn service() -> service::Service {
        DB.get()
            .expect("DB service not initialized — init() must be called first")
            .clone()
    }

    pub async fn ping() -> anyhow::Result<()> {
        DB.get()
            .ok_or_else(|| anyhow::anyhow!("DB not initialized"))?
            .ping()
            .await
    }
}

#[cfg(feature = "postgres")]
pub use postgres::init;
#[cfg(feature = "postgres")]
pub use postgres::ping;
#[cfg(feature = "postgres")]
pub use postgres::service;

#[cfg(feature = "sqlite")]
pub use sqlite::init;
#[cfg(feature = "sqlite")]
pub use sqlite::ping;
#[cfg(feature = "sqlite")]
pub use sqlite::service;
