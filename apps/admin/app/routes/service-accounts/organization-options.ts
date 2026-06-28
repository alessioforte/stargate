import type { Organization } from "@/services/types";

export function organizationOptions(
  organizations: Organization[],
  selectedOrgId?: string | null,
) {
  const options = organizations.map((organization) => ({
    value: organization.id,
    label: `${organization.name} (${organization.id})`,
  }));

  if (
    selectedOrgId &&
    !options.some((option) => option.value === selectedOrgId)
  ) {
    options.push({
      value: selectedOrgId,
      label: selectedOrgId,
    });
  }

  return options;
}
