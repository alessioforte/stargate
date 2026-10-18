use super::jwt_keyring::{self, JWKS_DIR};
use jwt::{Algorithm, JwtConfig, KeySource};
use once_cell::sync::Lazy;
use std::{env, fs, path::Path};
use tools::parse_duration;
use tracing::info;

const DEFAULT_ACCESS_EXP: &str = "1h";
const DEFAULT_REFRESH_EXP: &str = "1d";
pub(crate) const DEFAULT_KEY_ID: &str = "stargate-current";

pub static JWT_CONFIG: Lazy<JwtConfig> = Lazy::new(|| {
    // Unset means "whatever the active key is"; RS256 for a first start.
    let configured = env::var("JWT_ALGORITHM")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            value
                .trim()
                .parse::<Algorithm>()
                .expect("Invalid JWT algorithm")
        });

    let jwt_access_exp =
        env::var("JWT_ACCESS_EXP").unwrap_or_else(|_| DEFAULT_ACCESS_EXP.to_owned());
    let jwt_refresh_exp =
        env::var("JWT_REFRESH_EXP").unwrap_or_else(|_| DEFAULT_REFRESH_EXP.to_owned());
    let access_exp =
        parse_duration(&jwt_access_exp).expect("Invalid JWT access expiration duration");
    let refresh_exp =
        parse_duration(&jwt_refresh_exp).expect("Invalid JWT refresh expiration duration");

    let key_id = env::var("JWT_KID")
        .ok()
        .filter(|kid| !kid.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_KEY_ID.to_owned());

    match configured {
        Some(algorithm @ (Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512)) => {
            info!("JWT Algorithm: {:?}", algorithm);
            let secret = env::var("JWT_SECRET").unwrap_or_else(|_| load_or_generate_secret());
            JwtConfig::new_with_key_id(
                algorithm,
                KeySource::Secret(secret),
                access_exp,
                refresh_exp,
                Some(key_id),
            )
        }
        Some(algorithm) if !jwt_keyring::is_asymmetric(algorithm) => {
            panic!("Unsupported JWT algorithm: {algorithm:?}")
        }
        _ => {
            let keyring = jwt_keyring::load(Path::new(JWKS_DIR), &key_id, configured)
                .unwrap_or_else(|error| panic!("Invalid JWT signing keys: {error}"));
            info!(
                "JWT signing key: {} ({:?})",
                key_id,
                keyring.active.algorithm()
            );
            if keyring.active.algorithm() != Algorithm::RS256 {
                tracing::warn!(
                    "ID tokens are signed with {:?}; OpenID Connect Discovery requires RS256 support, so certified relying parties may reject this provider",
                    keyring.active.algorithm()
                );
            }
            for key in &keyring.additional {
                info!(
                    "JWT verification key: {} ({:?})",
                    key.key_id().unwrap_or_default(),
                    key.algorithm()
                );
            }
            JwtConfig::from_keyring(keyring.active, keyring.additional, access_exp, refresh_exp)
                .unwrap_or_else(|error| panic!("Invalid JWT signing keys: {error}"))
        }
    }
});

fn load_or_generate_secret() -> String {
    let secret_path = format!("{JWKS_DIR}/secret.key");

    fs::read_to_string(&secret_path).unwrap_or_else(|_| {
        fs::create_dir_all(JWKS_DIR).expect("Unable to create JWKS directory");
        let secret = pw::Generator::new(512).lowercase().digits().generate();
        fs::write(secret_path, &secret).expect("Unable to write secret key");
        secret
    })
}

pub fn init() {
    Lazy::force(&JWT_CONFIG);
}

pub fn jwt_config() -> &'static JwtConfig {
    &JWT_CONFIG
}
