use async_recursion::async_recursion;
use log;
use once_cell::sync::OnceCell;
use std::env;
use std::path::Path;
use surrealdb::engine::any::{self, Any};
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;
use tokio::time::{sleep, Duration};

pub static DB: OnceCell<Surreal<Any>> = OnceCell::new();
// pub static DB: LazyLock<Surreal<Any>> = LazyLock::new(Surreal::init);

pub async fn init() {
    sleep(Duration::from_secs(5)).await;
    let endpoint = match env::var("SURREALDB_ENDPOINT") {
        Ok(endpoint) => endpoint,
        Err(_) => {
            if !Path::new(".stargate/stargate.db").exists() {
                log::info!("Creating database");
                if let Err(e) = std::fs::create_dir_all(".stargate") {
                    log::error!("Error creating database: {}", e);
                }
            }
            "surrealkv://.stargate/stargate.db".to_owned()
        }
    };

    let username: String = env::var("SURREALDB_USERNAME").unwrap_or_else(|_| "".to_string());
    let password: String = env::var("SURREALDB_PASSWORD").unwrap_or_else(|_| "".to_string());
    let namespace: String =
        env::var("SURREALDB_NAMESPACE").unwrap_or_else(|_| "stargate".to_string());
    let database: String =
        env::var("SURREALDB_DATABASE").unwrap_or_else(|_| "stargate".to_string());

    let db = any::connect(endpoint).await;
    match db {
        Ok(db) => {
            log::info!("Connected to database");
            if !username.is_empty() && !password.is_empty() {
                let signed = db.signin(Root {
                    username: &username,
                    password: &password,
                });
                signed.await.unwrap();
            }
            match db.use_ns(namespace).use_db(database).await {
                Ok(_) => log::info!("Connected to namespace and database"),
                Err(e) => log::error!("Error connecting to namespace: {}", e),
            }

            DB.set(db).unwrap();
        }
        Err(e) => log::error!("Error connecting to database: {}", e),
    };
}

#[async_recursion]
pub async fn connection() -> surrealdb::Result<&'static Surreal<Any>> {
    match DB.get() {
        Some(db) => Ok(db),
        None => {
            // FIXME: Check if it works properly becouse if the OneCell is already set it will not initialize again
            init().await;
            connection().await
        }
    }
}
