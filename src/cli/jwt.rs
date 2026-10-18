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
    /// restart and signs with it once JWT_KID names it.
    GenerateKey(GenerateKeyArgs),
}

#[derive(Debug, Args)]
struct GenerateKeyArgs {
    /// Key id of the new key: its token `kid` and directory name.
    #[arg(long)]
    kid: String,

    /// Algorithm of the new key (RS256, RS384, RS512, ES256, ES384).
    /// Defaults to JWT_ALGORITHM, then RS256.
    #[arg(long)]
    algorithm: Option<String>,

    /// JWT key directory (the server reads `.stargate/jwks`).
    #[arg(long, default_value = JWKS_DIR)]
    jwks_dir: PathBuf,
}

pub fn run(args: JwtArgs) -> io::Result<()> {
    match args.command {
        JwtCommand::GenerateKey(args) => generate_key(args),
    }
}

fn parse_algorithm(value: &str) -> io::Result<Algorithm> {
    value
        .trim()
        .parse::<Algorithm>()
        .ok()
        .filter(|algorithm| jwt_keyring::is_asymmetric(*algorithm))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("'{value}' is not RS256, RS384, RS512, ES256, or ES384"),
            )
        })
}

fn generate_key(args: GenerateKeyArgs) -> io::Result<()> {
    let current_kid = std::env::var("JWT_KID")
        .ok()
        .filter(|kid| !kid.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_KEY_ID.to_string());
    let current_algorithm = match std::env::var("JWT_ALGORITHM") {
        Ok(value) if !value.trim().is_empty() => {
            if matches!(value.trim(), "HS256" | "HS384" | "HS512") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "JWT key rotation applies to asymmetric algorithms; JWT_ALGORITHM is an HMAC algorithm",
                ));
            }
            parse_algorithm(&value)?
        }
        _ => Algorithm::RS256,
    };
    let algorithm = match args.algorithm.as_deref() {
        Some(value) => parse_algorithm(value)?,
        None => current_algorithm,
    };
    if args.kid == current_kid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "'{}' is the active key id (JWT_KID); choose a new one",
                args.kid
            ),
        ));
    }

    // Keep the pre-keyring key published and accepted after the switch.
    if let Some(dir) = jwt_keyring::adopt_legacy(&args.jwks_dir, &current_kid, current_algorithm)? {
        println!(
            "Moved the current key '{current_kid}' into {}",
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
        "1. Restart Stargate. It keeps signing with '{current_kid}' and publishes '{}' in /.well-known/jwks.json.",
        args.kid
    );
    println!(
        "2. Wait for JWKS caches to refresh (JWKS_CACHE_MAX_AGE_SECS, plus your relying parties' caches)."
    );
    println!(
        "3. Set JWT_KID={}{} and restart. Tokens signed by '{current_kid}' stay valid.",
        args.kid,
        if algorithm == current_algorithm {
            String::new()
        } else {
            format!(" and JWT_ALGORITHM={algorithm:?}")
        }
    );
    println!(
        "4. After the longest token lifetime (JWT_REFRESH_EXP), remove keys/{current_kid} and restart."
    );
    Ok(())
}
