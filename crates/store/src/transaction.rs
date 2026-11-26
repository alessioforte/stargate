use crate::error::{StoreError, StoreResult};
use crate::keys;
use crate::store::SerializeValue;

/// Writes to keys of one hash tag, applied together and only while every
/// requirement holds.
///
/// Redis runs a transaction as one Lua script, so its keys must share a hash
/// tag (one Redis Cluster slot); a transaction that spans tags is rejected
/// before it reaches the server. The memory store applies transactions one at
/// a time: a key that transactions write must be written only by
/// transactions there.
#[derive(Debug, Default)]
pub struct Transaction {
    pub(crate) requirements: Vec<Requirement>,
    pub(crate) writes: Vec<Write>,
}

/// Values are MessagePack; each store applies its own encoding on commit.
#[derive(Debug)]
pub(crate) enum Requirement {
    Exists(String),
    Equals(String, Vec<u8>),
}

#[derive(Debug)]
pub(crate) enum Write {
    Set(String, Vec<u8>, u64),
    /// Only after an `Equals` requirement on the same key, which guarantees
    /// the key exists and keeps an expiry to preserve.
    Replace(String, Vec<u8>),
    Expire(String, u64),
    Delete(String),
}

impl Requirement {
    pub(crate) fn key(&self) -> &str {
        match self {
            Self::Exists(key) | Self::Equals(key, _) => key,
        }
    }
}

impl Write {
    pub(crate) fn key(&self) -> &str {
        match self {
            Self::Set(key, ..)
            | Self::Replace(key, _)
            | Self::Expire(key, _)
            | Self::Delete(key) => key,
        }
    }
}

fn encode<T: SerializeValue>(value: &T) -> StoreResult<Vec<u8>> {
    rmp_serde::to_vec(value)
        .map_err(|e| StoreError::SerializationFailed(format!("Failed to serialize value: {e}")))
}

impl Transaction {
    pub fn new() -> Self {
        Self::default()
    }

    /// Require `key` to hold a value.
    pub fn require(mut self, key: &str) -> Self {
        self.requirements.push(Requirement::Exists(key.to_owned()));
        self
    }

    /// Require `key` to hold `value`, compared in serialized form like
    /// [`crate::Store::compare_and_swap`].
    pub fn require_equal<T: SerializeValue>(mut self, key: &str, value: &T) -> StoreResult<Self> {
        self.requirements
            .push(Requirement::Equals(key.to_owned(), encode(value)?));
        Ok(self)
    }

    /// Store `value` under `key` for `ttl` seconds.
    pub fn set<T: SerializeValue>(mut self, key: &str, value: &T, ttl: u64) -> StoreResult<Self> {
        self.writes
            .push(Write::Set(key.to_owned(), encode(value)?, ttl));
        Ok(self)
    }

    /// Require `key` to hold `expected`, then store `value` with the key's
    /// remaining lifetime.
    pub fn swap<T: SerializeValue>(self, key: &str, expected: &T, value: &T) -> StoreResult<Self> {
        let mut transaction = self.require_equal(key, expected)?;
        transaction
            .writes
            .push(Write::Replace(key.to_owned(), encode(value)?));
        Ok(transaction)
    }

    /// Set the remaining lifetime of `key` when it exists.
    pub fn expire(mut self, key: &str, ttl: u64) -> Self {
        self.writes.push(Write::Expire(key.to_owned(), ttl));
        self
    }

    /// Delete `key`.
    pub fn delete(mut self, key: &str) -> Self {
        self.writes.push(Write::Delete(key.to_owned()));
        self
    }

    /// Reject what a store could not apply atomically or at all.
    pub(crate) fn validate(&self) -> StoreResult<()> {
        let mut keys = self
            .requirements
            .iter()
            .map(Requirement::key)
            .chain(self.writes.iter().map(Write::key));
        let Some(first) = keys.next() else {
            return Err(StoreError::InvalidInput(
                "Transaction has no keys".to_string(),
            ));
        };
        let slot = keys::slot(first);
        for key in std::iter::once(first).chain(keys) {
            if key.trim().is_empty() {
                return Err(StoreError::InvalidInput("Key cannot be empty".to_string()));
            }
            if keys::slot(key) != slot {
                return Err(StoreError::InvalidInput(format!(
                    "Transaction keys must share one hash tag: '{first}' and '{key}'"
                )));
            }
        }
        if self
            .writes
            .iter()
            .any(|write| matches!(write, Write::Set(_, _, 0) | Write::Expire(_, 0)))
        {
            return Err(StoreError::InvalidInput("TTL cannot be zero".to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_must_share_one_hash_tag() {
        let subject = keys::scoped("s:a", "subject");
        let record = keys::scoped("s:a", "session");
        let index = keys::scoped("u:a", "sessions");

        let same = Transaction::new()
            .require(&subject)
            .set(&record, &1, 60)
            .unwrap();
        assert!(same.validate().is_ok());

        let crossing = Transaction::new().require(&subject).delete(&index);
        assert!(matches!(
            crossing.validate(),
            Err(StoreError::InvalidInput(message)) if message.contains("one hash tag")
        ));
    }

    #[test]
    fn empty_keys_zero_ttls_and_empty_transactions_are_rejected() {
        assert!(Transaction::new().validate().is_err());
        assert!(Transaction::new().delete("").validate().is_err());
        assert!(Transaction::new().expire("key", 0).validate().is_err());
        assert!(
            Transaction::new()
                .set("key", &1, 0)
                .unwrap()
                .validate()
                .is_err()
        );
    }
}
