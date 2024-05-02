use once_cell::sync::OnceCell;
use redis::Client;
use std::env;

#[allow(dead_code)]
pub static CACHE: OnceCell<Client> = OnceCell::new();

#[allow(dead_code)]
pub async fn connect() {
    let address: String = env::var("REDIS_URI").unwrap_or_else(|_| "".to_string());
    let result = Client::open(address);
    let client = match result {
        Ok(client) => {
            log::info!("Connected to redis");
            client
        }
        Err(e) => {
            log::error!("Error connecting to redis: {}", e);
            return;
        }
    };

    CACHE.set(client).unwrap();
}

#[allow(dead_code)]
pub async fn get_connection() -> redis::RedisResult<redis::Connection> {
    let client = CACHE.get().unwrap();
    client.get_connection()
}

// let mut cache = client.get_connection().unwrap();
// let a: Result<i16, redis::RedisError> = cache.set("value", 40);
