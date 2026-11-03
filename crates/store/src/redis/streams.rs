use super::RedisStore;
use crate::{StoreError, StoreResult};
use std::{collections::HashMap, fmt, str::FromStr};

/// Redis stream IDs sort numerically by milliseconds and sequence, not lexically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamId {
    pub millis: u64,
    pub sequence: u64,
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.millis, self.sequence)
    }
}

impl FromStr for StreamId {
    type Err = StoreError;
    fn from_str(value: &str) -> StoreResult<Self> {
        let invalid = || StoreError::TypeMismatch("Invalid Redis stream ID".into());
        let (millis, sequence) = value.split_once('-').ok_or_else(invalid)?;
        Ok(Self {
            millis: millis.parse().map_err(|_| invalid())?,
            sequence: sequence.parse().map_err(|_| invalid())?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamGroupState {
    pub name: String,
    pub last_delivered: StreamId,
    pub pending: u64,
    pub oldest_pending: Option<StreamId>,
}

impl RedisStore {
    /// None means the stream is missing; absent groups are omitted from the result.
    /// Read delivery positions before pending summaries: concurrent delivery or
    /// acknowledgement can only make this older boundary more conservative.
    pub async fn stream_groups(
        &self,
        stream: &str,
        groups: &[String],
    ) -> StoreResult<Option<Vec<StreamGroupState>>> {
        let mut con = self.stream_connection();
        let reply: Vec<HashMap<String, redis::Value>> = match redis::cmd("XINFO")
            .arg("GROUPS")
            .arg(stream)
            .query_async(&mut con)
            .await
        {
            Ok(reply) => reply,
            Err(error) if error.detail() == Some("no such key") => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut result = Vec::new();
        for fields in reply {
            let field = |key: &str| {
                fields
                    .get(key)
                    .ok_or_else(|| StoreError::TypeMismatch(format!("Missing XINFO field {key}")))
            };
            let name: String = redis::from_redis_value(field("name")?.clone())
                .map_err(|e| StoreError::TypeMismatch(e.to_string()))?;
            if !groups.contains(&name) {
                continue;
            }
            let last: String = redis::from_redis_value(field("last-delivered-id")?.clone())
                .map_err(|e| StoreError::TypeMismatch(e.to_string()))?;
            type PendingSummary = (u64, Option<String>, Option<String>, Vec<(String, u64)>);
            let summary: PendingSummary = match redis::cmd("XPENDING")
                .arg(stream)
                .arg(&name)
                .query_async(&mut con)
                .await
            {
                Ok(summary) => summary,
                Err(error) if error.code() == Some("NOGROUP") => continue,
                Err(error) => return Err(error.into()),
            };
            let oldest_pending = summary.1.map(|id| id.parse()).transpose()?;
            if (summary.0 > 0) != oldest_pending.is_some() {
                return Err(StoreError::TypeMismatch(
                    "Inconsistent XPENDING summary".into(),
                ));
            }
            result.push(StreamGroupState {
                name,
                last_delivered: last.parse()?,
                pending: summary.0,
                oldest_pending,
            });
        }
        Ok(Some(result))
    }

    /// MINID ~ may retain extra entries, but never removes IDs >= boundary.
    /// Requires Redis/Valkey 6.2 or newer.
    pub async fn trim_stream_before(&self, stream: &str, boundary: StreamId) -> StoreResult<u64> {
        let mut con = self.stream_connection();
        Ok(redis::cmd("XTRIM")
            .arg(stream)
            .arg("MINID")
            .arg("~")
            .arg(boundary.to_string())
            .query_async(&mut con)
            .await?)
    }

    pub async fn stream_length(&self, stream: &str) -> StoreResult<u64> {
        let mut con = self.stream_connection();
        Ok(redis::cmd("XLEN").arg(stream).query_async(&mut con).await?)
    }

    pub async fn first_stream_id_after(
        &self,
        stream: &str,
        boundary: StreamId,
    ) -> StoreResult<Option<StreamId>> {
        let mut con = self.stream_connection();
        let entries: Vec<(String, Vec<(String, String)>)> = redis::cmd("XRANGE")
            .arg(stream)
            .arg(format!("({boundary}"))
            .arg("+")
            .arg("COUNT")
            .arg(1)
            .query_async(&mut con)
            .await?;
        entries.first().map(|(id, _)| id.parse()).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_ids_use_numeric_order_and_validate_both_components() {
        assert!("10-0".parse::<StreamId>().unwrap() > "9-5".parse().unwrap());
        assert!("10-11".parse::<StreamId>().unwrap() > "10-2".parse().unwrap());
        for invalid in ["", "1", "-1-0", "1-x", "1-2-3", "18446744073709551616-0"] {
            assert!(invalid.parse::<StreamId>().is_err(), "{invalid}");
        }
    }
}
