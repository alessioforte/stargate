use super::{AdminAuthenticationKind, AdminPrincipal};
use crate::err::{ErrorCode, ErrorResponse};
use axum::extract::Request;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

const MAX_PERMISSION_WARNINGS: usize = 1024;
static WARNED_PERMISSION_VALUES: LazyLock<Mutex<HashSet<(String, String)>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    utoipa::ToSchema,
)]
pub enum Permission {
    #[serde(rename = "users:read")]
    UsersRead,
    #[serde(rename = "users:create")]
    UsersCreate,
    #[serde(rename = "users:invite")]
    UsersInvite,
    #[serde(rename = "users:update")]
    UsersUpdate,
    #[serde(rename = "users:update_attrs")]
    UsersUpdateAttrs,
    #[serde(rename = "users:delete")]
    UsersDelete,
    #[serde(rename = "super_admins:read")]
    SuperAdminsRead,
    #[serde(rename = "memberships:read")]
    MembershipsRead,
    #[serde(rename = "memberships:create")]
    MembershipsCreate,
    #[serde(rename = "memberships:update")]
    MembershipsUpdate,
    #[serde(rename = "memberships:delete")]
    MembershipsDelete,
    #[serde(rename = "sessions:read")]
    SessionsRead,
    #[serde(rename = "sessions:revoke")]
    SessionsRevoke,
    #[serde(rename = "organizations:read")]
    OrganizationsRead,
    #[serde(rename = "organizations:create")]
    OrganizationsCreate,
    #[serde(rename = "organizations:update")]
    OrganizationsUpdate,
    #[serde(rename = "organizations:delete")]
    OrganizationsDelete,
    #[serde(rename = "api_keys:read")]
    ApiKeysRead,
    #[serde(rename = "api_keys:create")]
    ApiKeysCreate,
    #[serde(rename = "api_keys:update_attrs")]
    ApiKeysUpdateAttrs,
    #[serde(rename = "api_keys:revoke")]
    ApiKeysRevoke,
    #[serde(rename = "api_keys:delete")]
    ApiKeysDelete,
    #[serde(rename = "service_accounts:read")]
    ServiceAccountsRead,
    #[serde(rename = "service_accounts:create")]
    ServiceAccountsCreate,
    #[serde(rename = "service_accounts:update")]
    ServiceAccountsUpdate,
    #[serde(rename = "service_accounts:delete")]
    ServiceAccountsDelete,
    #[serde(rename = "oauth_clients:read")]
    OAuthClientsRead,
    #[serde(rename = "oauth_clients:create")]
    OAuthClientsCreate,
    #[serde(rename = "oauth_clients:update")]
    OAuthClientsUpdate,
    #[serde(rename = "oauth_clients:update_status")]
    OAuthClientsUpdateStatus,
    #[serde(rename = "oauth_clients:rotate_secret")]
    OAuthClientsRotateSecret,
    #[serde(rename = "oauth_clients:delete")]
    OAuthClientsDelete,
    #[serde(rename = "oauth_tokens:introspect")]
    OAuthTokensIntrospect,
    #[serde(rename = "oauth_tokens:revoke")]
    OAuthTokensRevoke,
    #[serde(rename = "configurations:read")]
    ConfigurationsRead,
    #[serde(rename = "configurations:update")]
    ConfigurationsUpdate,
    #[serde(rename = "access_control:read")]
    AccessControlRead,
    #[serde(rename = "access_control:update")]
    AccessControlUpdate,
    #[serde(rename = "access_control:validate")]
    AccessControlValidate,
    #[serde(rename = "access_control:evaluate")]
    AccessControlEvaluate,
    #[serde(rename = "audit:read")]
    AuditRead,
    #[serde(rename = "health:read")]
    HealthRead,
    #[serde(rename = "overview:read")]
    OverviewRead,
}

