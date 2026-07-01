import { getTranslation } from "@/i18n";
import { AdminKeyPermission } from "@/services/types";

export const adminKeyPermissionValues: AdminKeyPermission[] = [
  AdminKeyPermission.AccessControl,
  AdminKeyPermission.Users,
  AdminKeyPermission.Organizations,
  AdminKeyPermission.ApiKeys,
  AdminKeyPermission.OAuthClients,
  AdminKeyPermission.ServiceAccounts,
  AdminKeyPermission.Configurations,
];

export const permissionTranslationKeys: Record<AdminKeyPermission, string> = {
  [AdminKeyPermission.AccessControl]: "adminPermissions.accessControl",
  [AdminKeyPermission.Users]: "adminPermissions.users",
  [AdminKeyPermission.Organizations]: "adminPermissions.organizations",
  [AdminKeyPermission.ApiKeys]: "adminPermissions.apiKeys",
  [AdminKeyPermission.OAuthClients]: "adminPermissions.oauthClients",
  [AdminKeyPermission.ServiceAccounts]: "adminPermissions.serviceAccounts",
  [AdminKeyPermission.Configurations]: "adminPermissions.configurations",
};

export function isAdminKeyPermission(
  permission: string,
): permission is AdminKeyPermission {
  return adminKeyPermissionValues.some((value) => value === permission);
}

export function getAdminKeyPermissionLabel(
  permission: AdminKeyPermission | string,
) {
  if (permission === "super_admin") {
    return getTranslation("adminPermissions.superAdmin");
  }

  const translationKey =
    permissionTranslationKeys[permission as AdminKeyPermission];

  return translationKey ? getTranslation(translationKey) : permission;
}
