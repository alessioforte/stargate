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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    Seed,
    AdminApi,
    CliImport,
}

impl ConfigSource {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "seed" => Some(Self::Seed),
            "admin_api" => Some(Self::AdminApi),
            "cli_import" => Some(Self::CliImport),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seed => "seed",
            Self::AdminApi => "admin_api",
            Self::CliImport => "cli_import",
        }
    }
}

impl TryFrom<String> for ConfigSource {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or_else(|| anyhow::anyhow!("unknown configuration source: {value}"))
    }
}

/// Version metadata without fetching the stored configuration content.
#[derive(sqlx::FromRow, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigVersionSummary {
    pub version: i64,
    pub revision: String,
    #[sqlx(try_from = "String")]
    pub source: ConfigSource,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub size_bytes: i64,
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
    use super::{ConfigKind, ConfigSource, config_revision};

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
    fn configuration_sources_decode_to_typed_values_and_reject_unknown_sources() {
        for source in [
            ConfigSource::Seed,
            ConfigSource::AdminApi,
            ConfigSource::CliImport,
        ] {
            assert_eq!(
                ConfigSource::try_from(source.as_str().to_owned()).unwrap(),
                source
            );
            assert_eq!(serde_json::to_value(source).unwrap(), source.as_str());
        }
        for unknown in ["", "other", "CLI_IMPORT"] {
            assert!(ConfigSource::try_from(unknown.to_owned()).is_err());
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
