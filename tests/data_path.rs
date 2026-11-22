//! The edge data directory is one setting for the SQLite database and the
//! in-memory snapshot; the CLI and the server must resolve the same paths.

#![cfg(feature = "sqlite")]

#[cfg(feature = "memory")]
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(feature = "memory")]
use std::process::{Child, Stdio};
#[cfg(feature = "memory")]
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

const GATEWAY_SEED: &str = r#"
schema: stargate/v1
ingress:
  limit: ingress
  timeout: 250ms
limits:
  default:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 1s
  ingress:
    strategy: gcra
    params:
      max_burst: 100
      replenish_1_per: 100ms
"#;

const TEMPLATES: [&str; 6] = [
    "signup-request",
    "signup-account-exists",
    "signup-completed",
    "change-password-request",
    "password-changed",
    "email-otp",
];

#[cfg(feature = "memory")]
const ADMIN_EMAIL: &str = "admin@example.com";
#[cfg(feature = "memory")]
const ADMIN_PASSWORD: &str = "Stargate-test-password-1!";

fn temporary_dir() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after the Unix epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "stargate-data-path-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).expect("create temporary directory");
    path
}

fn isolated_root() -> PathBuf {
    let root = temporary_dir();
    let seeds = root.join("seeds");
    std::fs::create_dir_all(&seeds).expect("create seed directory");
    std::fs::write(seeds.join("config.yaml"), GATEWAY_SEED).expect("write gateway seed");
    write_email_templates(&root);
    root
}

fn write_email_templates(root: &Path) {
    let directory = root.join(".stargate/transactional");
    std::fs::create_dir_all(&directory).expect("create template directory");
    let mut subjects = serde_json::Map::new();
    for locale in ["en", "it"] {
        let mut templates = serde_json::Map::new();
        for template in TEMPLATES {
            templates.insert(template.to_string(), serde_json::json!("Subject"));
            std::fs::write(directory.join(format!("{template}.{locale}.html")), "Body")
                .expect("write email template");
        }
        subjects.insert(locale.to_string(), serde_json::Value::Object(templates));
    }
    std::fs::write(
        directory.join("subjects.json"),
        serde_json::Value::Object(subjects).to_string(),
    )
    .expect("write email subjects");
}

fn isolated_command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stargate"));
    command
        .current_dir(root)
        .env_remove("CONFIG_PATH")
        .env_remove("DATA_PATH")
        .env_remove("CORS_ORIGINS")
        .env_remove("TRUSTED_ORIGINS")
        .env_remove("OAUTH_CLIENT_CA_FILE")
        .env_remove("STORE_MAX_MEMORY_MB")
        .env_remove("STARGATE_RUNTIME_PROFILE");
    command
}

#[cfg(feature = "memory")]
fn reserve_port() -> u16 {
    let address = TcpListener::bind("127.0.0.1:0").expect("reserve a port");
    let port = address.local_addr().expect("read the port").port();
    drop(address);
    port
}

#[cfg(feature = "memory")]
fn spawn_server(root: &Path, seeds: &Path, data: &Path, port: u16, log: &Path) -> Child {
    isolated_command(root)
        .env("PORT", port.to_string())
        .env("CONFIG_PATH", seeds)
        .env("DATA_PATH", data)
        .env("SERVER_DRAIN_DELAY_SECS", "0")
        .env("SERVER_SHUTDOWN_TIMEOUT_SECS", "5")
        .env("RUST_LOG", "error")
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(log).expect("create server log"))
        .spawn()
        .expect("start the server")
}

#[cfg(feature = "memory")]
fn wait_until_listening(mut child: Child, port: u16, log: &Path) -> Child {
    let deadline = Instant::now() + Duration::from_secs(30);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        if let Some(status) = child.try_wait().expect("poll the server") {
            panic!(
                "server exited before listening: {status}\n{}",
                std::fs::read_to_string(log).unwrap_or_default()
            );
        }
        assert!(Instant::now() < deadline, "server did not start listening");
        std::thread::sleep(Duration::from_millis(100));
    }
    child
}

