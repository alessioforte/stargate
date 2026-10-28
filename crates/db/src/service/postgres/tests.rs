use super::*;

#[test]
fn creation_races_require_the_exact_error_and_a_confirmed_database() {
    for (code, constraint, exists, expected) in [
        (Some("42P04"), None, true, true),
        (Some("42P04"), None, false, false),
        (Some("23505"), Some("pg_database_datname_index"), true, true),
        (
            Some("23505"),
            Some("pg_database_datname_index"),
            false,
            false,
        ),
        (
            Some("23505"),
            Some("another_unique_constraint"),
            true,
            false,
        ),
        (Some("23505"), None, true, false),
        (Some("42501"), None, true, false),
        (None, None, true, false),
    ] {
        assert_eq!(is_creation_race(code, constraint, exists), expected);
    }
}

fn test_options() -> PgConnectOptions {
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set for postgres tests")
        .parse()
        .expect("valid PostgreSQL test URL")
}

#[tokio::test]
#[ignore = "requires PostgreSQL with CREATEROLE and CREATEDB privileges"]
async fn connection_options_preserve_special_password_characters() {
    let options = test_options();
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    let suffix = ulid::Ulid::generate().to_string().to_ascii_lowercase();
    let role = format!("password_role_{suffix}");
    let database = format!("password_database_{suffix}");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE ROLE \"{role}\" LOGIN CREATEDB PASSWORD 'p@ss:w/rd?#%'"
    )))
    .execute(&pool)
    .await
    .unwrap();

    let result: Result<()> = async {
        let options = options
            .username(&role)
            .password("p@ss:w/rd?#%")
            .database(&database);
        ensure_database(&options).await?;
        let service = init_with_options(options).await?;
        let result = service.ping().await;
        service.pool.close().await;
        result
    }
    .await;

    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS \"{database}\""
    )))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP ROLE \"{role}\"")))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    result.expect("create, migrate and connect with the unencoded password");
}

#[tokio::test]
#[ignore = "requires PostgreSQL with CREATEDB privileges"]
async fn three_concurrent_database_creations_emit_no_warnings_or_errors() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tracing::{Level, Subscriber, instrument::WithSubscriber};
    use tracing_subscriber::{Layer, layer::Context, prelude::*};

    struct Warnings(Arc<AtomicUsize>);

    impl<S: Subscriber> Layer<S> for Warnings {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            if matches!(*event.metadata().level(), Level::WARN | Level::ERROR) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    let options = test_options();
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    let database = format!("concurrent_database_{}", ulid::Ulid::generate());
    let options = options.database(&database);
    let warnings = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry().with(Warnings(warnings.clone()));
    let outcomes = async {
        tokio::join!(
            ensure_database(&options),
            ensure_database(&options),
            ensure_database(&options),
        )
    }
    .with_subscriber(subscriber)
    .await;
    let exists = database_exists(&pool, &database).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS \"{database}\""
    )))
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    for result in [outcomes.0, outcomes.1, outcomes.2] {
        result.expect("every concurrent creator succeeds");
    }
    assert!(exists);
    assert_eq!(warnings.load(Ordering::Relaxed), 0);
}
