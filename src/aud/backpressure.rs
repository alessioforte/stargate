use super::status::PauseReason;
use store::{StreamGroupState, StreamId};

/// A snapshot boundary remains safe as consumers deliver and acknowledge more
/// entries. Manual rewinds or creating groups after publication are unsupported.
pub(super) fn trim_boundary(
    groups: Option<&[StreamGroupState]>,
    configured: &[String],
) -> Result<Option<StreamId>, PauseReason> {
    if configured.is_empty() {
        return Ok(None);
    }
    let groups = groups.ok_or(PauseReason::GroupMissing)?;
    let mut boundary = None;
    for name in configured {
        let group = groups
            .iter()
            .find(|group| &group.name == name)
            .ok_or(PauseReason::GroupMissing)?;
        let limit = group
            .oldest_pending
            .map_or(group.last_delivered, |id| id.min(group.last_delivered));
        boundary = Some(boundary.map_or(limit, |id: StreamId| id.min(limit)));
    }
    Ok(boundary)
}
pub(super) fn at_capacity(length: u64, maxlen: Option<usize>, batch: i64) -> bool {
    maxlen.is_some_and(|maxlen| {
        let maxlen = maxlen as u64;
        let margin = (maxlen / 10).max((batch as u64).saturating_mul(4));
        length.saturating_add(margin) >= maxlen
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn group(name: &str, last: &str, pending: Option<&str>) -> StreamGroupState {
        StreamGroupState {
            name: name.into(),
            last_delivered: last.parse().unwrap(),
            pending: u64::from(pending.is_some()),
            oldest_pending: pending.map(|id| id.parse().unwrap()),
        }
    }
    #[test]
    fn trim_uses_the_earliest_position_across_all_groups() {
        let names = vec!["a".into(), "b".into()];
        for (groups, expected) in [
            (
                vec![group("a", "10-0", Some("9-5")), group("b", "10-1", None)],
                "9-5",
            ),
            (
                vec![group("a", "10-0", None), group("b", "9-5", None)],
                "9-5",
            ),
            (
                vec![group("a", "10-0", Some("9-5")), group("b", "0-0", None)],
                "0-0",
            ),
            (
                vec![
                    group("a", "10-0", Some("9-5")),
                    group("b", "9-4", Some("8-2")),
                ],
                "8-2",
            ),
        ] {
            assert_eq!(
                trim_boundary(Some(&groups), &names),
                Ok(Some(expected.parse().unwrap()))
            );
        }
        assert_eq!(trim_boundary(None, &names), Err(PauseReason::GroupMissing));
        assert_eq!(
            trim_boundary(Some(&[group("a", "0-0", None)]), &names),
            Err(PauseReason::GroupMissing)
        );
        assert_eq!(trim_boundary(None, &[]), Ok(None));
    }
    #[test]
    fn capacity_includes_the_batch_and_replica_margin() {
        for (length, maxlen, batch, expected) in [
            (599, Some(1000), 100, false),
            (600, Some(1000), 100, true),
            (900, Some(1000), 1, true),
            (899, Some(1000), 1, false),
            (u64::MAX, None, 100, false),
            (0, Some(10), 100, true),
        ] {
            assert_eq!(at_capacity(length, maxlen, batch), expected);
        }
    }
}