impl Permission {
    pub const ALL: &'static [Self] = &[
        Self::UsersRead,
        Self::UsersCreate,
        Self::UsersInvite,
        Self::UsersUpdate,
        Self::UsersUpdateAttrs,
        Self::UsersDelete,
        Self::SuperAdminsRead,
        Self::MembershipsRead,
        Self::MembershipsCreate,
        Self::MembershipsUpdate,
        Self::MembershipsDelete,
        Self::SessionsRead,
        Self::SessionsRevoke,
        Self::OrganizationsRead,
        Self::OrganizationsCreate,
        Self::OrganizationsUpdate,
        Self::OrganizationsDelete,
        Self::ApiKeysRead,
        Self::ApiKeysCreate,
        Self::ApiKeysUpdateAttrs,
        Self::ApiKeysRevoke,
        Self::ApiKeysDelete,
        Self::ServiceAccountsRead,
        Self::ServiceAccountsCreate,
        Self::ServiceAccountsUpdate,
        Self::ServiceAccountsDelete,
        Self::OAuthClientsRead,
        Self::OAuthClientsCreate,
        Self::OAuthClientsUpdate,
        Self::OAuthClientsUpdateStatus,
        Self::OAuthClientsRotateSecret,
        Self::OAuthClientsDelete,
        Self::OAuthTokensIntrospect,
        Self::OAuthTokensRevoke,
        Self::ConfigurationsRead,
        Self::ConfigurationsUpdate,
        Self::AccessControlRead,
        Self::AccessControlUpdate,
        Self::AccessControlValidate,
        Self::AccessControlEvaluate,
        Self::AuditRead,
        Self::HealthRead,
        Self::OverviewRead,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UsersRead => "users:read",
            Self::UsersCreate => "users:create",
            Self::UsersInvite => "users:invite",
            Self::UsersUpdate => "users:update",
            Self::UsersUpdateAttrs => "users:update_attrs",
            Self::UsersDelete => "users:delete",
            Self::SuperAdminsRead => "super_admins:read",
            Self::MembershipsRead => "memberships:read",
            Self::MembershipsCreate => "memberships:create",
            Self::MembershipsUpdate => "memberships:update",
            Self::MembershipsDelete => "memberships:delete",
            Self::SessionsRead => "sessions:read",
            Self::SessionsRevoke => "sessions:revoke",
            Self::OrganizationsRead => "organizations:read",
            Self::OrganizationsCreate => "organizations:create",
            Self::OrganizationsUpdate => "organizations:update",
            Self::OrganizationsDelete => "organizations:delete",
            Self::ApiKeysRead => "api_keys:read",
            Self::ApiKeysCreate => "api_keys:create",
            Self::ApiKeysUpdateAttrs => "api_keys:update_attrs",
            Self::ApiKeysRevoke => "api_keys:revoke",
            Self::ApiKeysDelete => "api_keys:delete",
            Self::ServiceAccountsRead => "service_accounts:read",
            Self::ServiceAccountsCreate => "service_accounts:create",
            Self::ServiceAccountsUpdate => "service_accounts:update",
            Self::ServiceAccountsDelete => "service_accounts:delete",
            Self::OAuthClientsRead => "oauth_clients:read",
            Self::OAuthClientsCreate => "oauth_clients:create",
            Self::OAuthClientsUpdate => "oauth_clients:update",
            Self::OAuthClientsUpdateStatus => "oauth_clients:update_status",
            Self::OAuthClientsRotateSecret => "oauth_clients:rotate_secret",
            Self::OAuthClientsDelete => "oauth_clients:delete",
            Self::OAuthTokensIntrospect => "oauth_tokens:introspect",
            Self::OAuthTokensRevoke => "oauth_tokens:revoke",
            Self::ConfigurationsRead => "configurations:read",
            Self::ConfigurationsUpdate => "configurations:update",
            Self::AccessControlRead => "access_control:read",
            Self::AccessControlUpdate => "access_control:update",
            Self::AccessControlValidate => "access_control:validate",
            Self::AccessControlEvaluate => "access_control:evaluate",
            Self::AuditRead => "audit:read",
            Self::HealthRead => "health:read",
            Self::OverviewRead => "overview:read",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|permission| permission.as_str() == value)
    }
}

