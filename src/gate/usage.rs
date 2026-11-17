//! Backend command usage of one gateway request.

use crate::iam::auth::identity::AuthKind;
use crate::obs::telemetry::{self, StorePath};
use lim::LimitKind;

/// Redis commands one gateway request issued, per accounting path.
///
/// A path is reported only when the request exercised it: an absent path is not
/// a zero measurement, so the per-request histogram counts one observation per
/// path actually used.
#[derive(Debug, Default)]
pub(crate) struct StoreUsage {
    auth: Option<(StorePath, u32)>,
    rate_limit: Option<u32>,
    quota: Option<u32>,
}

impl StoreUsage {
    /// Attribute the authentication stage to the credential the request
    /// presented. Anonymous requests report too: their credentials cost none.
    pub(crate) fn record_auth(&mut self, kind: AuthKind, commands: u32) {
        self.auth = Some((auth_path(kind), commands));
    }

    /// Add one traffic-limit check's commands to its path.
    pub(crate) fn record_limit(&mut self, kind: LimitKind, commands: u32) {
        let path = match kind {
            LimitKind::Rate => &mut self.rate_limit,
            LimitKind::Quota => &mut self.quota,
        };
        *path = Some(path.unwrap_or(0).saturating_add(commands));
    }

    /// Report one observation per path the request exercised.
    pub(crate) fn record(self) {
        for (path, commands) in self.observations() {
            telemetry::record_store_commands(path, commands);
        }
    }

    fn observations(&self) -> impl Iterator<Item = (StorePath, u32)> {
        self.auth.into_iter().chain(
            [
                (StorePath::RateLimit, self.rate_limit),
                (StorePath::Quota, self.quota),
            ]
            .into_iter()
            .filter_map(|(path, commands)| Some((path, commands?))),
        )
    }
}

fn auth_path(kind: AuthKind) -> StorePath {
    match kind {
        AuthKind::ApiKey => StorePath::ApiKey,
        AuthKind::Jwt => StorePath::Session,
        AuthKind::OAuth => StorePath::Oauth,
        AuthKind::Anonymous => StorePath::Anonymous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observations(usage: &StoreUsage) -> Vec<(StorePath, u32)> {
        usage.observations().collect()
    }

    #[test]
    fn every_credential_kind_maps_to_a_documented_path() {
        for (kind, path) in [
            (AuthKind::Anonymous, StorePath::Anonymous),
            (AuthKind::ApiKey, StorePath::ApiKey),
            (AuthKind::Jwt, StorePath::Session),
            (AuthKind::OAuth, StorePath::Oauth),
        ] {
            let mut usage = StoreUsage::default();
            usage.record_auth(kind, 2);
            assert_eq!(observations(&usage), [(path, 2)]);
        }
    }

    #[test]
    fn limit_checks_accumulate_per_path_and_keep_jwt_auth_separate() {
        let mut usage = StoreUsage::default();
        usage.record_auth(AuthKind::Jwt, 3);
        usage.record_limit(LimitKind::Rate, 1);
        usage.record_limit(LimitKind::Rate, 1);
        usage.record_limit(LimitKind::Quota, 2);
        assert_eq!(
            observations(&usage),
            [
                (StorePath::Session, 3),
                (StorePath::RateLimit, 2),
                (StorePath::Quota, 2)
            ]
        );
    }

    #[test]
    fn paths_a_request_did_not_exercise_are_not_reported() {
        let mut usage = StoreUsage::default();
        usage.record_limit(LimitKind::Rate, 2);
        assert_eq!(observations(&usage), [(StorePath::RateLimit, 2)]);
    }
}
