//! Cached per-org gateway context.
//!
//! The small subset of `organizations.attrs` the gateway needs on the hot
//! path — the org's named limit overrides — cached in the store under
//! `org:ctx:{org_id}` with a short TTL. Admin org mutations delete the key
//! (see [`invalidate`]) so attr changes propagate on the next request
//! instead of waiting out the TTL.

use crate::etc::store::use_store;
use serde::{Deserialize, Serialize};
use store::Store;

const ORG_CTX_PREFIX: &str = "org:ctx";
const ORG_CTX_TTL_SECS: u64 = 60;

/// Named limit overrides from org attrs, mirroring the subject-attrs
/// convention: `attrs.rate_limit` / `attrs.quota` name a configured limit
/// spec and override the policy default for `scope: org` checks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgLimitOverrides {
    pub rate_limit: Option<String>,
    pub quota: Option<String>,
}

fn ctx_key(org_id: &str) -> String {
    format!("{ORG_CTX_PREFIX}:{org_id}")
}

/// Extract the gateway-relevant subset from org attrs. Accepts snake_case
/// (canonical, matching subject attrs) and camelCase (matching the
/// `password_policy`/`passwordPolicy` precedent).
pub fn overrides_from_attrs(
    attrs: &serde_json::Value,
) -> Result<OrgLimitOverrides, crate::err::ErrorResponse> {
    use crate::etc::limits::attribute_name;
    let rate_limit = match attribute_name(attrs, "rate_limit", lim::LimitKind::Rate, "org")? {
        Some(name) => Some(name),
        None => attribute_name(attrs, "rateLimit", lim::LimitKind::Rate, "org")?,
    };
    let quota = attribute_name(attrs, "quota", lim::LimitKind::Quota, "org")?;
    Ok(OrgLimitOverrides {
        rate_limit: rate_limit.map(str::to_string),
        quota: quota.map(str::to_string),
    })
}

/// The org's limit-name overrides, from the store cache or the DB on miss.
/// Cache failures fall back to the DB. DB failures stop the selected checks;
/// substituting empty overrides could select a more permissive policy.
pub async fn limit_overrides(org_id: &str) -> Result<OrgLimitOverrides, crate::err::ErrorResponse> {
    let store = use_store();
    let key = ctx_key(org_id);

    match store.get::<OrgLimitOverrides>(&key).await {
        Ok(Some(cached)) => return Ok(cached),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(org_id, "failed to read org ctx cache: {error}");
        }
    }

    let overrides = match crate::db::get_organization_by_id(org_id).await {
        Ok(Some(org)) => overrides_from_attrs(&org.attrs)?,
        // A deleted org has no overrides; cache the empty value so a burst
        // of stale-session traffic doesn't hammer the DB.
        Ok(None) => OrgLimitOverrides::default(),
        Err(error) => {
            tracing::warn!(org_id, "failed to load org for limit overrides: {error}");
            return Err(crate::err::ErrorResponse::new(
                crate::err::ErrorCode::GatewayLimiterUnavailable,
            ));
        }
    };

    if let Err(error) = store.set(&key, &overrides, Some(ORG_CTX_TTL_SECS)).await {
        tracing::warn!(org_id, "failed to cache org ctx: {error}");
    }

    Ok(overrides)
}

/// Drop the cached org context so the next request re-reads fresh attrs.
/// Called by the admin org update/delete handlers.
pub async fn invalidate(org_id: &str) {
    if let Err(error) = use_store().delete(&ctx_key(org_id)).await {
        tracing::warn!(org_id, "failed to invalidate org ctx cache: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_limit_overrides_from_attrs() {
        let attrs = json!({ "rate_limit": "org-premium", "quota": "org-monthly" });
        let overrides = overrides_from_attrs(&attrs).unwrap();
        assert_eq!(overrides.rate_limit.as_deref(), Some("org-premium"));
        assert_eq!(overrides.quota.as_deref(), Some("org-monthly"));
    }

    #[test]
    fn accepts_camel_case_rate_limit() {
        let attrs = json!({ "rateLimit": "org-premium" });
        let overrides = overrides_from_attrs(&attrs).unwrap();
        assert_eq!(overrides.rate_limit.as_deref(), Some("org-premium"));
    }

    #[test]
    fn absent_and_null_attributes_disable_overrides_but_other_types_are_invalid() {
        assert_eq!(
            overrides_from_attrs(&json!({})).unwrap(),
            OrgLimitOverrides::default()
        );
        assert_eq!(
            overrides_from_attrs(&json!({ "rate_limit": null, "quota": null })).unwrap(),
            OrgLimitOverrides::default()
        );
        assert!(overrides_from_attrs(&json!({"rate_limit":5})).is_err());
        assert!(overrides_from_attrs(&json!({"quota":false})).is_err());
    }
}
