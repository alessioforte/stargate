use jwt::{Algorithm, JwtConfig, KeySource};
use once_cell::sync::Lazy;
use std::{env, path::Path};
use tools::parse_duration;
use tracing::info;

pub static JWT_CONFIG: Lazy<JwtConfig> = Lazy::new(|| {
    let algorithm = env::var("JWT_ALGORITHM")
        .unwrap_or_else(|_| "RS256".to_string()) // default algorithm
        .parse::<Algorithm>()
        .expect("Invalid JWT algorithm");

    info!("JWT Algorithm: {:?}", algorithm);

    let key_source = match algorithm {
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
            let secret = env::var("JWT_SECRET").unwrap_or_else(|_| {
                let jwks_path = ".stargate/jwks";
                let secret_path = format!("{}/secret.key", jwks_path);
                match std::fs::read_to_string(&secret_path) {
                    Ok(s) => s,
                    Err(_) => {
                        std::fs::create_dir_all(jwks_path)
                            .expect("Unable to create JWKS directory");
                        let generated = pw::generator(512, false, true, true, false);
                        std::fs::write(&secret_path, &generated)
                            .expect("Unable to write secret key");
                        generated
                    }
                }
            });
            KeySource::Secret(secret)
        }
        Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
            let jwks_path = ".stargate/jwks";
            ensure_key_pair(jwks_path, || jwt::generate_rsa_keys(2048));
            KeySource::Rsa {
                private_key_path: format!("{}/private.pem", jwks_path),
                public_key_path: format!("{}/public.pem", jwks_path),
            }
        }
        Algorithm::ES256 => {
            let jwks_path = ".stargate/jwks/es256";
            ensure_key_pair(jwks_path, jwt::generate_p256_keys);
            KeySource::Ec {
                private_key_path: format!("{}/private.pem", jwks_path),
                public_key_path: format!("{}/public.pem", jwks_path),
            }
        }
        Algorithm::ES384 => {
            let jwks_path = ".stargate/jwks/es384";
            ensure_key_pair(jwks_path, jwt::generate_p384_keys);
            KeySource::Ec {
                private_key_path: format!("{}/private.pem", jwks_path),
                public_key_path: format!("{}/public.pem", jwks_path),
            }
        }
        _ => panic!("Unsupported algorithm: {:?}", algorithm),
    };

    let jwt_access_exp = env::var("JWT_ACCESS_EXP").unwrap_or_else(|_| "1h".to_string());
    let jwt_refresh_exp = env::var("JWT_REFRESH_EXP").unwrap_or_else(|_| "1d".to_string());

    let access_exp =
        parse_duration(&jwt_access_exp).expect("Invalid JWT access expiration duration");
    let refresh_exp =
        parse_duration(&jwt_refresh_exp).expect("Invalid JWT refresh expiration duration");

    let key_id = env::var("JWT_KID")
        .ok()
        .filter(|kid| !kid.trim().is_empty())
        .unwrap_or_else(|| "stargate-current".to_string());

    JwtConfig::new_with_key_id(algorithm, key_source, access_exp, refresh_exp, Some(key_id))
});

fn ensure_key_pair<F>(jwks_path: &str, generate: F)
where
    F: FnOnce() -> Result<(String, String), Box<dyn std::error::Error>>,
{
    let private_key_path = format!("{}/private.pem", jwks_path);
    let public_key_path = format!("{}/public.pem", jwks_path);

    if Path::new(&private_key_path).exists() && Path::new(&public_key_path).exists() {
        return;
    }

    std::fs::create_dir_all(jwks_path).expect("Unable to create JWKS directory");
    let (private_key, public_key) = generate().expect("Failed to generate JWT signing keys");
    std::fs::write(private_key_path, private_key).expect("Unable to write private key");
    std::fs::write(public_key_path, public_key).expect("Unable to write public key");
}

pub fn init() {
    Lazy::force(&JWT_CONFIG);
}

pub fn jwt_config() -> &'static JwtConfig {
    &JWT_CONFIG
}
