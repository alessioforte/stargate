use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKind {
    Gateway,
    Policies,
}

impl ConfigKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "gateway" => Some(Self::Gateway),
            "policies" => Some(Self::Policies),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gateway => "gateway",
            Self::Policies => "policies",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRevision {
    pub kind: ConfigKind,
    pub revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Seed,
    AdminApi,
    CliImport,
}

impl ConfigSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seed => "seed",
            Self::AdminApi => "admin_api",
            Self::CliImport => "cli_import",
        }
    }
}

#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDocument {
    pub id: String,
    pub kind: String,
    pub version: i64,
    pub revision: String,
    pub content: String,
    pub source: String,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug)]
pub enum ConfigSaveOutcome {
    Saved(ConfigDocument),
    Unchanged(ConfigDocument),
    Conflict { current: Option<ConfigDocument> },
}

pub fn config_revision(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests {
    use super::{ConfigKind, config_revision};

    #[test]
    fn configuration_kind_names_round_trip_and_reject_unknown_names() {
        for kind in [ConfigKind::Gateway, ConfigKind::Policies] {
            assert_eq!(ConfigKind::parse(kind.as_str()), Some(kind));
        }
        for value in ["", "unknown", "Gateway"] {
            assert_eq!(ConfigKind::parse(value), None);
        }
    }

    #[test]
    fn config_revision_matches_known_sha256_values() {
        assert_eq!(
            config_revision(""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            config_revision("abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