#[cfg(feature = "memory")]
fn terminate_server(mut child: Child, log: &Path) {
    let signal = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("send SIGTERM");
    assert!(signal.success(), "SIGTERM must be delivered");

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().expect("poll the server") {
            assert!(
                status.success(),
                "server did not shut down cleanly: {status}\n{}",
                std::fs::read_to_string(log).unwrap_or_default()
            );
            return;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            panic!(
                "server did not shut down\n{}",
                std::fs::read_to_string(log).unwrap_or_default()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn the_cli_opens_the_database_in_data_path() {
    let root = isolated_root();
    // A space proves the path is not parsed as a SQLite URL.
    let data = root.join("custom data");
    let output = isolated_command(&root)
        .env("DATA_PATH", &data)
        .args(["db", "migrate"])
        .output()
        .expect("run the CLI");

    assert!(
        output.status.success(),
        "db migrate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        data.join("sqlite.db").is_file(),
        "sqlite.db must be created under DATA_PATH"
    );
    assert!(
        !root.join(".stargate/sqlite.db").exists(),
        "the default data directory must stay unused"
    );

    let second = isolated_command(&root)
        .env("DATA_PATH", &data)
        .args(["db", "migrate"])
        .output()
        .expect("run the CLI again");
    assert!(
        second.status.success(),
        "the same DATA_PATH must reopen the database: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn an_unusable_data_path_names_the_variable_and_the_path() {
    let root = isolated_root();
    let data = root.join("not-a-directory");
    std::fs::write(&data, "a regular file").expect("write the invalid data path");

    let output = isolated_command(&root)
        .env("DATA_PATH", &data)
        .args(["db", "migrate"])
        .output()
        .expect("run the CLI");

    assert!(!output.status.success(), "db migrate must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("DATA_PATH"), "{stderr}");
    assert!(stderr.contains(&data.display().to_string()), "{stderr}");
    assert!(
        stderr.contains("File exists"),
        "the OS cause must stay visible: {stderr}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[cfg(feature = "memory")]
#[test]
fn the_server_writes_both_edge_files_and_flushes_the_snapshot() {
    let root = isolated_root();
    let seeds = root.join("seeds");
    let data = root.join("custom data");
    let log = root.join("server.log");
    let port = reserve_port();

    let child = spawn_server(&root, &seeds, &data, port, &log);
    let child = wait_until_listening(child, port, &log);
    terminate_server(child, &log);

    assert!(
        data.join("sqlite.db").is_file(),
        "sqlite.db must be created under DATA_PATH"
    );
    assert!(
        data.join("memory.bin").is_file(),
        "memory.bin must be saved under DATA_PATH"
    );
    assert!(
        !root.join(".stargate/sqlite.db").exists(),
        "the default data directory must stay unused"
    );
    assert!(
        !root.join(".stargate/memory.bin").exists(),
        "the snapshot must not fall back to the default data directory"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[cfg(feature = "memory")]
#[tokio::test]
async fn a_session_survives_a_restart_with_data_path() {
    let root = isolated_root();
    let seeds = root.join("seeds");
    let data = root.join("custom data");
    let log = root.join("server.log");
    let port = reserve_port();
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::new();

    let bootstrap = isolated_command(&root)
        .env("CONFIG_PATH", &seeds)
        .env("DATA_PATH", &data)
        .args([
            "admin",
            "bootstrap",
            "--email",
            ADMIN_EMAIL,
            "--password",
            ADMIN_PASSWORD,
            "--oauth-redirect-uri",
            "https://identity.example.com/stargate/auth/callback",
        ])
        .output()
        .expect("bootstrap the first super admin");
    assert!(
        bootstrap.status.success(),
        "admin bootstrap failed: {}",
        String::from_utf8_lossy(&bootstrap.stderr)
    );

    let child = spawn_server(&root, &seeds, &data, port, &log);
    let mut child = wait_until_listening(child, port, &log);
    wait_until_ready(&client, &base, &mut child, &log).await;

    let tokens: serde_json::Value = client
        .post(format!("{base}/account/login"))
        .json(&serde_json::json!({
            "username": ADMIN_EMAIL,
            "password": ADMIN_PASSWORD,
        }))
        .send()
        .await
        .expect("log in")
        .error_for_status()
        .expect("login succeeds")
        .json()
        .await
        .expect("read the session tokens");
    let access_token = tokens["accessToken"]
        .as_str()
        .expect("access token")
        .to_string();

    assert_profile(&client, &base, &access_token, 200).await;
    terminate_server(child, &log);
    assert!(
        data.join("memory.bin").is_file(),
        "the snapshot must be saved under DATA_PATH"
    );

    let child = spawn_server(&root, &seeds, &data, port, &log);
    let mut child = wait_until_listening(child, port, &log);
    wait_until_ready(&client, &base, &mut child, &log).await;
    assert_profile(&client, &base, &access_token, 200).await;
    terminate_server(child, &log);

    // The same token must fail once the snapshot is gone: sessions live in the
    // store, not in SQLite.
    std::fs::remove_file(data.join("memory.bin")).expect("remove the snapshot");
    let child = spawn_server(&root, &seeds, &data, port, &log);
    let mut child = wait_until_listening(child, port, &log);
    wait_until_ready(&client, &base, &mut child, &log).await;
    assert_profile(&client, &base, &access_token, 401).await;
    terminate_server(child, &log);

    std::fs::remove_dir_all(&root).ok();
}

#[cfg(feature = "memory")]
async fn wait_until_ready(client: &reqwest::Client, base: &str, child: &mut Child, log: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().expect("poll the server") {
            panic!(
                "server exited before becoming ready: {status}\n{}",
                std::fs::read_to_string(log).unwrap_or_default()
            );
        }
        if let Ok(response) = client.get(format!("{base}/readyz")).send().await
            && response.status().is_success()
        {
            return;
        }
        assert!(Instant::now() < deadline, "server did not become ready");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(feature = "memory")]
async fn assert_profile(client: &reqwest::Client, base: &str, access_token: &str, expected: u16) {
    let response = client
        .get(format!("{base}/account/profile"))
        .bearer_auth(access_token)
        .send()
        .await
        .expect("read the profile");
    assert_eq!(response.status().as_u16(), expected);
}
