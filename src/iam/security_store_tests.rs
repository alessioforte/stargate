//! Exercise the real security services with a store that rejects data commands.
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_guard_and_recipient_throttle_stay_closed_on_store_errors() {
    const CHILD: &str = "STARGATE_TEST_SECURITY_STORE_FAILURE";
    if std::env::var_os(CHILD).is_some() {
        crate::data::store::init().await.unwrap();
        let throttle =
            super::auth::login_guard::LoginThrottle::for_identifier("203.0.113.1", "user");
        assert!(
            super::auth::login_guard::check_login_allowed(&throttle)
                .await
                .is_err()
        );
        let error = super::email_throttle::check_and_record_email_request(
            super::email_throttle::EmailFlow::PasswordReset,
            "user@example.com",
        )
        .await
        .unwrap_err();
        assert!(error.status.is_server_error());
        return;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("redis://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut socket = BufReader::new(socket);
                loop {
                    let mut line = String::new();
                    if socket.read_line(&mut line).await.unwrap_or(0) == 0 {
                        break;
                    }
                    let count: usize = line.trim().strip_prefix('*').unwrap().parse().unwrap();
                    let mut command = Vec::new();
                    for _ in 0..count {
                        line.clear();
                        socket.read_line(&mut line).await.unwrap();
                        let len: usize = line.trim().strip_prefix('$').unwrap().parse().unwrap();
                        let mut arg = vec![0; len + 2];
                        socket.read_exact(&mut arg).await.unwrap();
                        arg.truncate(len);
                        command.push(arg);
                    }
                    // Connection setup succeeds. Security reads fail at the backend.
                    let response = if command[0].eq_ignore_ascii_case(b"CLIENT") {
                        b"+OK\r\n".as_slice()
                    } else {
                        b"-ERR store unavailable\r\n".as_slice()
                    };
                    socket.get_mut().write_all(response).await.unwrap();
                }
            });
        }
    });
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "iam::security_store_tests::login_guard_and_recipient_throttle_stay_closed_on_store_errors", "--nocapture"])
            .env(CHILD, "1").env("REDIS_URL", url)
            .env_remove("REDIS_USERNAME").env_remove("REDIS_PASSWORD").env_remove("REDIS_CA_FILE")
            .output().unwrap()
    }).await.unwrap();
    server.abort();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
