use crate::data::db;
use ::db::ent::{ConfigDocument, ConfigKind, ConfigSaveOutcome, ConfigSource, TrustedAuditContext};

fn get_policies_path() -> String {
    format!("{}/policies", crate::conf::config_dir())
}

pub(crate) async fn load_policies_document() -> anyhow::Result<ConfigDocument> {
    load_policies_document_from_path(&get_policies_path()).await
}

async fn load_policies_document_from_path(path: &str) -> anyhow::Result<ConfigDocument> {
    db::active_or_seed(ConfigKind::Policies, || async {
        let content = match tokio::fs::read_to_string(path).await {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        super::rules::validate_policy_document(&content).map_err(anyhow::Error::msg)?;
        Ok(content)
    })
    .await
}

pub(super) async fn read_policies_document() -> anyhow::Result<ConfigDocument> {
    db::get_active_config_document(ConfigKind::Policies)
        .await?
        .ok_or_else(|| anyhow::anyhow!("access-control policies not found in the database"))
}

pub(super) async fn save_policies_document(
    content: String,
    revision: String,
    context: TrustedAuditContext,
) -> anyhow::Result<ConfigSaveOutcome> {
    db::save_config_document(
        ConfigKind::Policies,
        content,
        Some(revision),
        ConfigSource::AdminApi,
        context,
    )
    .await
}

#[cfg(all(test, feature = "sqlite"))]
mod tests;
