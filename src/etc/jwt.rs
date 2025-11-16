use jwt::{Algorithm, JwtConfig};
use once_cell::sync::Lazy;
use std::env;
use tools::parse_duration;
use tracing::{info, warn};

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

    info!("JWT Algorithm: {:?}", algorithm);

    if secret.is_none() && algorithm == Algorithm::HS256 {
        secret = Some(pw::generator(512, false, true, true, false));
        warn!(
            "JWT_SECRET not set, generating a random secret key, {}",
            secret.as_ref().unwrap()
        );
    }

    let jwt_access_exp = env::var("JWT_ACCESS_EXP").unwrap_or_else(|_| "1h".to_string());
    let jwt_refresh_exp = env::var("JWT_REFRESH_EXP").unwrap_or_else(|_| "1d".to_string());

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
