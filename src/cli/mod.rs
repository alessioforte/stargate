use crate::db;
use clap::{Args, Parser, Subcommand};
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
}

#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub command: AdminCommand,
}

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    Bootstrap(BootstrapArgs),
}

#[derive(Debug, Args)]
pub struct BootstrapArgs {
    #[arg(long)]
    pub email: String,

    #[arg(long, conflicts_with_all = ["password_stdin", "generate_password"])]
    pub password: Option<String>,

    #[arg(long, conflicts_with = "generate_password")]
    pub password_stdin: bool,

    #[arg(long)]
    pub generate_password: bool,

    #[arg(long)]
    pub name: Option<String>,

    #[arg(long)]
    pub nickname: Option<String>,
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
            AdminCommand::Bootstrap(args) => {
                let generated_password = if args.generate_password {
                    Some(
                        pw::Generator::new(40)
                            .uppercase()
                            .lowercase()
                            .digits()
                            .generate(),
                    )
                } else {
                    None
                };

                let password = match (
                    args.password,
                    args.password_stdin,
                    generated_password.clone(),
                ) {
                    (Some(password), false, None) => password,
                    (None, true, None) => read_password_from_stdin()?,
                    (None, false, Some(password)) => password,
                    (Some(_), true, _) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "use either --password or --password-stdin, not both",
                        ));
                    }
                    (Some(_), _, Some(_)) | (None, true, Some(_)) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "use only one of --password, --password-stdin, or --generate-password",
                        ));
                    }
                    (None, false, None) => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "admin bootstrap requires --password, --password-stdin, or --generate-password",
                        ));
                    }
                };

                // Generated passwords are exempt: 40 chars of upper+lower+digits
                // is categorically stronger than the composition policy.
                if generated_password.is_none() {
                    crate::act::password_policy::validate_global(
                        &password,
                        args.nickname.as_deref(),
                        Some(&args.email),
                    )
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
                }

                if let Err(e) = db::init().await {
                    return Err(io::Error::other(format!(
                        "database initialization failed: {e}"
                    )));
                }

                let result = crate::fun::bootstrap_super_admin(
                    args.email,
                    password,
                    args.name,
                    args.nickname,
                )
                .await;

                match result {
                    Ok(()) => {
                        if let Some(password) = generated_password.as_deref() {
                            println!("Generated password: {}", password);
                        }
                        Ok(())
                    }
                    Err(e) => Err(io::Error::other(e)),
                }
            }
        },
    }
}
