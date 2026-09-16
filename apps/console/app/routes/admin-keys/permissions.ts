import { getTranslation } from "@/i18n";
import {
  AdminKeyPermission,
  type AdminKeyPermission as AdminKeyPermissionValue,
} from "@/services/types";

interface AdminKeyPermissionGroup {
  labelKey: string;
  permissions: AdminKeyPermissionValue[];
}

export const adminKeyPermissionGroups: AdminKeyPermissionGroup[] = [
  {
    labelKey: "adminPermissions.resources.users",
    permissions: [
      AdminKeyPermission.UsersRead,
      AdminKeyPermission.UsersCreate,
      AdminKeyPermission.UsersInvite,
      AdminKeyPermission.UsersUpdate,
      AdminKeyPermission.UsersUpdateAttrs,
      AdminKeyPermission.UsersDelete,
    ],
  },
  {
    labelKey: "adminPermissions.resources.superAdmins",
    permissions: [AdminKeyPermission.SuperAdminsRead],
  },
  {
    labelKey: "adminPermissions.resources.memberships",
    permissions: [
      AdminKeyPermission.MembershipsRead,
      AdminKeyPermission.MembershipsCreate,
      AdminKeyPermission.MembershipsUpdate,
      AdminKeyPermission.MembershipsDelete,
    ],
  },
  {
    labelKey: "adminPermissions.resources.sessions",
    permissions: [
      AdminKeyPermission.SessionsRead,
      AdminKeyPermission.SessionsRevoke,
    ],
  },
  {
    labelKey: "adminPermissions.resources.organizations",
    permissions: [
      AdminKeyPermission.OrganizationsRead,
      AdminKeyPermission.OrganizationsCreate,
      AdminKeyPermission.OrganizationsUpdate,
      AdminKeyPermission.OrganizationsDelete,
    ],
  },
  {
    labelKey: "adminPermissions.resources.apiKeys",
    permissions: [
      AdminKeyPermission.ApiKeysRead,
      AdminKeyPermission.ApiKeysCreate,
      AdminKeyPermission.ApiKeysUpdateAttrs,
      AdminKeyPermission.ApiKeysRevoke,
      AdminKeyPermission.ApiKeysDelete,
    ],
  },
  {
    labelKey: "adminPermissions.resources.serviceAccounts",
    permissions: [
      AdminKeyPermission.ServiceAccountsRead,
      AdminKeyPermission.ServiceAccountsCreate,
      AdminKeyPermission.ServiceAccountsUpdate,
      AdminKeyPermission.ServiceAccountsDelete,
    ],
  },
  {
    labelKey: "adminPermissions.resources.oauthClients",
    permissions: [
      AdminKeyPermission.OAuthClientsRead,
      AdminKeyPermission.OAuthClientsCreate,
      AdminKeyPermission.OAuthClientsUpdate,
      AdminKeyPermission.OAuthClientsUpdateStatus,
      AdminKeyPermission.OAuthClientsRotateSecret,
      AdminKeyPermission.OAuthClientsDelete,
    ],
  },
  {
    labelKey: "adminPermissions.resources.oauthTokens",
    permissions: [
      AdminKeyPermission.OAuthTokensIntrospect,
      AdminKeyPermission.OAuthTokensRevoke,
    ],
  },
  {
    labelKey: "adminPermissions.resources.configurations",
    permissions: [
      AdminKeyPermission.ConfigurationsRead,
      AdminKeyPermission.ConfigurationsUpdate,
    ],
  },
  {
    labelKey: "adminPermissions.resources.accessControl",
    permissions: [
      AdminKeyPermission.AccessControlRead,
      AdminKeyPermission.AccessControlUpdate,
      AdminKeyPermission.AccessControlValidate,
      AdminKeyPermission.AccessControlEvaluate,
    ],
  },
  {
    labelKey: "adminPermissions.resources.audit",
    permissions: [AdminKeyPermission.AuditRead],
  },
  {
    labelKey: "adminPermissions.resources.health",
    permissions: [AdminKeyPermission.HealthRead],
  },
  {
    labelKey: "adminPermissions.resources.overview",
    permissions: [AdminKeyPermission.OverviewRead],
  },
];

export const adminKeyPermissionValues = adminKeyPermissionGroups.flatMap(
  (group) => group.permissions,
);

const actionTranslationKeys: Record<string, string> = {
  read: "adminPermissions.actions.read",
  create: "adminPermissions.actions.create",
  invite: "adminPermissions.actions.invite",
  update: "adminPermissions.actions.update",
  update_attrs: "adminPermissions.actions.updateAttrs",
  delete: "adminPermissions.actions.delete",
  revoke: "adminPermissions.actions.revoke",
  update_status: "adminPermissions.actions.updateStatus",
  rotate_secret: "adminPermissions.actions.rotateSecret",
  introspect: "adminPermissions.actions.introspect",
  validate: "adminPermissions.actions.validate",
  evaluate: "adminPermissions.actions.evaluate",
};

function permissionGroup(permission: AdminKeyPermissionValue) {
  return adminKeyPermissionGroups.find((group) =>
    group.permissions.includes(permission),
  );
}

export function isAdminKeyPermission(
  permission: string,
): permission is AdminKeyPermissionValue {
  return adminKeyPermissionValues.some((value) => value === permission);
}

export function getAdminKeyPermissionActionLabel(
  permission: AdminKeyPermissionValue,
) {
  const action = permission.split(":", 2)[1];
  const translationKey = actionTranslationKeys[action];
  return translationKey ? getTranslation(translationKey) : action;
}

export function getAdminKeyPermissionLabel(
  permission: AdminKeyPermissionValue | string,
) {
  if (!isAdminKeyPermission(permission)) return permission;

  const group = permissionGroup(permission);
  const resource = group ? getTranslation(group.labelKey) : permission;
  return `${resource}: ${getAdminKeyPermissionActionLabel(permission)}`;
}
