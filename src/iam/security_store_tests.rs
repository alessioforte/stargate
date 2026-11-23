//! Exercise the real security services with a store that rejects data commands.
use crate::err::{ErrorCode, ErrorResponse};
use axum::body::Body;
use http::{Request, StatusCode, header::CONTENT_TYPE};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const CHILD: &str = "STARGATE_TEST_SECURITY_STORE_FAILURE";

/// Run `test` in a child process whose Redis accepts connections but rejects
/// every command. The store therefore reports itself unavailable at the first
/// use, while the process keeps the settings of a working deployment.
async fn run_with_unavailable_store(test: &'static str) -> std::process::Output {
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
        crate::conf::child_test::run(test, |command| {
            command
                .env(CHILD, "1")
                .env("REDIS_URL", url)
                .env("SIGNUP_MODE", "open")
                .env("EMAIL_OTP_PEPPER", "synthetic-store-failure-pepper")
                .env("PASSWORDLESS_LOGIN_ENABLED", "true")
                .env("MFA_MODE", "optional")
                .env_remove("REDIS_USERNAME")
                .env_remove("REDIS_PASSWORD")
                .env_remove("REDIS_CA_FILE");
        })
    })
    .await
    .unwrap();
    server.abort();
    output
}

fn assert_unavailable(error: ErrorResponse) {
    assert_eq!(error.code, ErrorCode::AuthStateUnavailable, "{error:?}");
    let response = axum::response::IntoResponse::into_response(error);
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "10");
}

fn assert_store_error_unavailable(error: store::StoreError) {
    assert!(error.is_unavailable(), "{error}");
    assert_unavailable(error.into());
}

