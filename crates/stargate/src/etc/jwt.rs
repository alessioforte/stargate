use crate::act::parse_duration;
use jwt::{Algorithm, JwtConfig};
use once_cell::sync::Lazy;
use std::env;

pub static JWT_CONFIG: Lazy<JwtConfig> = Lazy::new(|| {
    let algorithm = env::var("JWT_ALGORITHM")
        .unwrap_or_else(|_| "HS256".to_string()) // default algorithm
        .parse::<Algorithm>()
        .expect("Invalid JWT algorithm");

    let private_key_path = env::var("JWT_PRIVATE_KEY_PATH")
        .unwrap_or_else(|_| ".stargate/jwks/private.pem".to_string());

    let public_key_path =
        env::var("JWT_PUBLIC_KEY_PATH").unwrap_or_else(|_| ".stargate/jwks/public.pem".to_string());

    let mut secret = env::var("JWT_SECRET").ok();

    log::info!("JWT Algorithm: {:?}", algorithm);

    if secret.is_none() && algorithm == Algorithm::HS256 {
        secret = Some(password::generator(512, false, true, true, false));
        log::warn!(
            "JWT_SECRET not set, generating a random secret key, {}",
            secret.as_ref().unwrap()
        );
    }

    let jwt_access_exp =
        env::var("JWT_ACCESS_EXPIRATION_MINUTES").unwrap_or_else(|_| "1h".to_string());
    let jwt_refresh_exp =
        env::var("JWT_REFRESH_EXPIRATION_DAYS").unwrap_or_else(|_| "1d".to_string());

    let access_exp =
        parse_duration(&jwt_access_exp).expect("Invalid JWT access expiration duration");
    let refresh_exp =
        parse_duration(&jwt_refresh_exp).expect("Invalid JWT refresh expiration duration");

    JwtConfig::new(
        algorithm,
        private_key_path,
        public_key_path,
        secret,
        access_exp,
        refresh_exp,
    )
});

pub fn init() {
    Lazy::force(&JWT_CONFIG);
}

pub fn jwt_config() -> &'static JwtConfig {
    &JWT_CONFIG
}
