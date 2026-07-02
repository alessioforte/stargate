use std::{env, io};

#[cfg(all(feature = "edge", feature = "cluster"))]
compile_error!(
    "features `edge` and `cluster` are mutually exclusive; build with exactly one profile, e.g. `--features edge` or `--features cluster`"
);

#[cfg(all(feature = "sqlite", feature = "postgres"))]
compile_error!("stargate must be built with exactly one database backend: sqlite or postgres");

#[cfg(not(any(feature = "sqlite", feature = "postgres")))]
compile_error!("stargate must be built with one database backend: sqlite or postgres");

#[cfg(all(feature = "memory", feature = "redis"))]
compile_error!("stargate must be built with exactly one state backend: memory or redis");

#[cfg(not(any(feature = "memory", feature = "redis")))]
compile_error!("stargate must be built with one state backend: memory or redis");

pub const COMPILED_DB_BACKEND: &str = if cfg!(feature = "postgres") {
    "postgres"
} else {
    "sqlite"
};

pub const COMPILED_STATE_BACKEND: &str = if cfg!(feature = "redis") {
    "redis"
} else {
    "memory"
};

pub const COMPILED_PROFILE: &str = if cfg!(all(feature = "sqlite", feature = "memory")) {
    "edge"
} else if cfg!(all(feature = "postgres", feature = "redis")) {
    "cluster"
} else {
    "custom"
};

fn parse_runtime_profile(value: &str) -> io::Result<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "edge" => Ok("edge"),
        "cluster" => Ok("cluster"),
        "custom" => Ok("custom"),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("STARGATE_RUNTIME_PROFILE must be one of: edge, cluster, custom; got: {value}"),
        )),
    }
}

pub fn validate_runtime_profile() -> io::Result<&'static str> {
    let requested = match env::var("STARGATE_RUNTIME_PROFILE") {
        Ok(value) => parse_runtime_profile(&value)?,
        Err(env::VarError::NotPresent) => COMPILED_PROFILE,
        Err(env::VarError::NotUnicode(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "STARGATE_RUNTIME_PROFILE must contain valid unicode data",
            ));
        }
    };

    if requested != COMPILED_PROFILE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "runtime profile {requested} does not match compiled profile {compiled} (db={db}, state={state})",
                compiled = COMPILED_PROFILE,
                db = COMPILED_DB_BACKEND,
                state = COMPILED_STATE_BACKEND,
            ),
        ));
    }

    Ok(requested)
}

#[cfg(test)]
mod tests {
    use super::parse_runtime_profile;

    #[test]
    fn parse_runtime_profile_accepts_known_profiles() {
        assert_eq!(parse_runtime_profile("edge").unwrap(), "edge");
        assert_eq!(parse_runtime_profile("Cluster").unwrap(), "cluster");
        assert_eq!(parse_runtime_profile("custom").unwrap(), "custom");
    }

    #[test]
    fn parse_runtime_profile_rejects_unknown_profiles() {
        assert!(parse_runtime_profile("ha").is_err());
    }
}
