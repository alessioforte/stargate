//! Invalid process-wide security settings must stop the server before it
//! serves anything, not silently fall back to a default.

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temporary_dir() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after the Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "stargate-startup-settings-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("create temporary directory");
    path
}

fn assert_startup_fails(variable: &str, value: &str) {
    let dir = temporary_dir();
    let output = Command::new(env!("CARGO_BIN_EXE_stargate"))
        .current_dir(&dir)
        // The test controls the setting under test; ambient values must not
        // decide how this process starts.
        .env_remove("CORS_ORIGINS")
        .env_remove("TRUSTED_ORIGINS")
        .env_remove("OAUTH_CLIENT_CA_FILE")
        .env_remove("STORE_MAX_MEMORY_MB")
        .env_remove("CONFIG_PATH")
        .env(variable, value)
        .output()
        .expect("run the server binary");
    std::fs::remove_dir_all(&dir).ok();

    assert!(
        !output.status.success(),
        "`{variable}={value}` must stop startup"
    );
    let logs = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        logs.contains(variable),
        "`{variable}={value}` must name the setting it refused: {logs}"
    );
}

#[test]
fn invalid_security_settings_stop_startup() {
    assert_startup_fails("CORS_ORIGINS", "https://app.example.com/login");
    assert_startup_fails("TRUSTED_ORIGINS", "https://app.example.com,*");
    assert_startup_fails(
        "OAUTH_CLIENT_CA_FILE",
        "/nonexistent/stargate-oauth-client-ca.pem",
    );
    if cfg!(feature = "edge") {
        assert_startup_fails("STORE_MAX_MEMORY_MB", "0");
    }
}
