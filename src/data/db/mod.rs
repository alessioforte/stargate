#[cfg(feature = "postgres")]
mod settings;
mod svc;

pub use svc::*;

fn parse_migrate_on_start(value: Option<&str>) -> anyhow::Result<bool> {
    match value {
        None => Ok(true),
        Some(value) => value
            .parse::<bool>()
            .map_err(|_| anyhow::anyhow!("DB_MIGRATE_ON_START must be true or false")),
    }
}

fn migrate_on_start() -> anyhow::Result<bool> {
    match std::env::var("DB_MIGRATE_ON_START") {
        Ok(value) => parse_migrate_on_start(Some(&value)),
        Err(std::env::VarError::NotPresent) => parse_migrate_on_start(None),
        Err(error) => Err(error.into()),
    }
}

async fn prepare(service: &::db::service::Service, migrate: bool) -> anyhow::Result<()> {
    if migrate {
        service.migrate().await
    } else {
        service.verify_schema().await
    }
}

#[cfg(test)]
tokio::task_local! {
    static TEST_SERVICE: ::db::service::Service;
}

#[cfg(test)]
pub(crate) async fn with_test_service<F: std::future::Future>(
    service: ::db::service::Service,
    future: F,
) -> F::Output {
    TEST_SERVICE.scope(service, future).await
}

#[cfg(all(test, feature = "sqlite"))]
pub(crate) async fn with_test_database<F: std::future::Future>(future: F) -> F::Output {
    let service = ::db::service::init("sqlite::memory:")
        .await
        .expect("in-memory database");
    service.migrate().await.expect("migrations apply");
    with_test_service(service, future).await
}

#[cfg(feature = "postgres")]
mod postgres {
    use db::service::{self, Service};
    use once_cell::sync::OnceCell;
    use tracing::info;

    static DB: OnceCell<Service> = OnceCell::new();

    async fn connect() -> anyhow::Result<Service> {
        let settings = super::settings::PostgresSettings::from_env()?;
        let options = settings.connect_options();
        if settings.create_database {
            service::ensure_database(&options).await?;
        }
        service::init_with_options(options).await
    }

    pub async fn migrate() -> anyhow::Result<()> {
        connect().await?.migrate().await
    }

    pub async fn init() -> anyhow::Result<()> {
        let migrate = super::migrate_on_start()?;
        let service = connect().await?;
        super::prepare(&service, migrate).await?;

        info!("PostgreSQL service initialized successfully");
        DB.set(service)
            .map_err(|_| anyhow::anyhow!("Failed to set the PostgreSQL service instance"))?;
        Ok(())
    }

    pub fn service() -> Service {
        #[cfg(test)]
        if let Ok(service) = super::TEST_SERVICE.try_with(Clone::clone) {
            return service;
        }
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

    async fn connect() -> anyhow::Result<service::Service> {
        if !std::path::Path::new(".stargate/sqlite.db").exists() {
            info!("SQLite database file not found, creating directory and file...");
            std::fs::create_dir_all(".stargate")?;
            std::fs::File::create(".stargate/sqlite.db")?;
            info!("SQLite database file created successfully");
        }

        service::init("sqlite://.stargate/sqlite.db").await
    }

    pub async fn migrate() -> anyhow::Result<()> {
        connect().await?.migrate().await
    }

    pub async fn init() -> anyhow::Result<()> {
        let migrate = super::migrate_on_start()?;
        let service = connect().await?;
        super::prepare(&service, migrate).await?;

        info!("SQLite service initialized successfully");
        DB.set(service)
            .map_err(|_| anyhow::anyhow!("Failed to set the SQLite service instance"))?;
        Ok(())
    }

    pub fn service() -> service::Service {
        #[cfg(test)]
        if let Ok(service) = super::TEST_SERVICE.try_with(Clone::clone) {
            return service;
        }
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
pub use postgres::migrate;
#[cfg(feature = "postgres")]
pub use postgres::ping;
#[cfg(feature = "postgres")]
pub use postgres::service;

#[cfg(feature = "sqlite")]
pub use sqlite::init;
#[cfg(feature = "sqlite")]
pub use sqlite::migrate;
#[cfg(feature = "sqlite")]
pub use sqlite::ping;
#[cfg(feature = "sqlite")]
pub use sqlite::service;

#[cfg(test)]
mod tests {
    use super::parse_migrate_on_start;

    #[test]
    fn migrate_on_start_defaults_to_true_and_rejects_invalid_values() {
        assert!(parse_migrate_on_start(None).unwrap());
        assert!(parse_migrate_on_start(Some("true")).unwrap());
        assert!(!parse_migrate_on_start(Some("false")).unwrap());
        for invalid in ["", "yes", "TRUE", "invalid"] {
            assert!(parse_migrate_on_start(Some(invalid)).is_err());
        }
    }
}
