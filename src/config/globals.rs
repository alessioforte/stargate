use crate::config::config;
use crate::config::trie::TriePath;
use std::env;
use std::sync::{Arc, Mutex};

pub struct Globals {
    pub config: Arc<Mutex<TriePath>>,
    pub jwt_secret: String,
    pub access_token_expiration: i64,
    pub refresh_token_expiration: i64,
}

impl Globals {
    pub fn init() -> Self {
        let cfg = config::Config::from_file();
        let mut trie = TriePath::new();
        for service in &cfg.services {
            trie.insert(&service.path, service.clone());
        }

        let config = Arc::new(Mutex::new(trie));

        let jwt_secret = env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());
        let access_token_expiration = env::var("ACCESS_TOKEN_EXPIRATION")
            .unwrap_or_else(|_| "60".to_string())
            .parse::<i64>()
            .unwrap();
        let refresh_token_expiration = env::var("REFRESH_TOKEN_EXPIRATION")
            .unwrap_or_else(|_| "1440".to_string())
            .parse::<i64>()
            .unwrap();

        Self {
            config,
            jwt_secret,
            access_token_expiration,
            refresh_token_expiration,
        }
    }
}
