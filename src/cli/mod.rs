mod internal_context;

use crate::db;
use clap::{Args, Parser, Subcommand};
use internal_context::InternalContextArgs;
use std::io::{self, Read};

#[derive(Debug, Parser)]
#[command(name = "stargate")]
#[command(about = "Stargate server and administrative CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Admin(AdminArgs),
    InternalContext(InternalContextArgs),
}

impl Command {
    pub const fn is_standalone(&self) -> bool {
        matches!(self, Self::InternalContext(_))
    }
}

#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub command: AdminCommand,
}

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// Provision the first super admin and the Console OAuth client.
    Bootstrap(BootstrapArgs),
}

#[derive(Debug, Args)]
pub struct BootstrapArgs {
    /// Email for the first super admin. Required only when none exists.
    #[arg(long)]
    pub email: Option<String>,

    /// Password for the first super admin.
    #[arg(long, conflicts_with_all = ["password_stdin", "generate_password"])]
    pub password: Option<String>,

    /// Read the first super-admin password from standard input.
    #[arg(long, conflicts_with = "generate_password")]
    pub password_stdin: bool,

    /// Generate and print a password after successful provisioning.
    #[arg(long)]
    pub generate_password: bool,

    /// Display name for the first super admin.
    #[arg(long)]
    pub name: Option<String>,

    /// Nickname for the first super admin; defaults to the email.
    #[arg(long)]
    pub nickname: Option<String>,

    /// Console OAuth client ID; defaults to CONSOLE_OAUTH_CLIENT_ID.
    #[arg(long)]
    pub oauth_client_id: Option<String>,

    /// Exact Console OAuth callback URI.
    #[arg(long)]
    pub oauth_redirect_uri: Option<String>,
}

pub fn parse() -> Cli {
    <Cli as Parser>::parse()
}

fn read_password_from_stdin() -> io::Result<String> {
    let mut password = String::new();
    io::stdin().read_to_string(&mut password)?;
    let password = password.trim_end_matches(['\r', '\n']).to_string();

    if password.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "password read from stdin must not be empty",
        ));
    }

    Ok(password)
}

pub async fn run_cli_command(command: Command) -> io::Result<()> {
    match command {
        Command::Admin(admin) => match admin.command {
            AdminCommand::Bootstrap(args) => run_bootstrap(args).await,
        },
        Command::InternalContext(args) => internal_context::run(args),
    }
}

async fn run_bootstrap(args: BootstrapArgs) -> io::Result<()> {
    db::init()
        .await
        .map_err(|error| io::Error::other(format!("database initialization failed: {error}")))?;

    let super_admin_exists = crate::fun::super_admin_exists()
        .await
        .map_err(io::Error::other)?;
    let credentials_supplied = args.email.is_some()
        || args.password.is_some()
        || args.password_stdin
        || args.generate_password
        || args.name.is_some()
        || args.nickname.is_some();

    let (email, password, generated_password) = if super_admin_exists {
        (None, None, None)
    } else {
        let email = args.email.clone().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "admin bootstrap requires --email when no super admin exists",
            )
        })?;
        let generated_password = args.generate_password.then(|| {
            pw::Generator::new(40)
                .uppercase()
                .lowercase()
                .digits()
                .generate()
        });
        let password = match (
            args.password.clone(),
            args.password_stdin,
            generated_password.clone(),
        ) {
            (Some(password), false, None) => password,
            (None, true, None) => read_password_from_stdin()?,
            (None, false, Some(password)) => password,
            (None, false, None) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "admin bootstrap requires --password, --password-stdin, or --generate-password when no super admin exists",
                ));
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "use only one of --password, --password-stdin, or --generate-password",
                ));
            }
        };

        // Generated passwords are exempt: 40 chars of upper+lower+digits
        // is categorically stronger than the composition policy.
        if generated_password.is_none() {
            crate::act::password_policy::validate_global(
                &password,
                args.nickname.as_deref(),
                Some(&email),
            )
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        }
        (Some(email), Some(password), generated_password)
    };

    let client_id = crate::fun::resolve_console_oauth_client_id(args.oauth_client_id)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let redirect_uri = crate::fun::resolve_console_oauth_redirect_uri(args.oauth_redirect_uri)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let result = crate::fun::bootstrap_instance(
        email,
        password,
        args.name,
        args.nickname,
        client_id.clone(),
        redirect_uri.clone(),
    )
    .await
    .map_err(io::Error::other)?;

    if result.super_admin_created {
        println!("Created the first super admin.");
        if let Some(password) = generated_password.as_deref() {
            println!("Generated password: {password}");
        }
    } else {
        println!("A super admin already exists; left it unchanged.");
        if credentials_supplied {
            eprintln!("Ignored super-admin credential and profile arguments.");
        }
    }

    if result.oauth_client_created {
        println!("Created Console OAuth client '{client_id}' with redirect URI '{redirect_uri}'.");
    } else {
        println!(
            "Console OAuth client '{client_id}' already exists and matches; left it unchanged."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_internal_context_key_generation_command() {
        let cli = Cli::try_parse_from([
            "stargate",
            "internal-context",
            "generate-key",
            "--output-dir",
            ".stargate/internal-context",
            "--kid",
            "stargate-internal-current",
            "--bits",
            "3072",
        ])
        .unwrap();

        assert!(matches!(cli.command, Some(Command::InternalContext(_))));
    }

    #[test]
    fn parses_combined_admin_bootstrap_configuration() {
        let cli = Cli::try_parse_from([
            "stargate",
            "admin",
            "bootstrap",
            "--email",
            "admin@example.com",
            "--generate-password",
            "--oauth-client-id",
            "stargate_console",
            "--oauth-redirect-uri",
            "https://identity.example.com/stargate/auth/callback",
        ])
        .unwrap();

        let Some(Command::Admin(AdminArgs {
            command: AdminCommand::Bootstrap(args),
        })) = cli.command
        else {
            panic!("expected admin bootstrap command");
        };
        assert_eq!(args.email.as_deref(), Some("admin@example.com"));
        assert!(args.generate_password);
        assert_eq!(args.oauth_client_id.as_deref(), Some("stargate_console"));
        assert_eq!(
            args.oauth_redirect_uri.as_deref(),
            Some("https://identity.example.com/stargate/auth/callback")
        );
    }

    #[test]
    fn parses_bootstrap_repair_without_admin_credentials() {
        let cli = Cli::try_parse_from([
            "stargate",
            "admin",
            "bootstrap",
            "--oauth-redirect-uri",
            "https://identity.example.com/stargate/auth/callback",
        ])
        .unwrap();

        let Some(Command::Admin(AdminArgs {
            command: AdminCommand::Bootstrap(args),
        })) = cli.command
        else {
            panic!("expected admin bootstrap command");
        };
        assert!(args.email.is_none());
        assert!(args.password.is_none());
        assert!(!args.password_stdin);
        assert!(!args.generate_password);
    }
}
