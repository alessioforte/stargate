use log;
use once_cell::sync::OnceCell;
use std::env;
use surrealdb::engine::any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

pub static DB: OnceCell<Surreal<any::Any>> = OnceCell::new();

pub async fn connect() {
    let endpoint = env::var("SURREALDB_ENDPOINT").unwrap_or_else(|_| "memory".to_owned());
    let username: String = env::var("SURREALDB_USERNAME").unwrap_or_else(|_| "".to_string());
    let password: String = env::var("SURREALDB_PASSWORD").unwrap_or_else(|_| "".to_string());
    let namespace: String = env::var("SURREALDB_NAMESPACE").unwrap_or_else(|_| "".to_string());
    let database: String = env::var("SURREALDB_DATABASE").unwrap_or_else(|_| "".to_string());

    let db = any::connect(endpoint).await;
    let db = match db {
        Ok(db) => {
            log::info!("Connected to database");
            db
        }
        Err(e) => {
            log::error!("Error connecting to database: {}", e);
            return;
        }
    };

    if !username.is_empty() && !password.is_empty() {
        let signed = db.signin(Root {
            username: &username,
            password: &password,
        });
        signed.await.unwrap();
    }

    match db.use_ns(namespace).use_db(database).await {
        Ok(_) => {
            log::info!("Connected to namespace and database");
        }
        Err(e) => {
            log::error!("Error connecting to namespace: {}", e);
            return;
        }
    }

    DB.set(db).unwrap();
}
