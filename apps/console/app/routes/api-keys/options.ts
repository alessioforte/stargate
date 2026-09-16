import type { ServiceAccount, User } from "@/services/types";

export function userOptions(users: User[], selectedUserId?: string | null) {
  const options = users.map((user) => ({
    value: user.id,
    label: `${user.email} (${user.nickname})`,
  }));

  if (
    selectedUserId &&
    !options.some((option) => option.value === selectedUserId)
  ) {
    options.push({
      value: selectedUserId,
      label: selectedUserId,
    });
  }

  return options;
}

export function serviceAccountOptions(
  serviceAccounts: ServiceAccount[],
  selectedServiceAccountId?: string | null,
) {
  const options = serviceAccounts.map((serviceAccount) => ({
    value: serviceAccount.id,
    label: `${serviceAccount.name} (${serviceAccount.id})`,
  }));

  if (
    selectedServiceAccountId &&
    !options.some((option) => option.value === selectedServiceAccountId)
  ) {
    options.push({
      value: selectedServiceAccountId,
      label: selectedServiceAccountId,
    });
  }

  return options;
}
