use crate::etc::auth::jwt::DEFAULT_KEY_ID;
use crate::etc::auth::jwt_keyring::{self, JWKS_DIR};
use clap::{Args, Subcommand};
use jwt::Algorithm;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct JwtArgs {
    #[command(subcommand)]
    command: JwtCommand,
}

#[derive(Debug, Subcommand)]
enum JwtCommand {
    /// Create the next JWT signing key. Stargate publishes it after a
    /// restart and signs with it once JWT_KID (or, with --id-token,
    /// JWT_ID_TOKEN_KID) names it.
    GenerateKey(GenerateKeyArgs),
}

#[derive(Debug, Args)]
struct GenerateKeyArgs {
    /// Key id of the new key: its token `kid` and directory name.
    #[arg(long)]
    kid: String,

    /// Algorithm of the new key (RS256, RS384, RS512, ES256, ES384).
    /// Defaults to JWT_ALGORITHM, then RS256; always RS256 with --id-token.
    #[arg(long)]
    algorithm: Option<String>,

    /// The key will sign OIDC ID tokens (JWT_ID_TOKEN_KID): an RS256 key,
    /// for when JWT_KID is an EC key or an HMAC secret.
    #[arg(long)]
    id_token: bool,

    /// JWT key directory (the server reads `.stargate/jwks`).
    #[arg(long, default_value = JWKS_DIR)]
    jwks_dir: PathBuf,
}

pub fn run(args: JwtArgs) -> io::Result<()> {
    match args.command {
        JwtCommand::GenerateKey(args) => generate_key(args),
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn parse_algorithm(value: &str) -> io::Result<Algorithm> {
    value
        .trim()
        .parse::<Algorithm>()
        .ok()
        .filter(|algorithm| jwt_keyring::is_asymmetric(*algorithm))
        .ok_or_else(|| {
            invalid(format!(
                "'{value}' is not RS256, RS384, RS512, ES256, or ES384"
            ))
        })
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn generate_key(args: GenerateKeyArgs) -> io::Result<()> {
    let active_kid = env_value("JWT_KID").unwrap_or_else(|| DEFAULT_KEY_ID.to_string());
    // None for HMAC: the active "key" is JWT_SECRET, outside the keyring.
    let active_algorithm = match env_value("JWT_ALGORITHM").as_deref() {
        Some("HS256" | "HS384" | "HS512") => None,
        Some(value) => Some(parse_algorithm(value)?),
        None => Some(Algorithm::RS256),
    };
    let current_id_kid = env_value("JWT_ID_TOKEN_KID");

    let algorithm = if args.id_token {
        match args.algorithm.as_deref().map(parse_algorithm).transpose()? {
            Some(algorithm) if algorithm != jwt_keyring::ID_TOKEN_ALGORITHM => {
                return Err(invalid(format!(
                    "ID token keys must be {:?}",
                    jwt_keyring::ID_TOKEN_ALGORITHM
                )));
            }
            _ => jwt_keyring::ID_TOKEN_ALGORITHM,
        }
    } else {
        let Some(active_algorithm) = active_algorithm else {
            return Err(invalid(
                "JWT_ALGORITHM is an HMAC algorithm: rotate JWT_SECRET instead, or use --id-token for the ID token key",
            ));
        };
        match args.algorithm.as_deref() {
            Some(value) => parse_algorithm(value)?,
            None => active_algorithm,
        }
    };
    if args.kid == active_kid || Some(&args.kid) == current_id_kid.as_ref() {
        return Err(invalid(format!(
            "'{}' is already in use (JWT_KID / JWT_ID_TOKEN_KID); choose a new key id",
            args.kid
        )));
    }

    // Keep the pre-keyring key published and accepted after the switch.
    if let Some(active_algorithm) = active_algorithm
        && let Some(dir) = jwt_keyring::adopt_legacy(&args.jwks_dir, &active_kid, active_algorithm)?
    {
        println!(
            "Moved the current key '{active_kid}' into {}",
            dir.display()
        );
    }
    let dir = jwt_keyring::generate(&args.jwks_dir, &args.kid, algorithm)?;
    println!(
        "Generated JWT signing key '{}' ({algorithm:?}) in {}",
        args.kid,
        dir.display()
    );
    println!();
    println!("Next steps (on every node, sharing this directory):");
    println!(
        "1. Restart Stargate. It publishes '{}' in /.well-known/jwks.json without signing with it yet.",
        args.kid
    );
    println!(
        "2. Wait for JWKS caches to refresh (JWKS_CACHE_MAX_AGE_SECS, plus your relying parties' caches)."
    );

    if args.id_token {
        println!(
            "3. Set JWT_ID_TOKEN_KID={} and restart. ID tokens are signed with it; other tokens keep using '{active_kid}'.",
            args.kid
        );
        if let Some(previous) = current_id_kid {
            println!(
                "4. After the ID token lifetime (JWT_ACCESS_EXP), remove keys/{previous} and restart."
            );
        }
    } else {
        let algorithm_change = if Some(algorithm) == active_algorithm {
            String::new()
        } else {
            format!(" and JWT_ALGORITHM={algorithm:?}")
        };
        println!(
            "3. Set JWT_KID={}{algorithm_change} and restart. Tokens signed by '{active_kid}' stay valid.",
            args.kid
        );
        if current_id_kid.is_none() && algorithm != jwt_keyring::ID_TOKEN_ALGORITHM {
            println!(
                "   ID tokens would then be {algorithm:?}; OpenID Connect requires RS256, so also set JWT_ID_TOKEN_KID (create it with --id-token)."
            );
        }
        println!(
            "4. After the longest token lifetime (JWT_REFRESH_EXP), remove keys/{active_kid} and restart."
        );
    }
    Ok(())
}
