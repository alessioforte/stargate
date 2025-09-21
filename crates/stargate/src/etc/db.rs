#[cfg(feature = "postgres")]
mod postgres {
    use db::svc;
    use once_cell::sync::OnceCell;

    static DB: OnceCell<svc::Service> = OnceCell::new();

    pub async fn init() {
        let password = std::env::var("POSTGRES_PASSWORD").unwrap_or_else(|_| "root".to_string());
        let user = std::env::var("POSTGRES_USER").unwrap_or_else(|_| "root".to_string());
        let host = std::env::var("POSTGRES_ENDPOINT").unwrap_or_else(|_| "localhost".to_string());
        let database =
            std::env::var("POSTGRES_DATABASE").unwrap_or_else(|_| "stargate".to_string());
        let db_url = format!("postgres://{}:{}@{}/{}", user, password, host, database);
        let service = svc::init(&db_url).await;
        match service {
            Ok(svc) => {
                log::info!("PostgreSQL service initialized successfully");
                if DB.set(svc).is_err() {
                    log::error!("Failed to set the PostgreSQL service instance");
                }
            }
            Err(e) => log::error!("Error initializing PostgreSQL service > {}", e),
        }
    }

    pub fn service() -> svc::Service {
        let service = DB.get().expect("Service not initialized");
        service.clone()
    }
}

#[cfg(feature = "sqlite")]
mod sqlite {
    use db::svc;
    use once_cell::sync::OnceCell;

    static DB: OnceCell<svc::Service> = OnceCell::new();

    pub async fn init() {
        if std::path::Path::new(".stargate/sqlite.db").exists() {
            log::info!("SQLite database file found, initializing service...");
        } else {
            log::info!("SQLite database file not found, creating directory and file...");
            std::fs::create_dir_all(".stargate").expect("Failed to create directory");
            std::fs::File::create(".stargate/sqlite.db")
                .expect("Failed to create SQLite database file");
            log::info!("SQLite database file created successfully");
        }

        let service = svc::init("sqlite://.stargate/sqlite.db").await;
        match service {
            Ok(svc) => {
                log::info!("Service initialized successfully");
                if DB.set(svc).is_err() {
                    log::error!("Failed to set the service instance");
                }
            }
            Err(e) => log::error!("Error initializing service: {}", e),
        }
    }

    pub fn service() -> svc::Service {
        let service = DB.get().expect("Service not initialized");
        service.clone()
    }
}

#[cfg(feature = "postgres")]
pub use postgres::init;
#[cfg(feature = "postgres")]
pub use postgres::service;

#[cfg(feature = "sqlite")]
pub use sqlite::init;
#[cfg(feature = "sqlite")]
pub use sqlite::service;