#[derive(Clone, Debug)]
pub struct AdminAuthorization {
    pub principal: AdminPrincipal,
    pub super_admin: bool,
    permissions: HashSet<Permission>,
}

impl AdminAuthorization {
    pub fn super_admin_user(
        user_id: String,
        kind: AdminAuthenticationKind,
        claims: Box<jwt::Claims>,
    ) -> Self {
        Self {
            principal: AdminPrincipal::User {
                user_id,
                kind,
                claims,
            },
            super_admin: true,
            permissions: Permission::ALL.iter().copied().collect(),
        }
    }

    pub fn admin_key(principal: AdminPrincipal, permissions: HashSet<Permission>) -> Self {
        Self {
            principal,
            super_admin: false,
            permissions,
        }
    }

    pub fn has(&self, permission: Permission) -> bool {
        self.super_admin || self.permissions.contains(&permission)
    }

    pub fn require(&self, permission: Permission) -> Result<(), ErrorResponse> {
        if self.has(permission) {
            Ok(())
        } else {
            Err(ErrorResponse::new(ErrorCode::AuthInsufficientPermissions))
        }
    }

    pub fn require_all(
        &self,
        permissions: impl IntoIterator<Item = Permission>,
    ) -> Result<(), ErrorResponse> {
        for permission in permissions {
            self.require(permission)?;
        }
        Ok(())
    }

    pub fn require_super_admin(&self) -> Result<(), ErrorResponse> {
        if self.super_admin {
            Ok(())
        } else {
            Err(ErrorResponse::new(ErrorCode::AuthInsufficientPermissions))
        }
    }

    pub fn permissions(&self) -> impl Iterator<Item = Permission> + '_ {
        Permission::ALL
            .iter()
            .copied()
            .filter(|permission| self.has(*permission))
    }
}

pub fn get(req: &Request) -> Result<&AdminAuthorization, ErrorResponse> {
    req.extensions()
        .get::<AdminAuthorization>()
        .ok_or_else(|| ErrorResponse::new(ErrorCode::AuthInsufficientPermissions))
}

pub fn require(req: &Request, permission: Permission) -> Result<(), ErrorResponse> {
    get(req)?.require(permission)
}

pub fn require_super_admin(req: &Request) -> Result<(), ErrorResponse> {
    get(req)?.require_super_admin()
}

pub fn parse_stored_permissions(
    key_id: &str,
    values: impl IntoIterator<Item = impl AsRef<str>>,
) -> HashSet<Permission> {
    let mut permissions = HashSet::new();
    for value in values {
        let value = value.as_ref();
        if let Some(permission) = Permission::parse(value) {
            permissions.insert(permission);
            continue;
        }

        let legacy = legacy_permissions(value);
        if legacy.is_empty() {
            warn_stored_permission_once(key_id, value, "ignoring unknown admin-key permission");
            continue;
        }

        warn_stored_permission_once(key_id, value, "expanding legacy admin-key permission");
        permissions.extend(legacy.iter().copied());
    }
    permissions
}

fn warn_stored_permission_once(key_id: &str, permission: &str, message: &'static str) {
    let Ok(mut warned) = WARNED_PERMISSION_VALUES.lock() else {
        return;
    };
    if warned.len() >= MAX_PERMISSION_WARNINGS
        || !warned.insert((key_id.to_string(), permission.to_string()))
    {
        return;
    }
    tracing::warn!(admin_key_id = %key_id, permission = %permission, "{message}");
}

pub fn canonical_permissions(values: impl IntoIterator<Item = Permission>) -> Vec<Permission> {
    let values: HashSet<_> = values.into_iter().collect();
    Permission::ALL
        .iter()
        .copied()
        .filter(|permission| values.contains(permission))
        .collect()
}

pub fn canonical_permission_strings(values: impl IntoIterator<Item = Permission>) -> Vec<String> {
    canonical_permissions(values)
        .into_iter()
        .map(|permission| permission.as_str().to_string())
        .collect()
}

