#[cfg(feature = "postgres")]
use db::postgres::svc;

#[cfg(feature = "sqlite")]
use db::sqlite::svc;

use once_cell::sync::OnceCell;

static DB: OnceCell<svc::Service> = OnceCell::new();

pub fn service() -> svc::Service {
    let service = DB.get().expect("Service not initialized");
    service.clone()
}

#[cfg(feature = "postgres")]
pub async fn init() {
    let db_url = "postgres://root:root@localhost:5432/stargate";
    let service = svc::init(db_url).await;
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

#[cfg(feature = "sqlite")]
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
