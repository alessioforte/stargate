use crate::data::db;
use crate::iam::authz::rules::validate_policy_document;
use ::db::ent::{
    ConfigDocument, ConfigKind, ConfigSaveOutcome, ConfigSource, ConfigVersionSummary,
    TrustedAuditContext,
};
use clap::{Args, Subcommand, ValueEnum};
use std::{
    io::{self, Write},
    path::PathBuf,
};

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// List stored document versions, newest first.
    Versions(VersionsArgs),
    /// Export the active document or a stored version without changing its contents.
    Export(ExportArgs),
    /// Validate and import a document into the database.
    Import(ImportArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum DocumentKind {
    Gateway,
    Policies,
}

impl From<DocumentKind> for ConfigKind {
    fn from(kind: DocumentKind) -> Self {
        match kind {
            DocumentKind::Gateway => Self::Gateway,
            DocumentKind::Policies => Self::Policies,
        }
    }
}

#[derive(Debug, Args)]
struct ExportArgs {
    #[arg(long, value_enum)]
    kind: DocumentKind,
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, value_parser = clap::value_parser!(i64).range(1..))]
    version: Option<i64>,
}

#[derive(Debug, Args)]
struct VersionsArgs {
    #[arg(long, value_enum)]
    kind: DocumentKind,
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(i64).range(1..))]
    limit: i64,
}

#[derive(Debug, Args)]
struct ImportArgs {
    #[arg(long, value_enum)]
    kind: DocumentKind,
    #[arg(long)]
    file: PathBuf,
}

pub async fn run(args: ConfigArgs) -> io::Result<()> {
    crate::srv::profile::validate_runtime_profile()?;
    db::init().await.map_err(io::Error::other)?;
    match args.command {
        ConfigCommand::Versions(args) => {
            let versions = db::list_config_versions(args.kind.into(), args.limit, 0)
                .await
                .map_err(io::Error::other)?;
            write_versions(&versions, &mut io::stdout().lock())
        }
        ConfigCommand::Export(args) => export_document(args).await,
        ConfigCommand::Import(args) => {
            let content = std::fs::read_to_string(args.file)?;
            match import_document(args.kind.into(), content).await? {
                ConfigSaveOutcome::Saved(document) => {
                    println!(
                        "imported version {} ({})",
                        document.version, document.revision
                    );
                    Ok(())
                }
                ConfigSaveOutcome::Unchanged(_) => {
                    println!("unchanged");
                    Ok(())
                }
                ConfigSaveOutcome::Conflict { .. } => Err(import_conflict()),
            }
        }
    }
}

async fn export_document(args: ExportArgs) -> io::Result<()> {
    let kind = args.kind.into();
    let document = match args.version {
        Some(version) => db::get_config_document(kind, version)
            .await
            .map_err(io::Error::other)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "no {} document version {version} in the database",
                        kind.as_str()
                    ),
                )
            })?,
        None => active_document(kind).await?,
    };
    match args.output {
        Some(path) => std::fs::write(path, document.content),
        None => io::stdout().lock().write_all(document.content.as_bytes()),
    }
}

fn write_versions(versions: &[ConfigVersionSummary], output: &mut impl Write) -> io::Result<()> {
    writeln!(
        output,
        "{:<10} {:>7} {:<71} {:<10} {:<26} {:<35} {:>10}",
        "STATUS", "VERSION", "REVISION", "SOURCE", "AUTHOR", "CREATED_AT", "SIZE_BYTES"
    )?;
    for (index, version) in versions.iter().enumerate() {
        writeln!(
            output,
            "{:<10} {:>7} {:<71} {:<10} {:<26} {:<35} {:>10}",
            if index == 0 { "active" } else { "historical" },
            version.version,
            version.revision,
            version.source.as_str(),
            version.created_by.as_deref().unwrap_or("-"),
            version.created_at.to_rfc3339(),
            version.size_bytes
        )?;
    }
    Ok(())
}

async fn active_document(kind: ConfigKind) -> io::Result<ConfigDocument> {
    db::get_active_config_document(kind)
        .await
        .map_err(io::Error::other)?
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no active {} document in the database", kind.as_str()),
            )
        })
}

fn validate_document(kind: ConfigKind, content: &str) -> io::Result<()> {
    match kind {
        ConfigKind::Gateway => {
            crate::gate::validate_gateway_document(content).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid gateway configuration document: {error}"),
                )
            })?;
        }
        ConfigKind::Policies => {
            validate_policy_document(content).map_err(|diagnostics| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid access-control policies document: {diagnostics}"),
                )
            })?;
        }
    }
    Ok(())
}

async fn import_document(kind: ConfigKind, content: String) -> io::Result<ConfigSaveOutcome> {
    validate_document(kind, &content)?;
    for _ in 0..2 {
        let expected = db::get_active_config_document(kind)
            .await
            .map_err(io::Error::other)?
            .map(|document| document.revision);
        let context = TrustedAuditContext::system_control_plane();
        match db::save_config_document(
            kind,
            content.clone(),
            expected,
            ConfigSource::CliImport,
            context,
        )
        .await
        .map_err(io::Error::other)?
        {
            ConfigSaveOutcome::Conflict { .. } => continue,
            outcome => return Ok(outcome),
        }
    }
    Err(import_conflict())
}

fn import_conflict() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "configuration changed during both import attempts; retry the command",
    )
}

#[cfg(test)]
mod tests;