fn legacy_permissions(value: &str) -> &'static [Permission] {
    match value {
        "users" => &[
            Permission::UsersRead,
            Permission::UsersCreate,
            Permission::UsersInvite,
            Permission::UsersUpdate,
            Permission::UsersUpdateAttrs,
            Permission::UsersDelete,
            Permission::SuperAdminsRead,
            Permission::MembershipsRead,
            Permission::MembershipsCreate,
            Permission::MembershipsUpdate,
            Permission::MembershipsDelete,
        ],
        "organizations" => &[
            Permission::OrganizationsRead,
            Permission::OrganizationsCreate,
            Permission::OrganizationsUpdate,
            Permission::OrganizationsDelete,
        ],
        "api_keys" => &[
            Permission::ApiKeysRead,
            Permission::ApiKeysCreate,
            Permission::ApiKeysUpdateAttrs,
            Permission::ApiKeysRevoke,
            Permission::ApiKeysDelete,
        ],
        "oauth_clients" => &[
            Permission::OAuthClientsRead,
            Permission::OAuthClientsCreate,
            Permission::OAuthClientsUpdate,
            Permission::OAuthClientsUpdateStatus,
            Permission::OAuthClientsRotateSecret,
            Permission::OAuthClientsDelete,
        ],
        "oauth_tokens" => &[
            Permission::OAuthTokensIntrospect,
            Permission::OAuthTokensRevoke,
        ],
        "service_accounts" => &[
            Permission::ServiceAccountsRead,
            Permission::ServiceAccountsCreate,
            Permission::ServiceAccountsUpdate,
            Permission::ServiceAccountsDelete,
        ],
        "configurations" => &[
            Permission::ConfigurationsRead,
            Permission::ConfigurationsUpdate,
        ],
        "access_control" => &[
            Permission::AccessControlRead,
            Permission::AccessControlUpdate,
            Permission::AccessControlValidate,
            Permission::AccessControlEvaluate,
        ],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admin_key(permissions: impl IntoIterator<Item = Permission>) -> AdminAuthorization {
        AdminAuthorization::admin_key(
            AdminPrincipal::AdminKey {
                key_id: "key-1".to_string(),
            },
            permissions.into_iter().collect(),
        )
    }

    #[test]
    fn canonical_permissions_roundtrip() {
        let mut canonical = HashSet::new();
        for permission in Permission::ALL {
            assert!(canonical.insert(permission.as_str()));
            assert_eq!(Permission::parse(permission.as_str()), Some(*permission));
            assert_eq!(
                serde_json::to_value(permission).unwrap(),
                serde_json::Value::String(permission.as_str().to_string())
            );
        }
    }

    #[test]
    fn exact_permissions_do_not_imply_sibling_actions() {
        let authorization = admin_key([Permission::UsersRead]);

        assert!(authorization.require(Permission::UsersRead).is_ok());
        assert!(authorization.require(Permission::UsersCreate).is_err());
    }

    #[test]
    fn require_all_uses_and_semantics() {
        let authorization = admin_key([Permission::UsersCreate]);

        assert!(
            authorization
                .require_all([Permission::UsersCreate, Permission::MembershipsCreate])
                .is_err()
        );
    }

    #[test]
    fn legacy_permissions_expand_to_a_fixed_set() {
        let permissions = parse_stored_permissions("key-1", ["users"]);

        assert!(permissions.contains(&Permission::UsersRead));
        assert!(permissions.contains(&Permission::UsersDelete));
        assert!(permissions.contains(&Permission::SuperAdminsRead));
        assert!(permissions.contains(&Permission::MembershipsCreate));
        assert!(!permissions.contains(&Permission::SessionsRevoke));
    }

    #[test]
    fn unknown_stored_permissions_fail_closed() {
        assert!(parse_stored_permissions("key-1", ["users:*", "super_admin"]).is_empty());
    }

    #[test]
    fn canonical_strings_are_deduplicated_in_catalog_order() {
        assert_eq!(
            canonical_permission_strings([
                Permission::UsersCreate,
                Permission::UsersRead,
                Permission::UsersCreate,
            ]),
            ["users:read", "users:create"]
        );
    }
}
