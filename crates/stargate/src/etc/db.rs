use db::sqlite::svc;
use once_cell::sync::OnceCell;

static SQLITE: OnceCell<svc::Service> = OnceCell::new();

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
            if SQLITE.set(svc).is_err() {
                log::error!("Failed to set the service instance");
            }
        }
        Err(e) => log::error!("Error initializing service: {}", e),
    }
}

pub fn service() -> svc::Service {
    let service = SQLITE.get().expect("Service not initialized");
    service.clone()
}
