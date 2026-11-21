use crate::conf::env::EnvError;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const DATA_PATH_ENV: &str = "DATA_PATH";
const DEFAULT_DATA_DIR: &str = ".stargate";
const SQLITE_FILE: &str = "sqlite.db";
const MEMORY_SNAPSHOT_FILE: &str = "memory.bin";

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Reads and validates `DATA_PATH` once. It names the writable edge data
/// directory for `sqlite.db` and `memory.bin`; `CONFIG_PATH` remains the
/// read-only seed directory.
pub fn init() -> Result<(), EnvError> {
    let directory = match std::env::var(DATA_PATH_ENV) {
        Ok(value) => parse_dir(&value)?,
        Err(std::env::VarError::NotPresent) => PathBuf::from(DEFAULT_DATA_DIR),
        Err(source) => {
            return Err(EnvError::Read {
                name: DATA_PATH_ENV.to_string(),
                source,
            });
        }
    };
    let _ = DATA_DIR.set(directory);
    Ok(())
}

fn parse_dir(value: &str) -> Result<PathBuf, EnvError> {
    if value.trim().is_empty() {
        return Err(EnvError::Parse {
            name: DATA_PATH_ENV.to_string(),
            value: value.to_string(),
            message: "must name a directory".to_string(),
        });
    }
    Ok(PathBuf::from(value))
}

/// The writable edge data directory (`DATA_PATH`, default `.stargate`).
pub fn dir() -> &'static Path {
    DATA_DIR.get().unwrap_or_else(|| {
        init().expect("DATA_PATH is validated at startup");
        DATA_DIR.get().expect("data directory initialized")
    })
}

/// Path of the edge SQLite database.
pub fn sqlite_file() -> PathBuf {
    dir().join(SQLITE_FILE)
}

/// Path of the in-memory store snapshot.
pub fn memory_snapshot() -> PathBuf {
    dir().join(MEMORY_SNAPSHOT_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_directory_holds_both_edge_files() {
        assert_eq!(parse_dir(DEFAULT_DATA_DIR).unwrap(), Path::new(".stargate"));
        assert_eq!(
            parse_dir(DEFAULT_DATA_DIR).unwrap().join(SQLITE_FILE),
            Path::new(".stargate/sqlite.db")
        );
        assert_eq!(
            parse_dir(DEFAULT_DATA_DIR)
                .unwrap()
                .join(MEMORY_SNAPSHOT_FILE),
            Path::new(".stargate/memory.bin")
        );
    }

    #[test]
    fn an_override_moves_both_edge_files_together() {
        let directory = parse_dir("/var/lib/stargate").unwrap();
        assert_eq!(
            directory.join(SQLITE_FILE),
            Path::new("/var/lib/stargate/sqlite.db")
        );
        assert_eq!(
            directory.join(MEMORY_SNAPSHOT_FILE),
            Path::new("/var/lib/stargate/memory.bin")
        );
    }

    #[test]
    fn an_empty_or_blank_override_is_rejected() {
        for value in ["", "   ", "\t"] {
            let error = parse_dir(value).expect_err("blank values must be rejected");
            assert!(error.to_string().contains(DATA_PATH_ENV));
        }
    }
}