fn json_request(uri: &str, body: serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

fn bearer_request(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn authenticated_json_request(uri: &str, token: &str, body: serde_json::Value) -> Request<Body> {
    let mut request = json_request(uri, body);
    *request.method_mut() = http::Method::PUT;
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    request
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn login_guard_and_recipient_throttle_stay_closed_on_store_errors() {
    if std::env::var_os(CHILD).is_none() {
        let output = run_with_unavailable_store(
            "iam::security_store_tests::login_guard_and_recipient_throttle_stay_closed_on_store_errors",
        )
        .await;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    crate::data::store::init().await.unwrap();
    let throttle = super::auth::login_guard::LoginThrottle::for_identifier("203.0.113.1", "user");
    assert!(throttle.reserve().await.is_err());
    let error = super::email_throttle::check_and_record_email_request(
        super::email_throttle::EmailFlow::PasswordReset,
        "user@example.com",
    )
    .await
    .unwrap_err();
    assert_unavailable(error);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_flows_stay_closed_on_store_errors() {
    if std::env::var_os(CHILD).is_none() {
        let output = run_with_unavailable_store(
            "iam::security_store_tests::account_flows_stay_closed_on_store_errors",
        )
        .await;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    crate::data::store::init().await.unwrap();

    // OTP and MFA start, verification and step-up.
    assert_unavailable(
        crate::api::account::otp::post_login_email_otp(json_request(
            "/account/login/otp/email",
            serde_json::json!({ "email": "user@example.com" }),
        ))
        .await
        .unwrap_err(),
    );
    assert_unavailable(
        crate::api::account::otp::put_login_email_otp(json_request(
            "/account/login/otp/email",
            serde_json::json!({
                "challengeId": "0123456789abcdef0123456789abcdef",
                "code": "123456",
            }),
        ))
        .await
        .unwrap_err(),
    );
    assert_unavailable(
        crate::api::account::otp::put_login_mfa_challenge(
            axum::extract::Path("0123456789abcdef0123456789abcdef".to_string()),
            json_request(
                "/account/login/mfa/challenges/0123456789abcdef0123456789abcdef",
                serde_json::json!({ "code": "123456" }),
            ),
        )
        .await
        .unwrap_err(),
    );

    let claims = jwt::Claims::default()
        .subject("user@example.com".to_string())
        .sub_id("01JZ000000000000000000000A".to_string())
        .sid("01JZ000000000000000000000B".to_string())
        .email("user@example.com".to_string());
    let session = crate::iam::auth::jwt::jwt_config()
        .generate_session_access_token(claims.clone())
        .unwrap();
    assert_unavailable(
        crate::api::account::otp::get_mfa_methods(bearer_request("/account/mfa/methods", &session))
            .await
            .unwrap_err(),
    );
    assert_unavailable(
        crate::api::account::otp::put_mfa_challenge(
            axum::extract::Path("0123456789abcdef0123456789abcdef".to_string()),
            authenticated_json_request(
                "/account/mfa/challenges/0123456789abcdef0123456789abcdef",
                &session,
                serde_json::json!({ "code": "123456" }),
            ),
        )
        .await
        .unwrap_err(),
    );

    let user = db::ent::User::new("user@example.com".to_string(), "user".to_string());
    assert_unavailable(
        super::otp::challenge::store_mfa_verified_marker(
            &super::otp::types::AuthenticatedUser {
                user: user.clone(),
                sid: "01JZ000000000000000000000B".to_string(),
            },
            super::otp::types::OtpMethod::Email,
            "login",
            1_700_000_000,
        )
        .await
        .unwrap_err(),
    );
    assert_store_error_unavailable(
        super::sess::registry::add_otp_factor("01JZ000000000000000000000B", &user.id)
            .await
            .unwrap_err(),
    );

    // Password reset: issuing the request and completing it both need the store.
    assert_store_error_unavailable(
        super::pwd::reset::issue_change_password_request("user@example.com", None)
            .await
            .unwrap_err(),
    );
    assert_unavailable(
        crate::api::account::credentials::forgot::post_credentials(axum::Json(
            crate::api::account::credentials::forgot::ForgotPasswordRequestBody {
                email: "user@example.com".to_string(),
                interaction_id: None,
            },
        ))
        .await
        .unwrap_err(),
    );
    let reset = crate::iam::auth::jwt::jwt_config()
        .generate_password_reset_token(claims.clone())
        .unwrap();
    assert_unavailable(
        crate::api::account::credentials::reset::put_credentials(
            Request::builder()
                .method("PUT")
                .uri("/account/credentials")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "token": reset,
                        "password": "Str0ng-Passw0rd!123",
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap_err(),
    );

    // Signup: starting, opening the link and completing it.
    assert_unavailable(
        crate::api::signup::request::post_signup(
            http::HeaderMap::new(),
            axum::Json(crate::api::signup::SignupRequestBody {
                email: "new@example.com".to_string(),
                interaction_id: None,
            }),
        )
        .await
        .unwrap_err(),
    );
    let verification = crate::iam::auth::jwt::jwt_config()
        .generate_signup_verification_token(
            jwt::Claims::default()
                .subject("signup_request".to_string())
                .sid("01JZ000000000000000000000C".to_string())
                .email("new@example.com".to_string()),
        )
        .unwrap();
    assert_unavailable(
        crate::api::signup::verification::get_signup(axum::extract::Query(
            crate::api::signup::SignupVerificationParams {
                token: verification,
            },
        ))
        .await
        .unwrap_err(),
    );
    let completion = crate::iam::auth::jwt::jwt_config()
        .generate_signup_token(
            jwt::Claims::default()
                .subject("signup".to_string())
                .sub_id("01JZ000000000000000000000C".to_string())
                .email("new@example.com".to_string()),
        )
        .unwrap();
    assert_unavailable(
        crate::api::signup::complete::put_signup(json_request(
            "/signup",
            serde_json::json!({
                "token": completion,
                "password": "Str0ng-Passw0rd!123",
                "givenName": "",
                "familyName": "",
                "nickname": "",
                "phoneNumber": null,
            }),
        ))
        .await
        .unwrap_err(),
    );
    assert_store_error_unavailable(
        super::user::email_verification::create_email_verification_request(
            &super::user::signup::PendingSignupProfile::self_service(
                "new@example.com",
                tools::Locale::En,
            ),
        )
        .await
        .unwrap_err(),
    );
}
