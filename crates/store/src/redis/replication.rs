use super::pool::{RedisConnection, with_request_budget};
use crate::{StoreError, StoreResult};
use redis::{FromRedisValue, Pipeline};

/// Confirm critical writes on replicas before returning to the caller.
#[derive(Clone, Debug)]
pub struct RedisReplicationConfig {
    pub replicas: std::num::NonZeroU32,
    pub timeout_ms: std::num::NonZeroU32,
    pub fail_on_timeout: bool,
    pub on_timeout: fn(bool),
}

#[derive(Clone)]
pub(super) struct Replication {
    // WAIT blocks a Redis socket. Keep ordinary reads and traffic checks on
    // the regular pool while a replica catches up.
    connection: RedisConnection,
    config: RedisReplicationConfig,
}

impl Replication {
    pub(super) fn new(connection: RedisConnection, config: RedisReplicationConfig) -> Self {
        Self { connection, config }
    }

    pub(super) async fn confirm<T: FromRedisValue>(&self, mut writes: Pipeline) -> StoreResult<T> {
        let config = &self.config;
        // A plain pipeline binds the writes and WAIT to one socket, even if
        // it reconnects before dispatch. WAIT cannot block inside MULTI.
        // https://redis.io/docs/latest/commands/wait/
        writes
            .cmd("WAIT")
            .arg(config.replicas.get())
            .arg(config.timeout_ms.get());
        let mut connection = self.connection.clone();
        let (result, acknowledged): (T, u32) =
            with_request_budget(None, writes.query_async(&mut connection)).await?;
        if acknowledged < config.replicas.get() {
            let fail = config.fail_on_timeout;
            (config.on_timeout)(fail);
            tracing::warn!(
                acknowledged,
                required = config.replicas.get(),
                fail,
                "Redis write replication timed out"
            );
            if fail {
                return Err(StoreError::ReplicationShortfall {
                    acknowledged,
                    required: config.replicas.get(),
                });
            }
        }
        Ok(result)
    }
}
