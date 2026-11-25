//! Transactions over the keys of one entity, shaped like a login session.

use redis::AsyncCommands;
use store::{RedisStore, Store, StoreError, Transaction, keys};

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn ttl(store: &RedisStore, key: &str) -> Result<i64, Box<dyn std::error::Error>> {
    let physical = store.prefixed_key(key);
    Ok(store.get_connection().ttl(physical.as_ref()).await?)
}

/// Create, renew and delete three keys under `tag` with `commit`, and with
/// `confirmed` for the deletions.
pub async fn exercise(store: &RedisStore, tag: &str, confirmed: &RedisStore) -> TestResult {
    let [subject, refresh, record] =
        ["subject", "refresh", "session"].map(|name| keys::scoped(tag, name));

    let create = Transaction::new()
        .set(&subject, &"subject", 30)?
        .set(&refresh, &"first", 30)?
        .set(&record, &1, 30)?;
    assert!(store.commit(&create).await?);

    let renew = |presented: &str| -> Result<Transaction, StoreError> {
        Ok(Transaction::new()
            .require(&subject)
            .require_equal(&refresh, &presented)?
            .set(&refresh, &"second", 60)?
            .swap(&record, &1, &2)?
            .expire(&subject, 60))
    };
    assert!(!store.commit(&renew("replayed")?).await?);
    assert_eq!(
        store.get::<String>(&refresh).await?.as_deref(),
        Some("first")
    );
    assert_eq!(store.get::<i32>(&record).await?, Some(1));

    assert!(store.commit(&renew("first")?).await?);
    assert_eq!(
        store.get::<String>(&refresh).await?.as_deref(),
        Some("second")
    );
    assert_eq!(store.get::<i32>(&record).await?, Some(2));
    assert!(ttl(store, &record).await? <= 30, "swap keeps the expiry");
    assert!(ttl(store, &subject).await? > 30);

    let delete = Transaction::new()
        .delete(&subject)
        .delete(&refresh)
        .delete(&record);
    assert!(confirmed.commit_confirmed(&delete).await?);
    // A deletion that finds nothing is still a replicated write.
    assert!(confirmed.commit_confirmed(&delete).await?);
    assert!(!store.commit(&renew("second")?).await?);
    for key in [&subject, &refresh, &record] {
        assert!(!store.exists(key).await?, "{key}");
    }

    let other = keys::scoped(&format!("{tag}-other"), "session");
    let crossing = Transaction::new().require(&subject).delete(&other);
    assert!(matches!(
        store.commit(&crossing).await,
        Err(StoreError::InvalidInput(_))
    ));
    Ok(())
}
